//! One GUI per user and display: starting the GUI again brings the running window to the
//! front instead of opening a second one.
//!
//! Only on X11. Wayland doesn't let a program raise its own window, so there a second start
//! still opens a second window. Windows isn't covered yet.
//!
//! The first GUI holds a lock (like the session helper's: the kernel releases it when the
//! process exits, so it can't go stale) and listens on a socket next to it; a later start
//! finds the lock taken, connects and exits. Anything that goes wrong on the way, other than
//! finding a running GUI, lets the GUI start as it would without this.

use eframe::egui;

/// What the GUI does after looking for a running one
pub enum Claim {
    /// Run the GUI; with an instance, it raises its window when started again
    Run(Option<Instance>),
    /// A running GUI was asked to raise its window; this one exits
    Raised,
}

/// The lock of the running GUI, and the socket a later start connects to
pub struct Instance {
    #[cfg(unix)]
    guard: modama_session_helper::singleton::SingletonGuard,
    #[cfg(unix)]
    listener: Option<std::os::unix::net::UnixListener>,
}

impl Instance {
    /// Raise the window whenever the GUI is started again
    pub fn listen(self, ctx: egui::Context) {
        #[cfg(unix)]
        if let Some(listener) = self.listener {
            let guard = self.guard;
            std::thread::spawn(move || {
                // the lock lives as long as the listener
                let _guard = guard;
                for stream in listener.incoming() {
                    match stream {
                        Ok(_) => raise(&ctx),
                        Err(e) => log::warn!("a second start of the GUI couldn't connect: {e}"),
                    }
                }
            });
        }
        #[cfg(not(unix))]
        let _ = ctx;
    }
}

#[cfg(unix)]
fn raise(ctx: &egui::Context) {
    log::info!("the GUI was started again: bringing its window to the front");
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
        egui::UserAttentionType::Informational,
    ));
    ctx.request_repaint();
}

/// Look for a running GUI on this display, from the environment
pub fn claim() -> Claim {
    #[cfg(unix)]
    {
        let non_empty = |name| std::env::var_os(name).filter(|value| !value.is_empty());
        if non_empty("WAYLAND_DISPLAY").is_some() {
            return Claim::Run(None);
        }
        let (Some(display), Some(runtime_dir)) =
            (non_empty("DISPLAY"), non_empty("XDG_RUNTIME_DIR"))
        else {
            return Claim::Run(None);
        };
        unix::claim_in(
            &std::path::PathBuf::from(runtime_dir).join("modama"),
            &display.to_string_lossy(),
        )
    }
    #[cfg(not(unix))]
    Claim::Run(None)
}

#[cfg(unix)]
mod unix {
    use std::{
        io::Write as _,
        os::unix::net::{UnixListener, UnixStream},
        path::Path,
        time::Duration,
    };

    use modama_session_helper::singleton::{Singleton, acquire_at};

    use super::{Claim, Instance};

    /// The display in a file name: `:0` becomes `_0`, `localhost:10` `localhost_10`. The
    /// screen number goes, so `:0.0` is `:0` too: desktops set either.
    fn file_name_part(display: &str) -> String {
        let display = match display.rsplit_once(':') {
            Some((host, number)) => {
                let number = number
                    .split_once('.')
                    .map_or(number, |(number, _screen)| number);
                format!("{host}:{number}")
            }
            None => display.to_owned(),
        };
        display
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    pub(super) fn claim_in(dir: &Path, display: &str) -> Claim {
        let name = format!("gui-{}", file_name_part(display));
        let lock_path = dir.join(format!("{name}.lock"));
        let socket_path = dir.join(format!("{name}.sock"));
        match acquire_at(&lock_path) {
            Ok(Singleton::Acquired(guard)) => {
                // we hold the lock, so an existing socket file is left over from a GUI that ended
                let _ = std::fs::remove_file(&socket_path);
                let listener = UnixListener::bind(&socket_path)
                    .inspect_err(|e| {
                        log::warn!(
                            "could not listen on {}; starting the GUI again will open a second \
                             window: {e}",
                            socket_path.display()
                        );
                    })
                    .ok();
                Claim::Run(Some(Instance { guard, listener }))
            }
            Ok(Singleton::AlreadyRunning) => {
                // the running GUI may not be listening yet, right after its start
                for attempt in 0..10 {
                    match UnixStream::connect(&socket_path).and_then(|mut s| s.write_all(b"r")) {
                        Ok(()) => return Claim::Raised,
                        Err(e) if attempt == 9 => log::warn!(
                            "a GUI is running, but it couldn't be reached at {}; opening a \
                             second window: {e}",
                            socket_path.display()
                        ),
                        Err(_) => std::thread::sleep(Duration::from_millis(100)),
                    }
                }
                Claim::Run(None)
            }
            Err(e) => {
                log::warn!("could not check for a running GUI: {e}");
                Claim::Run(None)
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::io::Read as _;

        use super::*;

        fn temp_dir(name: &str) -> std::path::PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "modama-gui-instance-test-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            dir
        }

        #[test]
        fn a_second_start_reaches_the_running_gui() {
            let dir = temp_dir("second");
            let Claim::Run(Some(first)) = claim_in(&dir, ":0") else {
                panic!("the first start should run the GUI");
            };
            let listener = first.listener.as_ref().unwrap();
            assert!(matches!(claim_in(&dir, ":0"), Claim::Raised));
            let mut message = [0u8; 1];
            listener
                .accept()
                .unwrap()
                .0
                .read_exact(&mut message)
                .unwrap();
            assert_eq!(&message, b"r");
        }

        #[test]
        fn another_display_gets_its_own_gui() {
            let dir = temp_dir("display");
            let first = claim_in(&dir, ":0");
            assert!(matches!(first, Claim::Run(Some(_))));
            assert!(matches!(
                claim_in(&dir, "localhost:10"),
                Claim::Run(Some(_))
            ));
        }

        #[test]
        fn the_gui_starts_when_the_lock_fails() {
            // a regular file can't be the lock's directory:
            let blocker = temp_dir("blocked");
            std::fs::write(&blocker, b"").unwrap();
            assert!(matches!(claim_in(&blocker, ":0"), Claim::Run(None)));
        }

        #[test]
        fn a_leftover_socket_is_replaced() {
            let dir = temp_dir("leftover");
            std::fs::create_dir_all(&dir).unwrap();
            drop(UnixListener::bind(dir.join("gui-_0.sock")).unwrap());
            let Claim::Run(Some(instance)) = claim_in(&dir, ":0") else {
                panic!("the GUI should run");
            };
            assert!(instance.listener.is_some());
        }

        #[test]
        fn a_gui_that_does_not_answer_lets_this_one_start() {
            let dir = temp_dir("silent");
            let Claim::Run(Some(first)) = claim_in(&dir, ":0") else {
                panic!("the first start should run the GUI");
            };
            drop(first.listener);
            std::fs::remove_file(dir.join("gui-_0.sock")).unwrap();
            assert!(matches!(claim_in(&dir, ":0"), Claim::Run(None)));
        }

        #[test]
        fn the_screen_number_names_the_same_display() {
            let dir = temp_dir("screen");
            let _first = claim_in(&dir, ":1");
            assert!(matches!(claim_in(&dir, ":1.0"), Claim::Raised));
        }

        #[test]
        fn displays_become_file_names() {
            assert_eq!(file_name_part(":0"), "_0");
            assert_eq!(file_name_part(":0.0"), "_0");
            assert_eq!(file_name_part("localhost:10.0"), "localhost_10");
            assert_eq!(file_name_part("host.example.org:1"), "host.example.org_1");
            assert_eq!(file_name_part("/tmp/x:1"), "_tmp_x_1");
        }
    }
}
