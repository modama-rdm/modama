//! Linux mount backend: SMB shares mounted in the user's session via GIO/GVfs.
//!
//! GIO's volume monitor only works on the thread that drives GLib's global-default main
//! context, so a dedicated actor thread owns that context, the volume monitor, and every GIO
//! object. A whole reconciliation pass runs on that thread; the rest of the helper only sends
//! the desired state and gets the resulting state back.
//!
//! On Linux, we only manage mounts that are recognizably our own: their root URI has our form
//! (`smb://<configured host>[:<configured port>]/<share>`). Other mounts, even to the same
//! server, don't get in the way of ours (unlike Windows connections), so they are left alone.

use std::{cell::Cell, path::PathBuf, rc::Rc, sync::mpsc, thread, time::Duration};

use gio::prelude::*;
use modama_common::{internal_api_types::SessionHelperConfig, messages::Share};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use tokio::sync::oneshot;
use url::Url;

use crate::{
    mount_backend::{EnumerationError, MountBackend, PassResult},
    reconcile::{
        AddFailure, DuplicateEntries, MountTable, PathlessEntries, Policy, TableEntry,
        reconcile_pass,
    },
};

/// Upper bound for a single mount or unmount operation
const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

const DEFAULT_SMB_PORT: u32 = 445;

/// A mount as listed by GIO; `handle` is how the API refers to it for unmounting
struct ListedMount<H> {
    root_uri: String,
    path: Option<PathBuf>,
    handle: H,
}

#[derive(Debug, PartialEq, Eq)]
enum MountError {
    /// Someone mounted it between our inventory and our mount attempt; not an error for us
    AlreadyMounted,
    AuthenticationFailed,
    TimedOut,
    HostNotFound,
    Other(String),
}

impl std::fmt::Display for MountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyMounted => write!(f, "already mounted"),
            Self::AuthenticationFailed => write!(f, "mount failed: authentication failed"),
            Self::TimedOut => write!(f, "mount failed: timed out"),
            Self::HostNotFound => write!(f, "mount failed: host not found"),
            Self::Other(message) => write!(f, "mount failed: {message}"),
        }
    }
}

/// The GIO operations a pass needs; only ever used on the actor thread
trait GioApi {
    type Handle;

    fn mounts(&self) -> Result<Vec<ListedMount<Self::Handle>>, String>;
    fn mount(&self, uri: &Url, share: &Share, domain: &str) -> Result<(), MountError>;
    fn unmount(&self, handle: &Self::Handle) -> Result<(), String>;
}

/// The share name, if `root_uri` is one of our mounts for `config`
fn our_share(root_uri: &str, config: &SessionHelperConfig) -> Option<String> {
    let url = Url::parse(root_uri).ok()?;
    if url.scheme() != "smb"
        || !url.host_str()?.eq_ignore_ascii_case(&config.smb_hostname)
        || url.port().map_or(DEFAULT_SMB_PORT, u32::from) != config.smb_port
    {
        return None;
    }
    // exactly one non-empty segment, possibly followed by an empty one (trailing slash)
    let mut segments = url.path_segments()?;
    let share = segments.next().filter(|s| !s.is_empty())?;
    if segments.any(|s| !s.is_empty()) {
        return None;
    }
    percent_decode_str(share)
        .decode_utf8()
        .ok()
        .map(|s| s.into_owned())
}

/// `smb://<host>[:<port>]/`: the base of the URIs we mount with, if the configuration allows
/// one; credentials never go into it
fn base_uri(config: &SessionHelperConfig) -> Result<Url, EnumerationError> {
    let host = &config.smb_hostname;
    if host.is_empty()
        || host
            .chars()
            .any(|c| c.is_whitespace() || "@/:?#[]\\%".contains(c))
    {
        return Err(EnumerationError(format!(
            "invalid SMB hostname in the configuration: {host:?}"
        )));
    }
    let mut url = Url::parse(&format!("smb://{host}/"))
        .map_err(|e| EnumerationError(format!("invalid SMB hostname {host:?}: {e}")))?;
    if config.smb_port != DEFAULT_SMB_PORT {
        let port = u16::try_from(config.smb_port)
            .map_err(|_| EnumerationError(format!("invalid SMB port {}", config.smb_port)))?;
        url.set_port(Some(port))
            .map_err(|()| EnumerationError(format!("invalid SMB port {port}")))?;
    }
    Ok(url)
}

/// The URI we mount `share_name` with: any share name, percent-encoded
fn share_uri(base: &Url, share_name: &str) -> Url {
    let mut url = base.clone();
    url.set_path(&utf8_percent_encode(share_name, NON_ALPHANUMERIC).to_string());
    url
}

/// GIO messages may contain the URI; error messages must not (and never the password)
fn redact(message: &str, uri: &Url, share: &Share) -> String {
    let mut message = message.replace(uri.as_str(), &share.share_name);
    if let Ok(decoded) = percent_decode_str(uri.as_str()).decode_utf8() {
        message = message.replace(decoded.as_ref(), &share.share_name);
    }
    if !share.password.is_empty() {
        message = message.replace(&share.password, "[REDACTED]");
    }
    message
}

/// Our mounts, as a [`MountTable`] for one pass
struct GioTable<'a, A: GioApi> {
    api: &'a A,
    config: &'a SessionHelperConfig,
    base: Url,
}

impl<'a, A: GioApi> GioTable<'a, A> {
    fn new(api: &'a A, config: &'a SessionHelperConfig) -> Result<Self, EnumerationError> {
        Ok(Self {
            api,
            config,
            base: base_uri(config)?,
        })
    }
}

impl<A: GioApi> MountTable for GioTable<'_, A> {
    type Handle = A::Handle;

    /// Remounting a mount without a local path would give one without a path again; it is
    /// kept, and the error points at the likely cause
    fn policy(&self) -> Policy {
        Policy {
            pathless: PathlessEntries::KeepWithError(
                "mounted, but without a local path (is gvfs-fuse installed?)",
            ),
            duplicates: DuplicateEntries::Keep,
        }
    }

    fn entries(&mut self) -> Result<Vec<TableEntry<A::Handle>>, String> {
        Ok(self
            .api
            .mounts()?
            .into_iter()
            .filter_map(|m| {
                Some(TableEntry {
                    share_name: our_share(&m.root_uri, self.config)?,
                    path: m.path,
                    handle: m.handle,
                })
            })
            .collect())
    }

    /// GVfs keeps credentials per mount, so an old mount never blocks a new one on Linux;
    /// nothing to force
    fn remove(&mut self, entry: &TableEntry<A::Handle>, _force: bool) -> Result<(), String> {
        self.api.unmount(&entry.handle)
    }

    fn add(&mut self, share: &Share) -> Result<(), AddFailure> {
        let uri = share_uri(&self.base, &share.share_name);
        match self.api.mount(&uri, share, &self.config.smb_domain) {
            Ok(()) => Ok(()),
            Err(MountError::AlreadyMounted) => Err(AddFailure::AlreadyMounted),
            Err(e) => Err(AddFailure::Failed(redact(&e.to_string(), &uri, share))),
        }
    }
}

/// One reconciliation pass over our mounts: remove those that aren't desired, mount the missing
/// ones, and return the actual state afterwards
fn reconcile<A: GioApi>(
    api: &A,
    shares: &[Share],
    config: &SessionHelperConfig,
) -> Result<PassResult, EnumerationError> {
    // forcing makes no difference here (see `GioTable::remove`)
    reconcile_pass(&mut GioTable::new(api, config)?, shares, false)
}

/// GIO itself; must only be created and used on the thread that owns the default main context
struct NativeGio {
    context: glib::MainContext,
    /// held for the actor's lifetime, so it keeps tracking mounts between passes
    monitor: gio::VolumeMonitor,
}

impl NativeGio {
    /// Process what's pending, e.g. volume monitor updates
    fn drain(&self) {
        while self.context.pending() {
            self.context.iteration(false);
        }
    }

    fn run<T>(
        &self,
        future: impl Future<Output = Result<T, glib::Error>>,
    ) -> Result<Result<T, glib::Error>, glib::FutureWithTimeoutError> {
        self.context
            .block_on(glib::future_with_timeout(OPERATION_TIMEOUT, future))
    }
}

impl GioApi for NativeGio {
    type Handle = gio::Mount;

    fn mounts(&self) -> Result<Vec<ListedMount<gio::Mount>>, String> {
        self.drain();
        Ok(self
            .monitor
            .mounts()
            .into_iter()
            .map(|mount| {
                let root = mount.root();
                ListedMount {
                    root_uri: root.uri().into(),
                    path: root.path(),
                    handle: mount,
                }
            })
            .collect())
    }

    fn mount(&self, uri: &Url, share: &Share, domain: &str) -> Result<(), MountError> {
        let file = gio::File::for_uri(uri.as_str());
        let operation = gio::MountOperation::new();
        operation.set_password_save(gio::PasswordSave::Never);

        // answer once; asking again means the credentials were rejected, and sending them again
        // could lock the account
        let prompts = Rc::new(Cell::new(0u32));
        let (username, password, domain) = (
            share.username.clone(),
            share.password.clone(),
            domain.to_owned(),
        );
        let counter = Rc::clone(&prompts);
        operation.connect_ask_password(move |operation, _message, _user, _domain, _flags| {
            counter.set(counter.get() + 1);
            if counter.get() > 1 {
                operation.reply(gio::MountOperationResult::Aborted);
                return;
            }
            operation.set_username(Some(&username));
            operation.set_domain(Some(&domain));
            operation.set_password(Some(&password));
            operation.set_password_save(gio::PasswordSave::Never);
            operation.reply(gio::MountOperationResult::Handled);
        });
        // nobody is there to answer questions; without a reply, the mount would hang
        operation.connect_local("ask-question", false, |values| {
            if let Some(Ok(operation)) = values.first().map(|v| v.get::<gio::MountOperation>()) {
                operation.reply(gio::MountOperationResult::Aborted);
            }
            None
        });

        match self
            .run(file.mount_enclosing_volume_future(gio::MountMountFlags::NONE, Some(&operation)))
        {
            Ok(Ok(())) => Ok(()),
            Err(_timeout) => Err(MountError::TimedOut),
            Ok(Err(e)) => Err(match e.kind::<gio::IOErrorEnum>() {
                Some(gio::IOErrorEnum::AlreadyMounted) => MountError::AlreadyMounted,
                _ if prompts.get() > 1 => MountError::AuthenticationFailed,
                Some(gio::IOErrorEnum::PermissionDenied | gio::IOErrorEnum::FailedHandled) => {
                    MountError::AuthenticationFailed
                }
                Some(gio::IOErrorEnum::TimedOut) => MountError::TimedOut,
                Some(gio::IOErrorEnum::HostNotFound) => MountError::HostNotFound,
                _ => MountError::Other(e.message().to_owned()),
            }),
        }
    }

    fn unmount(&self, handle: &gio::Mount) -> Result<(), String> {
        match self.run(handle.unmount_with_operation_future(
            gio::MountUnmountFlags::NONE,
            None::<&gio::MountOperation>,
        )) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e.message().to_owned()),
            Err(_timeout) => Err("timed out".to_owned()),
        }
    }
}

type ReconcileResult = Result<PassResult, EnumerationError>;

enum Command {
    Reconcile {
        shares: Vec<Share>,
        config: SessionHelperConfig,
        reply: oneshot::Sender<ReconcileResult>,
    },
    Shutdown,
}

fn gio_actor(commands: mpsc::Receiver<Command>, ready: mpsc::SyncSender<Result<(), String>>) {
    let context = glib::MainContext::default();
    let _guard = match context.acquire() {
        Ok(guard) => guard,
        Err(e) => {
            let _ = ready.send(Err(format!("could not acquire the GLib main context: {e}")));
            return;
        }
    };
    let gio = NativeGio {
        context: context.clone(),
        monitor: gio::VolumeMonitor::get(),
    };
    if ready.send(Ok(())).is_err() {
        return;
    }

    loop {
        match commands.try_recv() {
            Ok(Command::Reconcile {
                shares,
                config,
                reply,
            }) => {
                // the caller may have given up; nothing to do about that
                let _ = reply.send(reconcile(&gio, &shares, &config));
            }
            Ok(Command::Shutdown) => break,
            // a sender wakes the context after sending, so this doesn't sleep through commands
            Err(mpsc::TryRecvError::Empty) => {
                context.iteration(true);
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
        }
    }
}

/// Mounts via GIO/GVfs on a dedicated actor thread
pub struct GioBackend {
    commands: mpsc::Sender<Command>,
    context: glib::MainContext,
}

impl GioBackend {
    pub async fn new() -> Result<Self, EnumerationError> {
        let (commands, receiver) = mpsc::channel();
        let (ready, ready_receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("gio-actor".to_owned())
            .spawn(move || gio_actor(receiver, ready))
            .map_err(|e| EnumerationError(format!("could not start the GIO thread: {e}")))?;
        match ready_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                commands,
                context: glib::MainContext::default(),
            }),
            Ok(Err(e)) => Err(EnumerationError(e)),
            Err(_) => Err(EnumerationError("the GIO thread stopped".to_owned())),
        }
    }
}

impl Drop for GioBackend {
    fn drop(&mut self) {
        // the actor sleeps in the main loop, so it must be woken to notice; this releases the
        // default main context for whoever needs it next
        let _ = self.commands.send(Command::Shutdown);
        self.context.wakeup();
    }
}

impl MountBackend for GioBackend {
    async fn reconcile(
        &self,
        desired_shares: &[Share],
        config: &SessionHelperConfig,
        // nothing to force (see `GioTable::remove`)
        _force: bool,
    ) -> ReconcileResult {
        let stopped = || EnumerationError("the GIO thread stopped".to_owned());
        let (reply, result) = oneshot::channel();
        self.commands
            .send(Command::Reconcile {
                shares: desired_shares.to_vec(),
                config: config.clone(),
                reply,
            })
            .map_err(|_| stopped())?;
        self.context.wakeup();
        result.await.map_err(|_| stopped())?
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap};

    use modama_common::internal_api_types::SessionHelperState;

    use super::*;
    use crate::reconcile::tests::share;

    fn config() -> SessionHelperConfig {
        SessionHelperConfig {
            smb_hostname: "smb.example.test".to_owned(),
            smb_port: 445,
            smb_domain: "WORKGROUP".to_owned(),
        }
    }

    #[test]
    fn our_share_recognizes_our_uri_form() {
        let mut config = config();
        assert_eq!(
            our_share("smb://smb.example.test/ingest/", &config).as_deref(),
            Some("ingest")
        );
        assert_eq!(
            our_share("smb://SMB.example.test/a%20b", &config).as_deref(),
            Some("a b")
        );
        assert_eq!(
            our_share("smb://smb.example.test:445/ingest", &config).as_deref(),
            Some("ingest")
        );
        config.smb_port = 1445;
        assert_eq!(
            our_share("smb://smb.example.test:1445/ingest/", &config).as_deref(),
            Some("ingest")
        );
    }

    #[test]
    fn our_share_ignores_everything_else() {
        let config = config();
        for uri in [
            "smb://other.example.test/ingest/",
            "smb://smb.example.test:1445/ingest/",
            "smb://smb.example.test/",
            "smb://smb.example.test/ingest/subdir/",
            "sftp://smb.example.test/ingest/",
            "file:///run/user/1000/gvfs/",
            "gphoto2://[usb:001,005]/",
            "not a uri",
        ] {
            assert_eq!(our_share(uri, &config), None, "{uri}");
        }
    }

    #[test]
    fn share_uri_round_trips_through_our_share() {
        let mut config = config();
        let base = base_uri(&config).unwrap();
        for name in ["ingest", "a b", "ünï%cöde", r"b\..", "c/d"] {
            let uri = share_uri(&base, name);
            assert_eq!(our_share(uri.as_str(), &config).as_deref(), Some(name));
        }
        assert_eq!(
            share_uri(&base, "ingest").as_str(),
            "smb://smb.example.test/ingest"
        );
        config.smb_port = 1445;
        assert_eq!(
            share_uri(&base_uri(&config).unwrap(), "ingest").as_str(),
            "smb://smb.example.test:1445/ingest"
        );
    }

    #[test]
    fn base_uri_rejects_hosts_that_would_change_the_uri() {
        for host in ["", "user@host", "host/path", "host:1", "a b", "host?x"] {
            let config = SessionHelperConfig {
                smb_hostname: host.to_owned(),
                ..config()
            };
            assert!(base_uri(&config).is_err(), "{host:?}");
        }
    }

    /// Mounts by URI; `mount` and `unmount` results can be scripted per share name
    #[derive(Default)]
    struct FakeGio {
        mounts: RefCell<Vec<(String, Option<PathBuf>)>>,
        mount_errors: HashMap<String, MountError>,
        /// mount succeeds, but without a local path (no gvfs-fuse)
        no_path: bool,
        calls: RefCell<Vec<String>>,
    }

    impl FakeGio {
        fn with_mounts(uris: &[&str]) -> Self {
            Self {
                mounts: RefCell::new(
                    uris.iter()
                        .map(|u| ((*u).to_owned(), Some(PathBuf::from("/fuse"))))
                        .collect(),
                ),
                ..Self::default()
            }
        }
    }

    impl GioApi for FakeGio {
        type Handle = String;

        fn mounts(&self) -> Result<Vec<ListedMount<String>>, String> {
            Ok(self
                .mounts
                .borrow()
                .iter()
                .map(|(uri, path)| ListedMount {
                    root_uri: uri.clone(),
                    path: path.clone(),
                    handle: uri.clone(),
                })
                .collect())
        }

        fn mount(&self, uri: &Url, share: &Share, _domain: &str) -> Result<(), MountError> {
            self.calls.borrow_mut().push(format!("mount {uri}"));
            match self.mount_errors.get(&share.share_name) {
                Some(MountError::AlreadyMounted) => return Err(MountError::AlreadyMounted),
                Some(MountError::Other(m)) => return Err(MountError::Other(m.clone())),
                Some(MountError::AuthenticationFailed) => {
                    return Err(MountError::AuthenticationFailed);
                }
                Some(_) => return Err(MountError::TimedOut),
                None => {}
            }
            let path =
                (!self.no_path).then(|| PathBuf::from(format!("/fuse/{}", share.share_name)));
            self.mounts.borrow_mut().push((format!("{uri}/"), path));
            Ok(())
        }

        fn unmount(&self, handle: &String) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("unmount {handle}"));
            self.mounts.borrow_mut().retain(|(uri, _)| uri != handle);
            Ok(())
        }
    }

    fn pass(api: &FakeGio, shares: &[Share]) -> SessionHelperState {
        reconcile(api, shares, &config()).unwrap().state
    }

    fn names(state: &SessionHelperState) -> Vec<&str> {
        let mut names: Vec<_> = state
            .mounted_shares
            .iter()
            .map(|m| m.share_name.as_str())
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn mounts_missing_removes_undesired_and_leaves_others_alone() {
        let api = FakeGio::with_mounts(&[
            "smb://smb.example.test/keep/",
            "smb://smb.example.test/stale/",
            "smb://other.example.test/stale/",
            "smb://smb.example.test:1445/stale/",
            "smb://smb.example.test/",
            "gphoto2://[usb:001,005]/",
        ]);
        let state = pass(&api, &[share("1", "keep"), share("2", "new")]);

        assert_eq!(names(&state), vec!["keep", "new"]);
        assert!(state.errors.is_empty());
        assert_eq!(
            *api.calls.borrow(),
            vec![
                "unmount smb://smb.example.test/stale/",
                "mount smb://smb.example.test/new",
            ]
        );
        // other hosts, ports and unrelated mounts are still there:
        assert_eq!(api.mounts.borrow().len(), 6);
    }

    #[test]
    fn invalid_hostname_fails_the_pass_before_anything_changes() {
        let api = FakeGio::with_mounts(&["smb://smb.example.test/stale/"]);
        let config = SessionHelperConfig {
            smb_hostname: "smb.example.test/x".to_owned(),
            ..config()
        };
        assert!(reconcile(&api, &[share("1", "a")], &config).is_err());
        assert!(api.calls.borrow().is_empty());
    }

    #[test]
    fn already_mounted_is_not_an_error() {
        let mut api = FakeGio::default();
        api.mount_errors
            .insert("a".to_owned(), MountError::AlreadyMounted);
        // the fake doesn't record the mount then; the next inventory decides:
        let result = reconcile(&api, &[share("1", "a")], &config()).unwrap();
        assert_eq!(result.state.errors[0].message, "not mounted");
        assert!(result.created.is_empty());

        api.mounts.borrow_mut().push((
            "smb://smb.example.test/a/".to_owned(),
            Some("/fuse/a".into()),
        ));
        assert!(pass(&api, &[share("1", "a")]).errors.is_empty());
    }

    #[test]
    fn mount_errors_are_reported_without_uri_or_password() {
        let mut api = FakeGio::default();
        api.mount_errors.insert(
            "a b".to_owned(),
            MountError::Other(
                "smb://smb.example.test/a%20b: no access for s3cret-password at smb://smb.example.test/a b"
                    .to_owned(),
            ),
        );
        api.mount_errors
            .insert("c".to_owned(), MountError::AuthenticationFailed);
        let state = pass(&api, &[share("1", "a b"), share("2", "c")]);

        assert!(state.mounted_shares.is_empty());
        let message = &state.errors[0].message;
        assert_eq!(state.errors[0].share_id, "1");
        assert!(!message.contains("smb://"), "{message}");
        assert!(!message.contains("s3cret-password"), "{message}");
        assert_eq!(
            state.errors[1].message,
            "mount failed: authentication failed"
        );
    }

    #[test]
    fn mount_without_local_path_is_kept_and_explains_itself() {
        let api = FakeGio {
            no_path: true,
            ..FakeGio::default()
        };
        let state = pass(&api, &[share("1", "a")]);
        assert_eq!(state.mounted_shares[0].mount_path, None);
        assert!(state.errors[0].message.contains("gvfs-fuse"));

        // the next pass leaves it alone
        let state = pass(&api, &[share("1", "a")]);
        assert_eq!(*api.calls.borrow(), vec!["mount smb://smb.example.test/a"]);
        assert!(state.errors[0].message.contains("gvfs-fuse"));
    }
}
