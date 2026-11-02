/// Info by default, but zbus and its tracing spans only from warn on: they log every D-Bus
/// handshake at info. `MODAMA_LOG_LEVEL` is applied on top, so it can still turn them up
/// (`debug,zbus=debug`).
fn builder(env: env_logger::Env) -> env_logger::Builder {
    let mut builder = env_logger::Builder::new();
    builder
        .filter_level(log::LevelFilter::Info)
        .filter_module("zbus", log::LevelFilter::Warn)
        .filter_module("tracing", log::LevelFilter::Warn)
        .parse_env(env);
    builder
}

pub fn init_logging() {
    let env = env_logger::Env::default()
        .filter("MODAMA_LOG_LEVEL")
        .write_style("always");
    if let Err(e) = builder(env).try_init() {
        eprintln!("logging already initialized or initialization failed: {e}")
    }
}

/// Log into rotating files `log_dir/file_name`, for processes without a console
#[cfg(windows)]
pub fn init_file_logging(log_dir: &std::path::Path, file_name: &str) -> anyhow::Result<()> {
    use file_rotate::{ContentLimit, FileRotate, compression::Compression, suffix::AppendCount};

    std::fs::create_dir_all(log_dir)?;
    let writer = FileRotate::new(
        log_dir.join(file_name),
        AppendCount::new(5),
        ContentLimit::BytesSurpassed(10 * 1024 * 1024),
        Compression::None,
        None,
    );
    let env = env_logger::Env::default()
        .filter("MODAMA_LOG_LEVEL")
        .write_style("never");
    builder(env)
        .target(env_logger::Target::Pipe(Box::new(writer)))
        .try_init()
        .map_err(Into::into)
}

/// The per-user log directory on Windows, shared by the processes of the user's sessions (GUI,
/// session helper, mount popup): `%LOCALAPPDATA%\modama\modama-agent\logs`
#[cfg(windows)]
pub fn user_log_dir() -> anyhow::Result<std::path::PathBuf> {
    use windows::Win32::UI::Shell::CSIDL_LOCAL_APPDATA;

    Ok(crate::windows_folders::known_folder(CSIDL_LOCAL_APPDATA)?
        .join("modama")
        .join("modama-agent")
        .join("logs"))
}

/// Logging for a process of the user's session. On Windows these usually run without a console,
/// so they log into `file_name` in [`user_log_dir`] and also log panics there (in release builds
/// a panic aborts, and the default hook only writes to the missing stderr). Elsewhere they log to
/// stderr. Returns the log file's path, if there is one.
pub fn init_user_session_logging(file_name: &str) -> anyhow::Result<Option<std::path::PathBuf>> {
    #[cfg(windows)]
    {
        let log_dir = user_log_dir()?;
        init_file_logging(&log_dir, file_name)?;
        install_panic_logging();
        Ok(Some(log_dir.join(file_name)))
    }
    #[cfg(not(windows))]
    {
        let _ = file_name;
        init_logging();
        Ok(None)
    }
}

/// Log panics (message and location) before the previous hook runs
#[cfg(windows)]
fn install_panic_logging() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("{info}");
        log::logger().flush();
        previous(info);
    }));
}
