//! End to end: a mount removed behind the session helper's back comes back.
//!
//! After `gio mount -u`, the helper's next pass (every 30 s) finds the ingest share missing
//! and mounts it again: its reconcile loop against real GVfs, where the unit tests have a
//! stand-in. See `e2e_common` for how the tests run.
#![cfg(test)]
#![cfg(feature = "e2e")]

mod e2e_common;

use std::time::Duration;

use e2e_common::{gio, gui, has_text, mount_uri, sign_in, start_ingest, wait_for, wait_until};

#[test]
fn a_removed_mount_comes_back() {
    let mut harness = gui();
    sign_in(&mut harness, "clausen", "1234");
    let ingest = start_ingest(&mut harness, "testproject");
    let uri = mount_uri(&ingest.share_name).expect("GVfs has mounted the ingest share");

    gio(&["mount", "-u", &uri], None);
    wait_for("the mount to go", Duration::from_secs(10), || {
        mount_uri(&ingest.share_name).is_none()
    });
    wait_for(
        "the session helper to mount it again",
        Duration::from_secs(90),
        || mount_uri(&ingest.share_name).is_some(),
    );

    let share_marker = format!("share={}-", ingest.session);
    wait_until(
        &mut harness,
        "the GUI to show the mount path again",
        Duration::from_secs(30),
        |h| has_text(h, &share_marker),
    );
}
