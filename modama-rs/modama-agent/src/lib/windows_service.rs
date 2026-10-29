use std::{
    ffi::{OsStr, OsString},
    fs,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use env_logger::Target;
use file_rotate::{ContentLimit, FileRotate, compression::Compression, suffix::AppendCount};
use log::{error, info};
use widestring::U16CString;
use windows::{
    Win32::{
        Foundation::{
            ERROR_SERVICE_DOES_NOT_EXIST, ERROR_SERVICE_MARKED_FOR_DELETE, HLOCAL, LocalFree,
        },
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            DACL_SECURITY_INFORMATION, LookupAccountNameW, PROTECTED_DACL_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR, PSID, SID_NAME_USE, SetFileSecurityW,
        },
        UI::Shell::{
            CSIDL_APPDATA, CSIDL_COMMON_APPDATA, CSIDL_PROGRAM_FILES, SHGFP_TYPE_CURRENT,
            SHGetFolderPathW,
        },
    },
    core::{PCWSTR, PWSTR},
};
use windows_service::{
    define_windows_service,
    service::{
        ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
        ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
        ServiceInfo, ServiceSidType, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle},
    service_dispatcher,
    service_manager::{ServiceManager, ServiceManagerAccess},
};

use crate::{config::AgentConfig, runtime::run_agent};

const SERVICE_NAME: &str = "modama-agent";
const SERVICE_DISPLAY_NAME: &str = "Modama Agent";
// CreateService treats `NT SERVICE\<service-name>` as a passwordless virtual
// account. Windows manages the identity; the installer must not create a local
// SAM user. Virtual accounts are supported on Windows 7 / Server 2008 R2+.
// https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-createservicew
// https://learn.microsoft.com/windows-server/identity/ad-ds/manage/understand-service-accounts#virtual-accounts
const SERVICE_ACCOUNT: &str = r"NT SERVICE\modama-agent";
const CONFIG_FILE: &str = "modama-agent.toml";
const EXECUTABLE_FILE: &str = "modama-agent.exe";
const START_STOP_TIMEOUT: Duration = Duration::from_secs(30);

static SERVICE_CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();

// `define_windows_service!` supplies the callback shape required by the SCM's
// SERVICE_TABLE_ENTRY / StartServiceCtrlDispatcher contract.
// https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-startservicectrldispatcherw
define_windows_service!(ffi_service_main, service_main);

fn known_folder(csidl: u32) -> Result<PathBuf> {
    let mut path = [0u16; 260];
    // SAFETY: `path` is a live, writable MAX_PATH-sized UTF-16 buffer for the
    // duration of the call. `hwnd` and `hToken` are allowed to be NULL, and the
    // supplied CSIDL values identify real filesystem folders. On success the
    // API writes a NUL-terminated path without retaining the buffer pointer.
    // SHGetFolderPathW is used instead of SHGetKnownFolderPath so the resulting
    // binary does not import CoTaskMemFree from post-Windows-7 combase.dll.
    // https://learn.microsoft.com/windows/win32/api/shlobj_core/nf-shlobj_core-shgetfolderpathw
    unsafe {
        SHGetFolderPathW(
            None,
            csidl as i32,
            None,
            SHGFP_TYPE_CURRENT.0 as u32,
            &mut path,
        )?;
    }
    let length = path
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(path.len());
    Ok(PathBuf::from(OsString::from_wide(&path[..length])))
}

fn installation_paths() -> Result<(PathBuf, PathBuf, PathBuf)> {
    let executable = known_folder(CSIDL_PROGRAM_FILES)?
        .join("modama")
        .join("modama-agent")
        .join(EXECUTABLE_FILE);
    let data_dir = known_folder(CSIDL_COMMON_APPDATA)?
        .join("modama")
        .join("modama-agent");
    let config = data_dir.join(CONFIG_FILE);
    Ok((executable, config, data_dir.join("logs")))
}

pub fn resolve_install_config(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_owned());
    }
    let (_, machine_config, _) = installation_paths()?;
    if machine_config.exists() {
        return Ok(machine_config);
    }
    user_config_path()
}

pub fn user_config_path() -> Result<PathBuf> {
    Ok(known_folder(CSIDL_APPDATA)?
        .join("modama")
        .join("modama-agent")
        .join("config")
        .join(CONFIG_FILE))
}

fn service_info(executable: PathBuf, config: &Path) -> ServiceInfo {
    // `windows-service` passes this to CreateServiceW. In particular, a virtual
    // account must be named `NT SERVICE\ServiceName` and have a NULL password.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-createservicew
    ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: executable,
        launch_arguments: vec![
            OsString::from("run-service"),
            OsString::from("--config-path"),
            config.as_os_str().to_owned(),
        ],
        dependencies: vec![],
        account_name: Some(OsString::from(SERVICE_ACCOUNT)),
        account_password: None,
    }
}

fn manager(access: ServiceManagerAccess) -> Result<ServiceManager> {
    // ServiceManager::local_computer wraps OpenSCManagerW.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-openscmanagerw
    ServiceManager::local_computer(None::<&str>, access).map_err(Into::into)
}

fn service_exists(service_manager: &ServiceManager) -> Result<bool> {
    // ServiceManager::open_service wraps OpenServiceW. A service marked for
    // deletion still exists until every open handle has been closed.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-openservicew
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-deleteservice
    match service_manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
        Ok(_) => Ok(true),
        Err(windows_service::Error::Winapi(error))
            if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST.0 as i32) =>
        {
            Ok(false)
        }
        Err(windows_service::Error::Winapi(error))
            if error.raw_os_error() == Some(ERROR_SERVICE_MARKED_FOR_DELETE.0 as i32) =>
        {
            Ok(true)
        }
        Err(error) => Err(error.into()),
    }
}

fn set_path_acl(path: &Path, sddl: &str) -> Result<()> {
    let wide_path = U16CString::from_os_str(path.as_os_str())
        .map_err(|_| anyhow!("path contains an interior NUL: {path:?}"))?;
    let wide_sddl = U16CString::from_str(sddl)
        .map_err(|_| anyhow!("security descriptor contains an interior NUL"))?;
    let mut descriptor = PSECURITY_DESCRIPTOR::default();

    // SAFETY: `wide_path` and `wide_sddl` are NUL-terminated and remain alive
    // throughout both calls. On successful conversion, Windows initializes
    // `descriptor` with a self-relative descriptor allocated by LocalAlloc.
    // SetFileSecurityW only borrows it for the duration of the call, after which
    // we release that exact allocation once with LocalFree. On conversion
    // failure no allocation is returned and `?` exits before LocalFree.
    // https://learn.microsoft.com/windows/win32/api/sddl/nf-sddl-convertstringsecuritydescriptortosecuritydescriptorw
    // https://learn.microsoft.com/windows/win32/api/securitybaseapi/nf-securitybaseapi-setfilesecurityw
    // https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-localfree
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR::from_raw(wide_sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
        let result = SetFileSecurityW(
            PCWSTR::from_raw(wide_path.as_ptr()),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
        .ok();
        LocalFree(Some(HLOCAL(descriptor.0)));
        result.with_context(|| format!("could not protect {path:?}"))
    }
}

fn service_sid_string() -> Result<String> {
    let account = U16CString::from_str(SERVICE_ACCOUNT)
        .map_err(|_| anyhow!("service account contains an interior NUL"))?;
    let mut sid_bytes = 0u32;
    let mut domain_chars = 0u32;
    let mut sid_type = SID_NAME_USE::default();

    // A service SID uses the account form `NT SERVICE\SvcName`; it is distinct
    // from creating a user and can be resolved after the service is registered.
    // https://learn.microsoft.com/windows/win32/api/winsvc/ns-winsvc-service_sid_info
    // https://learn.microsoft.com/windows/win32/services/service-security-and-access-rights

    // SAFETY: `account` is a live NUL-terminated string. LookupAccountNameW
    // explicitly permits NULL output buffers when their input sizes are zero;
    // this sizing call is expected to report insufficient buffer and populate
    // `sid_bytes` and `domain_chars`. No pointer is retained.
    // https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-lookupaccountnamew
    let _ = unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            PCWSTR::from_raw(account.as_ptr()),
            None,
            &mut sid_bytes,
            None,
            &mut domain_chars,
            &mut sid_type,
        )
    };
    if sid_bytes == 0 {
        bail!("Windows did not resolve the virtual service account {SERVICE_ACCOUNT}");
    }

    let word_size = std::mem::size_of::<usize>();
    let mut sid_storage = vec![0usize; (sid_bytes as usize).div_ceil(word_size)];
    let mut domain = vec![0u16; domain_chars as usize];
    let sid = PSID(sid_storage.as_mut_ptr().cast());
    let domain_buffer = (!domain.is_empty()).then(|| PWSTR::from_raw(domain.as_mut_ptr()));
    // SAFETY: `sid_storage` is usize-aligned and has at least `sid_bytes` bytes;
    // `domain` has the character count returned by the sizing call. Both buffers
    // and `account` remain alive and exclusively borrowed while Windows writes
    // them. A successful lookup initializes a valid SID. ConvertSidToStringSidW
    // then allocates a NUL-terminated string with LocalAlloc; we copy it before
    // releasing that exact pointer once with LocalFree. No pointer escapes.
    // https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-lookupaccountnamew
    // https://learn.microsoft.com/windows/win32/api/sddl/nf-sddl-convertsidtostringsidw
    // https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-localfree
    unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            PCWSTR::from_raw(account.as_ptr()),
            Some(sid),
            &mut sid_bytes,
            domain_buffer,
            &mut domain_chars,
            &mut sid_type,
        )?;

        let mut string_sid = PWSTR::null();
        ConvertSidToStringSidW(sid, &mut string_sid)?;
        let value = string_sid.to_string();
        LocalFree(Some(HLOCAL(string_sid.0.cast())));
        value.map_err(Into::into)
    }
}

fn copy_if_different(source: &Path, destination: &Path) -> Result<()> {
    let same_file = source.canonicalize().ok() == destination.canonicalize().ok();
    if !same_file {
        fs::copy(source, destination)
            .with_context(|| format!("could not copy {:?} to {:?}", source, destination))?;
    }
    Ok(())
}

pub fn install(source_config: &Path) -> Result<()> {
    let source_config = source_config
        .canonicalize()
        .with_context(|| format!("could not resolve config {source_config:?}"))?;
    tokio::runtime::Runtime::new()?.block_on(AgentConfig::from_file(&source_config))?;
    println!("Validated configuration: {}", source_config.display());

    let service_manager =
        manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
    if service_exists(&service_manager)? {
        bail!("Windows service {SERVICE_NAME} is already installed");
    }

    let (installed_executable, installed_config, log_dir) = installation_paths()?;
    let executable_dir = installed_executable
        .parent()
        .ok_or_else(|| anyhow!("installed executable has no parent directory"))?;
    let data_dir = installed_config
        .parent()
        .ok_or_else(|| anyhow!("installed config has no parent directory"))?;
    fs::create_dir_all(executable_dir)?;
    fs::create_dir_all(data_dir)?;
    // Protect the machine data directory before copying the configuration and
    // its embedded private key. `D:P` protects the DACL from parent inheritance;
    // `OICI` propagates the full-access (`FA`) SYSTEM (`SY`) and Administrators
    // (`BA`) ACEs to files and subdirectories created afterward. The service
    // SID is added only after CreateService makes that SID resolvable.
    // SDDL and ACE syntax:
    // https://learn.microsoft.com/windows/win32/secauthz/security-descriptor-string-format
    // https://learn.microsoft.com/windows/win32/secauthz/ace-strings
    set_path_acl(data_dir, "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)")?;

    let current_executable = std::env::current_exe()?;
    copy_if_different(&current_executable, &installed_executable)?;
    println!("Copied executable: {}", installed_executable.display());
    copy_if_different(&source_config, &installed_config)?;
    println!("Copied configuration: {}", installed_config.display());

    // ServiceManager::create_service wraps CreateServiceW and registers both
    // the executable command line and the virtual service logon identity.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-createservicew
    let service = service_manager.create_service(
        &service_info(installed_executable.clone(), &installed_config),
        ServiceAccess::CHANGE_CONFIG
            | ServiceAccess::DELETE
            | ServiceAccess::QUERY_STATUS
            | ServiceAccess::START
            | ServiceAccess::STOP,
    )?;
    let configuration_result = (|| -> Result<()> {
        // ChangeServiceConfig2W(SERVICE_CONFIG_SERVICE_SID_INFO) requests that
        // Windows add this service's SID to its process token on service start.
        // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-changeserviceconfig2w
        // https://learn.microsoft.com/windows/win32/api/winsvc/ns-winsvc-service_sid_info
        service.set_config_service_sid_info(ServiceSidType::Unrestricted)?;
        let service_sid = service_sid_string()?;

        // Keep the protected SYSTEM/Administrators DACL and add inheritable
        // generic-read/generic-execute (`GRGX`) for only this service SID. This
        // permits traversal and gives future children a read-only inherited ACE.
        set_path_acl(
            data_dir,
            &format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;GRGX;;;{service_sid})"),
        )?;
        // SetFileSecurityW does not propagate a changed directory DACL to an
        // existing child, and the config was copied before the service SID was
        // resolvable. Protect the file explicitly: SYSTEM and Administrators
        // retain full access (`FA`); the service receives read-only access (`GR`)
        // to the configuration and its embedded private key.
        // https://learn.microsoft.com/windows/win32/api/securitybaseapi/nf-securitybaseapi-setfilesecurityw
        set_path_acl(
            &installed_config,
            &format!("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;GR;;;{service_sid})"),
        )?;
        fs::create_dir_all(&log_dir)?;
        // The log subtree gets its own protected DACL. SYSTEM, Administrators,
        // and this service SID receive inheritable full access because the
        // service must create, append, rotate, and delete its log files.
        set_path_acl(
            &log_dir,
            &format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{service_sid})"),
        )?;

        // Description and recovery policy are ChangeServiceConfig2W information
        // levels. Restart crashes and reported nonzero exits after 5s, 30s,
        // then 60s; Windows repeats the final action until 24 failure-free hours
        // reset the failure count.
        // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-changeserviceconfig2w
        // https://learn.microsoft.com/windows/win32/api/winsvc/ns-winsvc-service_failure_actionsw
        // https://learn.microsoft.com/windows/win32/api/winsvc/ns-winsvc-service_failure_actions_flag
        service.set_description("Connects this system to Modama ingest services.")?;
        service.update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(24 * 60 * 60)),
            reboot_msg: None,
            command: None,
            actions: Some(vec![
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: Duration::from_secs(5),
                },
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: Duration::from_secs(30),
                },
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: Duration::from_secs(60),
                },
            ]),
        })?;
        // Include failures that the service reports through SERVICE_STOPPED
        // with a nonzero exit code, not just crashes or abrupt termination.
        // https://learn.microsoft.com/windows/win32/api/winsvc/ns-winsvc-service_failure_actions_flag
        service.set_failure_actions_on_non_crash_failures(true)?;
        Ok(())
    })();
    if let Err(error) = configuration_result {
        // Roll back the CreateServiceW registration if any mandatory
        // ChangeServiceConfig2W or ACL step fails. DeleteService is deferred
        // until the handle closes; the copied files are deliberately retained.
        // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-deleteservice
        let _ = service.delete();
        return Err(error).context("could not configure service; registration was rolled back");
    }
    println!("Protected machine data: {}", data_dir.display());
    println!("Configured rotating logs: {}", log_dir.display());
    println!("Registered Windows service: {SERVICE_NAME}");

    // StartServiceW returns after requesting startup; wait_for_state observes
    // the independently reported transition to SERVICE_RUNNING.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-startservicew
    service.start::<&OsStr>(&[])?;
    wait_for_state(&service, ServiceState::Running, START_STOP_TIMEOUT)?;
    println!("Started Windows service: {SERVICE_NAME}");
    Ok(())
}

fn open_service(access: ServiceAccess) -> Result<windows_service::service::Service> {
    // OpenServiceW checks the requested access mask against the service object.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-openservicew
    manager(ServiceManagerAccess::CONNECT)?
        .open_service(SERVICE_NAME, access)
        .map_err(Into::into)
}

fn wait_for_state(
    service: &windows_service::service::Service,
    desired: ServiceState,
    timeout: Duration,
) -> Result<ServiceStatus> {
    // query_status wraps QueryServiceStatusEx. Pending states are polled rather
    // than assuming StartServiceW or ControlService completed synchronously.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-queryservicestatusex
    let started = Instant::now();
    loop {
        let current = service.query_status()?;
        if current.current_state == desired {
            return Ok(current);
        }
        if current.current_state == ServiceState::Stopped && desired != ServiceState::Stopped {
            bail!(
                "service stopped before reaching {desired:?}; exit code: {:?}",
                current.exit_code
            );
        }
        if started.elapsed() >= timeout {
            bail!(
                "timed out waiting for service state {desired:?}; current state: {:?}",
                current.current_state
            );
        }
        thread::sleep(Duration::from_millis(250));
    }
}

pub fn start() -> Result<()> {
    // Service::start wraps StartServiceW.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-startservicew
    let service = open_service(ServiceAccess::QUERY_STATUS | ServiceAccess::START)?;
    match service.query_status()?.current_state {
        ServiceState::Running => {}
        ServiceState::StartPending | ServiceState::ContinuePending => {
            wait_for_state(&service, ServiceState::Running, START_STOP_TIMEOUT)?;
        }
        ServiceState::StopPending => {
            wait_for_state(&service, ServiceState::Stopped, START_STOP_TIMEOUT)?;
            service.start::<&OsStr>(&[])?;
            wait_for_state(&service, ServiceState::Running, START_STOP_TIMEOUT)?;
        }
        ServiceState::Stopped => {
            service.start::<&OsStr>(&[])?;
            wait_for_state(&service, ServiceState::Running, START_STOP_TIMEOUT)?;
        }
        state => bail!("cannot start service while it is in state {state:?}"),
    }
    println!("Windows service is running: {SERVICE_NAME}");
    Ok(())
}

pub fn stop() -> Result<()> {
    let service = open_service(ServiceAccess::QUERY_STATUS | ServiceAccess::STOP)?;
    stop_open_service(&service)?;
    println!("Windows service is stopped: {SERVICE_NAME}");
    Ok(())
}

fn stop_open_service(service: &windows_service::service::Service) -> Result<()> {
    // Service::stop sends SERVICE_CONTROL_STOP through ControlService.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-controlservice
    match service.query_status()?.current_state {
        ServiceState::Stopped => Ok(()),
        ServiceState::StopPending => {
            wait_for_state(service, ServiceState::Stopped, START_STOP_TIMEOUT)?;
            Ok(())
        }
        ServiceState::StartPending | ServiceState::ContinuePending => {
            wait_for_state(service, ServiceState::Running, START_STOP_TIMEOUT)?;
            service.stop()?;
            wait_for_state(service, ServiceState::Stopped, START_STOP_TIMEOUT)?;
            Ok(())
        }
        ServiceState::PausePending => {
            wait_for_state(service, ServiceState::Paused, START_STOP_TIMEOUT)?;
            service.stop()?;
            wait_for_state(service, ServiceState::Stopped, START_STOP_TIMEOUT)?;
            Ok(())
        }
        ServiceState::Running | ServiceState::Paused => {
            service.stop()?;
            wait_for_state(service, ServiceState::Stopped, START_STOP_TIMEOUT)?;
            Ok(())
        }
    }
}

pub fn status() -> Result<()> {
    let current = open_service(ServiceAccess::QUERY_STATUS)?.query_status()?;
    println!("service: {SERVICE_NAME}");
    println!("state: {:?}", current.current_state);
    if let Some(process_id) = current.process_id {
        println!("pid: {process_id}");
    }
    println!("exit-code: {:?}", current.exit_code);
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let service_manager = manager(ServiceManagerAccess::CONNECT)?;
    let service = service_manager.open_service(
        SERVICE_NAME,
        ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS | ServiceAccess::STOP,
    )?;
    stop_open_service(&service)?;
    // DeleteService marks the service for deletion; removal is deferred until
    // all handles are closed, which is why we drop ours and poll afterwards.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-deleteservice
    service.delete()?;
    drop(service);

    let started = Instant::now();
    while service_exists(&service_manager)? && started.elapsed() < START_STOP_TIMEOUT {
        thread::sleep(Duration::from_millis(250));
    }
    if service_exists(&service_manager)? {
        bail!("service is marked for deletion but Windows has not removed it yet");
    }
    let (executable, config, logs) = installation_paths()?;
    println!("Unregistered Windows service: {SERVICE_NAME}");
    println!("Retained executable: {}", executable.display());
    println!("Retained configuration: {}", config.display());
    println!("Retained logs: {}", logs.display());
    Ok(())
}

fn service_status(
    state: ServiceState,
    exit_code: ServiceExitCode,
    checkpoint: u32,
) -> ServiceStatus {
    let controls_accepted = if state == ServiceState::Running {
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
    } else {
        ServiceControlAccept::empty()
    };
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted,
        exit_code,
        checkpoint: if matches!(
            state,
            ServiceState::StartPending | ServiceState::StopPending
        ) {
            checkpoint
        } else {
            0
        },
        wait_hint: if matches!(
            state,
            ServiceState::StartPending | ServiceState::StopPending
        ) {
            Duration::from_secs(30)
        } else {
            Duration::ZERO
        },
        process_id: None,
    }
}

fn set_status(
    handle: ServiceStatusHandle,
    state: ServiceState,
    exit_code: ServiceExitCode,
    checkpoint: u32,
) -> Result<()> {
    // ServiceStatusHandle::set_service_status wraps SetServiceStatus. The SCM
    // uses these updates to gate controls and diagnose startup/shutdown delays.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-setservicestatus
    handle
        .set_service_status(service_status(state, exit_code, checkpoint))
        .with_context(|| format!("could not report service state {state:?}"))
}

fn set_status_best_effort(
    handle: ServiceStatusHandle,
    state: ServiceState,
    exit_code: ServiceExitCode,
    checkpoint: u32,
) {
    if let Err(status_error) = set_status(handle, state, exit_code, checkpoint) {
        error!("could not report service state {state:?}: {status_error}");
    }
}

fn init_service_logging(config_path: &Path) -> Result<()> {
    let data_dir = config_path
        .parent()
        .ok_or_else(|| anyhow!("service config path has no parent directory"))?;
    let log_dir = data_dir.join("logs");
    fs::create_dir_all(&log_dir)?;
    let writer = FileRotate::new(
        log_dir.join("modama-agent.log"),
        AppendCount::new(5),
        ContentLimit::BytesSurpassed(10 * 1024 * 1024),
        Compression::None,
        None,
    );
    let env = env_logger::Env::default()
        .filter_or("MODAMA_LOG_LEVEL", "info")
        .write_style("never");
    env_logger::Builder::from_env(env)
        .target(Target::Pipe(Box::new(writer)))
        .try_init()
        .map_err(Into::into)
}

pub fn run_service_dispatcher(config_path: PathBuf) -> Result<()> {
    SERVICE_CONFIG_PATH
        .set(config_path)
        .map_err(|_| anyhow!("service dispatcher was already initialized"))?;
    // This blocks in StartServiceCtrlDispatcherW until the service has stopped.
    // It must be called promptly by a process launched by the SCM.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-startservicectrldispatcherw
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
        .context("could not connect modama-agent to the Windows Service Control Manager")
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(service_error) = service_main_inner() {
        error!("Windows service failed: {service_error:#}");
    }
}

fn service_main_inner() -> Result<()> {
    let config_path = SERVICE_CONFIG_PATH
        .get()
        .ok_or_else(|| anyhow!("service config path was not initialized"))?
        .clone();

    let (shutdown_sender, mut shutdown_receiver) = tokio::sync::watch::channel(false);
    let shared_status: Arc<StdMutex<Option<ServiceStatusHandle>>> = Arc::new(StdMutex::new(None));
    let handler_status = Arc::clone(&shared_status);
    let event_handler = move |event| match event {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            if let Ok(status) = handler_status.lock()
                && let Some(handle) = *status
            {
                set_status_best_effort(
                    handle,
                    ServiceState::StopPending,
                    ServiceExitCode::NO_ERROR,
                    1,
                );
            }
            let _ = shutdown_sender.send(true);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    // register wraps RegisterServiceCtrlHandlerExW. Registration precedes slow
    // initialization so the process can immediately report START_PENDING.
    // https://learn.microsoft.com/windows/win32/api/winsvc/nf-winsvc-registerservicectrlhandlerexw
    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;
    *shared_status
        .lock()
        .map_err(|_| anyhow!("service status lock was poisoned"))? = Some(status_handle);
    set_status(
        status_handle,
        ServiceState::StartPending,
        ServiceExitCode::NO_ERROR,
        1,
    )?;

    let result = (|| -> Result<()> {
        init_service_logging(&config_path)?;
        set_status(
            status_handle,
            ServiceState::StartPending,
            ServiceExitCode::NO_ERROR,
            2,
        )?;
        let runtime = tokio::runtime::Runtime::new()?;
        set_status(
            status_handle,
            ServiceState::StartPending,
            ServiceExitCode::NO_ERROR,
            3,
        )?;
        runtime.block_on(run_agent(
            &config_path,
            async move {
                if !*shutdown_receiver.borrow() {
                    let _ = shutdown_receiver.changed().await;
                }
            },
            || {
                set_status(
                    status_handle,
                    ServiceState::Running,
                    ServiceExitCode::NO_ERROR,
                    0,
                )?;
                info!("Windows service entered the running state");
                Ok(())
            },
        ))
    })();

    let exit_code = if result.is_ok() {
        ServiceExitCode::NO_ERROR
    } else {
        ServiceExitCode::ServiceSpecific(1)
    };
    if let Err(runtime_error) = &result {
        error!("agent runtime stopped with an error: {runtime_error:#}");
    }
    // Once shutdown is already committed there is no useful local recovery
    // from a final SetServiceStatus failure; process termination still lets the
    // SCM observe that the service is gone.
    set_status_best_effort(status_handle, ServiceState::Stopped, exit_code, 0);
    result
}
