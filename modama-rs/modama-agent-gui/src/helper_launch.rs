use std::{
    io,
    process::{Child, Command, Stdio},
    thread,
};

/// Starts a session helper when the agent reports none for our scope
pub trait HelperLauncher: Send {
    fn launch(&mut self);
}

/// Start `modama-agent session-helper` from the GUI's own directory, detached from the GUI.
/// A helper that is already running makes the new one exit right away (singleton).
pub fn launch_session_helper(agent_url: &str) -> io::Result<Child> {
    let exe_dir = std::env::current_exe()?
        .parent()
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::other("the GUI executable has no parent directory"))?;
    let agent = exe_dir.join(format!("modama-agent{}", std::env::consts::EXE_SUFFIX));

    let mut command = Command::new(agent);
    command
        .args(["session-helper", "--agent-url", agent_url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // keep Ctrl-C in the GUI's terminal away from the helper:
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    // the helper is a console program; don't open a console window for it:
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};
        command.creation_flags(CREATE_NO_WINDOW.0 | CREATE_NEW_PROCESS_GROUP.0);
    }

    command.spawn()
}

pub struct ProcessLauncher {
    agent_url: String,
}

impl ProcessLauncher {
    pub fn new(agent_url: impl Into<String>) -> Self {
        Self {
            agent_url: agent_url.into(),
        }
    }
}

impl HelperLauncher for ProcessLauncher {
    fn launch(&mut self) {
        match launch_session_helper(&self.agent_url) {
            Ok(mut child) => {
                log::info!("launched session helper (pid {})", child.id());
                // reap it, so it doesn't stay around as a zombie while the GUI runs:
                thread::spawn(move || child.wait());
            }
            Err(error) => log::warn!("could not launch the session helper: {error}"),
        }
    }
}
