//! Shared by the end-to-end tests (`tests/e2e_*.rs`): the GUI in egui_kittest against
//! test-env's agent1, and checks of what the session helper in `session-1` and smbdrop did.
//!
//! The tests run in test-env's `gui-e2e` service, one at a time, against one stack: each takes
//! the resource as the previous test left it.
#![cfg(test)]
// each test uses some of these helpers, none all of them
#![allow(dead_code)]

use std::{
    env,
    io::Write as _,
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use eframe::egui::{self, accesskit::Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use modama_agent_gui::{
    app::AgentGuiApp, helper_launch::HelperLauncher, service::HttpAgentService, styles,
};

pub type AppHarness = Harness<'static, AgentGuiApp>;

pub fn setting(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

/// The resource of test-env's agents
pub fn resource() -> String {
    setting("E2E_RESOURCE", "testresource")
}

/// A name no earlier run used, for sessions
pub fn unique() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_nanos();
    format!("{:x}", nanos % 0x1_0000_0000)
}

/// In test-env, `session-1` runs the session helper; the GUI must not start another one (here,
/// its own executable would be the test binary)
struct NoLauncher;

impl HelperLauncher for NoLauncher {
    fn launch(&mut self) {
        log::error!("the agent reports no session helper; is session-1's helper connected?");
    }
}

/// A GUI on agent1, as started on the instrument computer
pub fn gui() -> AppHarness {
    let agent_url = setting("E2E_AGENT_URL", "http://localhost:3003/");
    let service = Arc::new(HttpAgentService::new(agent_url).expect("could not create the service"));
    let mut harness = Harness::new_eframe(move |cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        styles::theme::install(&cc.egui_ctx);
        AgentGuiApp::with_service(service).with_helper_launcher(|_| Box::new(NoLauncher))
    });
    harness.set_size(egui::vec2(
        styles::components::app_window::width::EGUI_POINTS,
        styles::components::app_window::height::EGUI_POINTS,
    ));
    harness
}

/// What a widget says, as kittest's label queries see it: a label keeps its text in
/// AccessKit's `value`, other widgets in `label`
pub fn text(node: &egui_kittest::Node<'_>) -> Option<String> {
    let node = node.accesskit_node();
    if node.role() == Role::Label {
        node.value()
    } else {
        node.label()
    }
}

/// Every text the GUI shows, for failure messages
pub fn texts(harness: &AppHarness) -> Vec<String> {
    harness
        .query_all_by_label_contains("")
        .filter_map(|node| text(&node))
        .collect()
}

/// A few frames for an input to take effect. Not `Harness::run`: spinners keep asking for
/// repaints, so the GUI never comes to rest while it waits for the agent.
pub fn settle(harness: &mut AppHarness) {
    harness.run_steps(4);
}

/// Step the GUI until `done` holds; the agent's answers arrive in the background
pub fn wait_until(
    harness: &mut AppHarness,
    what: &str,
    timeout: Duration,
    done: impl Fn(&AppHarness) -> bool,
) {
    let deadline = Instant::now() + timeout;
    loop {
        harness.step();
        if done(harness) {
            return;
        }
        if Instant::now() > deadline {
            panic!(
                "timeout after {timeout:?} waiting for {what}; the GUI shows: {:?}",
                texts(harness)
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Poll `done` outside the GUI (GVfs, smbdrop) until it holds
pub fn wait_for(what: &str, timeout: Duration, done: impl Fn() -> bool) {
    let deadline = Instant::now() + timeout;
    while !done() {
        assert!(
            Instant::now() < deadline,
            "timeout after {timeout:?} waiting for {what}"
        );
        thread::sleep(Duration::from_millis(500));
    }
}

/// An enabled button named `label` (the GUI greys buttons out while it waits for the agent)
pub fn has_button(harness: &AppHarness, label: &str) -> bool {
    harness
        .query_all_by_role_and_label(Role::Button, label)
        .any(|button| !button.accesskit_node().is_disabled())
}

pub fn has_text(harness: &AppHarness, part: &str) -> bool {
    harness.query_all_by_label_contains(part).next().is_some()
}

/// Wait for an enabled button named `label` and click it; of several, the last one, which is
/// a dialog's rather than the page's below it
pub fn click_button(harness: &mut AppHarness, label: &str) {
    wait_until(
        harness,
        &format!("button {label:?}"),
        Duration::from_secs(20),
        |h| has_button(h, label),
    );
    harness
        .query_all_by_role_and_label(Role::Button, label)
        .rfind(|button| !button.accesskit_node().is_disabled())
        .expect("the button is there")
        .click();
    settle(harness);
}

pub fn type_into(harness: &mut AppHarness, role: Role, label: &str, text: &str) {
    harness.get_by_role_and_label(role, label).focus();
    settle(harness);
    harness.get_by_role_and_label(role, label).type_text(text);
    settle(harness);
}

pub fn sign_in(harness: &mut AppHarness, user: &str, password: &str) {
    click_button(harness, "Log in");
    wait_until(harness, "the login form", Duration::from_secs(10), |h| {
        has_button(h, "Sign in")
    });
    type_into(harness, Role::TextInput, "Username", user);
    type_into(harness, Role::PasswordInput, "Password", password);
    click_button(harness, "Sign in");
    wait_until(
        harness,
        &format!("signing in as {user}"),
        Duration::from_secs(20),
        |h| has_button(h, "Log out"),
    );
}

/// An ingest started in the GUI and mounted by the session helper
pub struct Ingest {
    pub session: String,
    /// What the GUI shows as the mount path on this computer
    pub mount_path: String,
    pub share_name: String,
    /// The warning the GUI showed before starting it, if another user's ingest share was there
    pub warning: Option<String>,
}

/// Start an ingest in a new session of `project`, as the signed-in user, and wait until the
/// GUI shows its mount path. Questions about ingest shares that are there already (ours or
/// another user's) are answered so that the new one starts.
pub fn start_ingest(harness: &mut AppHarness, project: &str) -> Ingest {
    click_button(harness, "Start Ingest");
    let project_prefix = format!("{project}, owner ");
    wait_until(harness, "the project list", Duration::from_secs(20), |h| {
        has_button(h, "Keep it") || has_button(h, "Continue") || has_text(h, &project_prefix)
    });
    let warning = harness
        .query_all_by_label_contains("started the ingest share")
        .find_map(|node| text(&node));
    for question in ["Keep it", "Continue"] {
        if has_button(harness, question) {
            click_button(harness, question);
        }
    }
    wait_until(harness, "the project", Duration::from_secs(20), |h| {
        has_text(h, &project_prefix)
    });
    harness.get_by_label_contains(&project_prefix).click();
    settle(harness);
    click_button(harness, "Select Project");
    click_button(harness, "Create a new session");

    let suffix = format!("e2e-{}", unique());
    harness
        .get_by_role_and_label(Role::TextInput, "Session name")
        .focus();
    settle(harness);
    harness.key_press(egui::Key::End);
    type_into(harness, Role::TextInput, "Session name", &suffix);
    let session = harness
        .get_by_role_and_label(Role::TextInput, "Session name")
        .value()
        .expect("the session name field has a value");
    assert!(
        session.ends_with(&suffix),
        "the session name is {session:?}, not ending with {suffix:?}"
    );
    click_button(harness, "Create Session");

    // the session helper reports the mount, the GUI shows its path
    let share_marker = format!("share={session}-");
    wait_until(
        harness,
        &format!("the mount path of {session}"),
        Duration::from_secs(60),
        |h| has_text(h, &share_marker),
    );
    let mount_path = harness
        .query_all_by_label_contains(&share_marker)
        .find_map(|node| text(&node))
        .expect("the mount path has a text");
    assert!(
        mount_path.starts_with("/run/user/1000/gvfs/"),
        "unexpected mount path {mount_path:?}"
    );
    let share_name = mount_path
        .rsplit("share=")
        .next()
        .expect("the mount path names the share")
        .trim_end_matches('/')
        .to_owned();
    Ingest {
        session,
        mount_path,
        share_name,
        warning,
    }
}

/// Run a command that must succeed and return its output
fn run(program: &str, args: &[&str], stdin: Option<&str>) -> String {
    let mut child = Command::new(program)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("could not run {program}: {e}"));
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .expect("stdin is piped")
            .write_all(input.as_bytes())
            .expect("could not write to the command");
    }
    let output = child
        .wait_with_output()
        .expect("the command did not finish");
    assert!(
        output.status.success(),
        "{program} {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `gio` in the shared login session, which reaches `session-1`'s GVfs daemons
pub fn gio(args: &[&str], stdin: Option<&str>) -> String {
    run("gio", args, stdin)
}

/// The GVfs mounts in `session-1`'s login session, as URIs (`smb://host/share/`)
pub fn gio_mounts() -> Vec<String> {
    gio(&["mount", "-l"], None)
        .lines()
        .filter_map(|line| line.split(" -> ").nth(1))
        .map(|uri| uri.trim().to_owned())
        .collect()
}

/// The URI of the GVfs mount of the ingest share `share_name`, if it is mounted
pub fn mount_uri(share_name: &str) -> Option<String> {
    gio_mounts().into_iter().find(|uri| {
        uri.trim_end_matches('/')
            .ends_with(&format!("/{share_name}"))
    })
}

/// smbdrop's ingest shares, from its unauthenticated API
pub fn smbdrop_shares() -> Vec<serde_json::Value> {
    let url = setting("E2E_SMBDROP_API", "http://smbdrop:5051") + "/share/";
    let body = run("curl", &["-fsS", "-m", "10", &url], None);
    let shares: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&body).expect("smbdrop lists shares as a JSON object");
    shares.into_values().collect()
}

/// smbdrop's ingest share `share_name`, if it exists
pub fn smbdrop_share(share_name: &str) -> Option<serde_json::Value> {
    smbdrop_shares()
        .into_iter()
        .find(|share| share["share_name"] == share_name)
}
