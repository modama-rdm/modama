//! The mount popup: whenever a pass created mounts, the helper shows them in a small window (the
//! GUI executable in popup mode) until the user opens them or dismisses the popup. There is at
//! most one popup; mounts created while it is open are added to it, and mounts of shares that
//! are no longer desired leave it. Once none is left, the popup closes.

use std::{
    collections::HashSet,
    future::Future,
    io,
    path::{Path, PathBuf},
    process::Stdio,
};

use log::{info, warn};
use modama_common::mount_popup::{PopupChoice, PopupContent, PopupMount};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::mpsc,
};

/// One open popup
pub trait PopupWindow {
    /// Show `content`, replacing what the popup showed before
    fn update(&mut self, content: &PopupContent) -> impl Future<Output = io::Result<()>>;

    /// The user's answer; None if the popup ended without one (closed window, no display).
    /// Must be cancel-safe: it is raced against newly created mounts.
    fn choice(&mut self) -> impl Future<Output = Option<PopupChoice>>;
}

/// Opens popups
pub trait PopupWindows {
    type Window: PopupWindow;

    fn spawn(&self) -> io::Result<Self::Window>;
}

/// Shows a folder in the file manager; failures are logged, not returned: there is nobody to
/// tell, and the mount stays usable
pub trait FolderOpener {
    fn open(&self, path: &Path) -> impl Future<Output = ()>;
}

/// A mount a pass created, with the share it belongs to
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedMount {
    pub share_id: String,
    pub mount: PopupMount,
}

/// What the main loop tells the popup task
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupEvent {
    /// A pass created these mounts
    Created(Vec<CreatedMount>),
    /// The IDs of the shares that should be mounted now; the popup drops the others
    Desired(Vec<String>),
}

async fn next_choice<W: PopupWindow>(window: &mut Option<W>) -> Option<PopupChoice> {
    match window {
        Some(window) => window.choice().await,
        None => std::future::pending().await,
    }
}

/// Show the mounts arriving on `events` until the helper stops
pub async fn run_popup_task(
    mut events: mpsc::UnboundedReceiver<PopupEvent>,
    windows: impl PopupWindows,
    opener: impl FolderOpener,
) {
    let mut shown: Vec<CreatedMount> = Vec::new();
    let mut window = None;
    loop {
        tokio::select! {
            // an answer that is already there is handled before new mounts extend the popup
            biased;
            choice = next_choice(&mut window) => {
                if choice == Some(PopupChoice::Open) {
                    for created in &shown {
                        opener.open(&created.mount.mount_path).await;
                    }
                }
                window = None;
                shown.clear();
            }
            event = events.recv() => {
                let Some(event) = event else {
                    return;
                };
                let before = shown.len();
                match event {
                    PopupEvent::Created(mounts) => {
                        for created in mounts {
                            if !shown.iter().any(|s| s.mount.mount_path == created.mount.mount_path) {
                                shown.push(created);
                            }
                        }
                    }
                    PopupEvent::Desired(share_ids) => {
                        let desired: HashSet<String> = share_ids.into_iter().collect();
                        shown.retain(|created| desired.contains(&created.share_id));
                    }
                }
                if shown.is_empty() {
                    // dropping the window ends the popup process
                    if window.take().is_some() {
                        info!("closing the mount popup: its shares are no longer desired");
                    }
                    continue;
                }
                if window.is_some() && shown.len() == before {
                    continue;
                }
                if window.is_none() {
                    match windows.spawn() {
                        Ok(spawned) => window = Some(spawned),
                        Err(e) => {
                            warn!("could not show the mount popup: {e}");
                            shown.clear();
                            continue;
                        }
                    }
                }
                if let Some(open) = &mut window {
                    let content = PopupContent {
                        mounts: shown.iter().map(|created| created.mount.clone()).collect(),
                    };
                    if let Err(e) = open.update(&content).await {
                        warn!("could not update the mount popup: {e}");
                        window = None;
                        shown.clear();
                    }
                }
            }
        }
    }
}

/// Our own executable (the GUI's, which the helper runs from), started as `mount-popup`
pub struct GuiPopups {
    gui: PathBuf,
}

impl GuiPopups {
    pub fn from_current_exe() -> io::Result<Self> {
        Ok(Self {
            gui: std::env::current_exe()?,
        })
    }
}

pub struct GuiPopup {
    /// killed when dropped, so a popup never outlives its helper or its answer
    _child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
}

impl PopupWindows for GuiPopups {
    type Window = GuiPopup;

    fn spawn(&self) -> io::Result<GuiPopup> {
        let mut child = Command::new(&self.gui)
            .arg("mount-popup")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        info!("showing the mount popup (pid {:?})", child.id());
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("the popup has no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("the popup has no stdout"))?;
        Ok(GuiPopup {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
        })
    }
}

impl PopupWindow for GuiPopup {
    async fn update(&mut self, content: &PopupContent) -> io::Result<()> {
        let mut line = serde_json::to_string(content)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await
    }

    async fn choice(&mut self) -> Option<PopupChoice> {
        loop {
            // `next_line` is cancel-safe
            match self.stdout.next_line().await {
                Ok(Some(line)) => match serde_json::from_str(&line) {
                    Ok(choice) => return Some(choice),
                    Err(e) => warn!("ignoring unexpected output of the mount popup: {e}"),
                },
                Ok(None) => return None,
                Err(e) => {
                    warn!("could not read from the mount popup: {e}");
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use std::time::Duration;

    use tokio::{sync::oneshot, time::timeout};

    use super::*;

    /// What happened, in order: "spawn", "update a,b", "open a"
    type Log = Arc<Mutex<Vec<String>>>;

    /// Each spawned window answers with what the test sends on its oneshot
    struct FakeWindows {
        log: Log,
        answers: Arc<Mutex<Vec<oneshot::Receiver<Option<PopupChoice>>>>>,
        /// Windows spawned and not dropped yet
        open: Arc<AtomicUsize>,
    }

    struct FakeWindow {
        log: Log,
        answer: Option<oneshot::Receiver<Option<PopupChoice>>>,
        open: Arc<AtomicUsize>,
    }

    impl Drop for FakeWindow {
        fn drop(&mut self) {
            self.open.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl PopupWindows for FakeWindows {
        type Window = FakeWindow;

        fn spawn(&self) -> io::Result<FakeWindow> {
            self.log.lock().unwrap().push("spawn".to_owned());
            self.open.fetch_add(1, Ordering::SeqCst);
            Ok(FakeWindow {
                log: Arc::clone(&self.log),
                answer: Some(self.answers.lock().unwrap().remove(0)),
                open: Arc::clone(&self.open),
            })
        }
    }

    impl PopupWindow for FakeWindow {
        async fn update(&mut self, content: &PopupContent) -> io::Result<()> {
            let sessions: Vec<_> = content.mounts.iter().map(|m| m.session.as_str()).collect();
            self.log
                .lock()
                .unwrap()
                .push(format!("update {}", sessions.join(",")));
            Ok(())
        }

        async fn choice(&mut self) -> Option<PopupChoice> {
            match &mut self.answer {
                Some(answer) => {
                    let choice = answer.await.ok().flatten();
                    self.answer = None;
                    choice
                }
                None => std::future::pending().await,
            }
        }
    }

    struct FakeOpener(Log);

    impl FolderOpener for FakeOpener {
        async fn open(&self, path: &Path) {
            self.0
                .lock()
                .unwrap()
                .push(format!("open {}", path.display()));
        }
    }

    /// The mount of share `session`, at path `session`
    fn mount(session: &str) -> CreatedMount {
        CreatedMount {
            share_id: session.to_owned(),
            mount: PopupMount {
                project: "project".to_owned(),
                session: session.to_owned(),
                mount_path: PathBuf::from(session),
            },
        }
    }

    fn desired(share_ids: &[&str]) -> PopupEvent {
        PopupEvent::Desired(share_ids.iter().map(|id| (*id).to_owned()).collect())
    }

    struct Setup {
        log: Log,
        events: mpsc::UnboundedSender<PopupEvent>,
        answers: Vec<oneshot::Sender<Option<PopupChoice>>>,
        open: Arc<AtomicUsize>,
    }

    impl Setup {
        /// The task, with `windows` popups available
        fn start(windows: usize) -> Self {
            let log = Log::default();
            let open = Arc::new(AtomicUsize::new(0));
            let (answers, receivers): (Vec<_>, Vec<_>) =
                (0..windows).map(|_| oneshot::channel()).unzip();
            let (events, rx) = mpsc::unbounded_channel();
            tokio::spawn(run_popup_task(
                rx,
                FakeWindows {
                    log: Arc::clone(&log),
                    answers: Arc::new(Mutex::new(receivers)),
                    open: Arc::clone(&open),
                },
                FakeOpener(Arc::clone(&log)),
            ));
            Self {
                log,
                events,
                answers,
                open,
            }
        }

        fn created(&self, mounts: &[&str]) {
            let mounts = mounts.iter().map(|session| mount(session)).collect();
            self.events.send(PopupEvent::Created(mounts)).unwrap();
        }

        async fn wait_for_open_windows(&self, expected: usize) {
            timeout(Duration::from_secs(2), async {
                while self.open.load(Ordering::SeqCst) != expected {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("{} windows open", self.open.load(Ordering::SeqCst)));
        }

        async fn wait_for_log(&self, expected: &[&str]) {
            timeout(Duration::from_secs(2), async {
                while *self.log.lock().unwrap() != expected {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("log is {:?}", self.log.lock().unwrap()));
        }
    }

    #[tokio::test]
    async fn new_mounts_extend_the_open_popup_and_open_opens_them_all() {
        let mut setup = Setup::start(1);
        setup.created(&["a"]);
        setup.wait_for_log(&["spawn", "update a"]).await;

        // a duplicate is not shown twice:
        setup.created(&["b", "a"]);
        setup
            .wait_for_log(&["spawn", "update a", "update a,b"])
            .await;

        setup
            .answers
            .remove(0)
            .send(Some(PopupChoice::Open))
            .unwrap();
        setup
            .wait_for_log(&["spawn", "update a", "update a,b", "open a", "open b"])
            .await;
    }

    #[tokio::test]
    async fn dismissed_or_closed_popups_open_nothing_and_the_next_mount_shows_a_new_one() {
        let mut setup = Setup::start(3);
        setup.created(&["a"]);
        setup.wait_for_log(&["spawn", "update a"]).await;
        setup
            .answers
            .remove(0)
            .send(Some(PopupChoice::Dismiss))
            .unwrap();

        setup.created(&["b"]);
        // the dismissed "a" is not shown again:
        setup
            .wait_for_log(&["spawn", "update a", "spawn", "update b"])
            .await;
        // the window closed without an answer:
        setup.answers.remove(0).send(None).unwrap();

        setup.created(&["c"]);
        setup
            .wait_for_log(&[
                "spawn", "update a", "spawn", "update b", "spawn", "update c",
            ])
            .await;
    }

    #[tokio::test]
    async fn mounts_no_longer_desired_leave_the_popup_and_the_last_one_closes_it() {
        let setup = Setup::start(1);
        setup.created(&["a", "b"]);
        setup.wait_for_log(&["spawn", "update a,b"]).await;

        // "b" is still desired, "c" is new but not mounted yet: only "a" goes
        setup.events.send(desired(&["b", "c"])).unwrap();
        setup
            .wait_for_log(&["spawn", "update a,b", "update b"])
            .await;
        // the same desired state again changes nothing
        setup.events.send(desired(&["b", "c"])).unwrap();

        setup.events.send(desired(&["c"])).unwrap();
        setup.wait_for_open_windows(0).await;
        assert_eq!(
            *setup.log.lock().unwrap(),
            ["spawn", "update a,b", "update b"]
        );
    }

    #[tokio::test]
    async fn after_closing_for_lack_of_desired_mounts_the_next_mount_shows_a_new_popup() {
        let setup = Setup::start(2);
        setup.created(&["a"]);
        setup.wait_for_log(&["spawn", "update a"]).await;
        setup.events.send(desired(&[])).unwrap();
        setup.wait_for_open_windows(0).await;

        setup.created(&["b"]);
        setup
            .wait_for_log(&["spawn", "update a", "spawn", "update b"])
            .await;
        setup.wait_for_open_windows(1).await;
    }

    #[tokio::test]
    async fn a_desired_state_without_an_open_popup_opens_none() {
        let setup = Setup::start(1);
        setup.events.send(desired(&["a"])).unwrap();
        setup.created(&["a"]);
        setup.wait_for_log(&["spawn", "update a"]).await;
        setup.events.send(desired(&["a"])).unwrap();
        setup.wait_for_open_windows(1).await;
        assert_eq!(*setup.log.lock().unwrap(), ["spawn", "update a"]);
    }
}
