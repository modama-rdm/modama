//! End to end: a hand-off between two researchers in one login session, then stopping the
//! new ingest.
//!
//! clausen starts an ingest at the resource; weber, in a second GUI, starts one after the
//! warning that clausen's is there. smbdrop keeps the ingest shares of one user per resource:
//! clausen's keeps existing but is no longer at the resource, and the session helper replaces
//! its mount by weber's. clausen's GUI shows weber's ingest share but can't stop it. Stopping
//! weber's ingest removes the share and the mount. See `e2e_common` for how the tests run.
#![cfg(test)]
#![cfg(feature = "e2e")]

mod e2e_common;

use std::time::Duration;

use e2e_common::{
    click_button, gui, has_button, has_text, mount_uri, resource, sign_in, smbdrop_share,
    start_ingest, wait_for, wait_until,
};

#[test]
fn hand_off_to_another_researcher_then_stop() {
    let resource = resource();

    let mut clausen = gui();
    sign_in(&mut clausen, "clausen", "1234");
    let first = start_ingest(&mut clausen, "testproject");
    assert_eq!(
        smbdrop_share(&first.share_name).expect("smbdrop has clausen's ingest share")["resource"],
        resource.as_str()
    );

    // weber is warned whose ingest share the new one displaces
    let mut weber = gui();
    sign_in(&mut weber, "weber", "1234");
    let second = start_ingest(&mut weber, "weberproject");
    let warning = second
        .warning
        .expect("the GUI warned about clausen's ingest share");
    assert!(
        warning.contains(&format!(
            "clausen started the ingest share for session {}",
            first.session
        )),
        "unexpected warning: {warning:?}"
    );

    // the resource has weber's ingest share, clausen's still exists elsewhere
    let share = smbdrop_share(&second.share_name).expect("smbdrop has weber's ingest share");
    assert_eq!(share["resource"], resource.as_str());
    let displaced = smbdrop_share(&first.share_name).expect("clausen's ingest share still exists");
    assert!(
        displaced["resource"].is_null(),
        "clausen's ingest share is still at {}",
        displaced["resource"]
    );

    // the session helper mounted weber's and removed clausen's
    assert!(
        mount_uri(&second.share_name).is_some(),
        "weber's ingest share isn't mounted"
    );
    wait_for("clausen's mount to go", Duration::from_secs(60), || {
        mount_uri(&first.share_name).is_none()
    });

    // clausen's GUI, still open and signed in, shows weber's ingest share, which only weber
    // can stop
    wait_until(
        &mut clausen,
        "clausen's GUI to show weber's ingest share",
        Duration::from_secs(60),
        |h| has_text(h, &format!("share={}-", second.session)),
    );
    assert!(!has_button(&clausen, "Stop Ingest"));
    drop(clausen);

    // stopping removes weber's ingest share and its mount
    click_button(&mut weber, "Stop Ingest");
    wait_until(
        &mut weber,
        "the confirmation",
        Duration::from_secs(10),
        |h| has_text(h, "Stop ingest?"),
    );
    click_button(&mut weber, "Stop Ingest");
    let share_marker = format!("share={}-", second.session);
    wait_until(
        &mut weber,
        "the GUI to show the ingest as stopped",
        Duration::from_secs(60),
        |h| !has_text(h, &share_marker),
    );
    wait_for(
        "smbdrop to remove weber's ingest share",
        Duration::from_secs(30),
        || smbdrop_share(&second.share_name).is_none(),
    );
    wait_for("weber's mount to go", Duration::from_secs(60), || {
        mount_uri(&second.share_name).is_none()
    });
}
