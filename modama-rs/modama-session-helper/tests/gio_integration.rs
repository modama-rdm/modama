//! GIO backend against real GVfs and a real Samba server. Needs GVfs (`gvfs-backends`,
//! `gvfs-fuse`) and a D-Bus session in the running user session, plus the Samba container on
//! 127.0.0.1:1445; run it via `just gio-integration-test`.
//!
//! It mounts into the real user session; the mounts are briefly visible in the file manager.
#![cfg(target_os = "linux")]

use std::{
    path::Path,
    time::{Duration, Instant},
};

use modama_common::{
    internal_api_types::{SessionHelperConfig, SessionHelperState},
    messages::Share,
};
use modama_session_helper::{gio_backend::GioBackend, mount_backend::MountBackend};

fn config(host: &str) -> SessionHelperConfig {
    SessionHelperConfig {
        smb_hostname: host.to_owned(),
        smb_port: 1445,
        smb_domain: "WORKGROUP".to_owned(),
    }
}

fn share(id: &str, name: &str, password: &str) -> Share {
    Share {
        resource: None,
        share_id: id.to_owned(),
        share_name: name.to_owned(),
        dest_path: format!("/project/{id}"),
        created_timestamp: 0,
        username: "testuser".to_owned(),
        password: password.to_owned(),
        uid: 1000,
        gid: 1000,
    }
}

fn mount_path<'a>(state: &'a SessionHelperState, name: &str) -> Option<&'a Path> {
    state
        .mounted_shares
        .iter()
        .find(|m| m.share_name == name)
        .and_then(|m| m.mount_path.as_deref())
}

#[tokio::test]
#[ignore = "needs GVfs and the Samba container; run via `just gio-integration-test`"]
async fn gio_backend_against_samba() {
    let backend = GioBackend::new().await.unwrap();
    let localhost = config("localhost");
    let ingest = share("1", "ingest", "testpass");

    // 0. start clean: a run killed before the recipe's cleanup leaves its mounts behind, and
    // the first pass below must actually mount
    for host in [&localhost, &config("127.0.0.1")] {
        let state = backend.reconcile(&[], host, false).await.unwrap().state;
        assert!(state.mounted_shares.is_empty(), "{state:?}");
    }

    // 1. mount: the share gets a FUSE path in the user's runtime directory, and is usable
    let result = backend
        .reconcile(std::slice::from_ref(&ingest), &localhost, false)
        .await
        .unwrap();
    assert_eq!(result.created, vec!["ingest"]);
    let state = result.state;
    assert!(state.errors.is_empty(), "{state:?}");
    let path = mount_path(&state, "ingest").expect("ingest mounted with a path");
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap();
    assert!(
        path.starts_with(Path::new(&runtime_dir).join("gvfs")),
        "{path:?}"
    );
    let file = path.join("gio-integration-test.txt");
    std::fs::write(&file, b"hello").unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"hello");
    std::fs::remove_file(&file).unwrap();

    // 2. an unchanged desired state leaves everything as it is: our mount is recognized
    let again = backend
        .reconcile(std::slice::from_ref(&ingest), &localhost, false)
        .await
        .unwrap();
    assert!(again.created.is_empty(), "{again:?}");
    assert_eq!(again.state, state);

    // 3. a wrong password fails quickly, i.e. it isn't retried until the timeout
    let wrong = share("2", "ingest2", "wrong-password");
    let start = Instant::now();
    let state = backend
        .reconcile(&[ingest.clone(), wrong], &localhost, false)
        .await
        .unwrap()
        .state;
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(state.errors.len(), 1, "{state:?}");
    assert_eq!(state.errors[0].share_id, "2");
    assert!(
        !state.errors[0].message.contains("wrong-password"),
        "{state:?}"
    );
    assert!(mount_path(&state, "ingest").is_some());

    // 4. mounts to another host (here: the same server under another name) are left alone
    let other_host = config("127.0.0.1");
    let other = share("3", "other", "testpass");
    let state = backend
        .reconcile(std::slice::from_ref(&other), &other_host, false)
        .await
        .unwrap()
        .state;
    assert!(mount_path(&state, "other").is_some(), "{state:?}");
    let state = backend
        .reconcile(&[], &localhost, false)
        .await
        .unwrap()
        .state;
    assert!(state.mounted_shares.is_empty(), "{state:?}");
    let state = backend
        .reconcile(std::slice::from_ref(&other), &other_host, false)
        .await
        .unwrap()
        .state;
    assert!(mount_path(&state, "other").is_some(), "{state:?}");

    // 5. an empty desired state removes our mounts, and their paths disappear
    let state = backend
        .reconcile(&[], &other_host, false)
        .await
        .unwrap()
        .state;
    assert!(state.mounted_shares.is_empty(), "{state:?}");
    assert!(!path.exists(), "{path:?} still exists");
}
