//! End to end: start an ingest in the GUI against test-env (`modama-rs/test-env`), and find
//! a file written to the ingest share in the session folder.
//!
//! What it checks is what the unit and visual tests can't: the GUI's calls against the real
//! agent, the event streams between agent, GUI and session helper, the GVfs mount against
//! Samba, and that a file written to the ingest share lands in the session folder as its user.
//! See `e2e_common` for how the tests run.
#![cfg(test)]
#![cfg(feature = "e2e")]

mod e2e_common;

use std::{fs, os::unix::fs::MetadataExt as _, path::Path};

use e2e_common::{gio, gui, mount_uri, setting, sign_in, start_ingest};

#[test]
fn start_an_ingest_in_the_gui() {
    let project = setting("E2E_PROJECT", "testproject");
    let data_root = setting("E2E_DATA_ROOT", "/data/adhoc");
    let mut harness = gui();
    sign_in(&mut harness, "clausen", "1234");
    let ingest = start_ingest(&mut harness, &project);

    // GVfs in session-1 has mounted it
    let uri = mount_uri(&ingest.share_name).unwrap_or_else(|| {
        panic!(
            "gio mount -l doesn't list {}:\n{}",
            ingest.share_name,
            gio(&["mount", "-l"], None)
        )
    });

    // a file written to the ingest share lands in the session folder, as its user
    let content = format!("written by the end-to-end test into {}\n", ingest.session);
    gio(
        &[
            "save",
            &format!("{}/raw/e2e.txt", uri.trim_end_matches('/')),
        ],
        Some(&content),
    );
    let session_folder = Path::new(&data_root).join(&project).join(&ingest.session);
    let written = session_folder.join("raw").join("e2e.txt");
    assert_eq!(
        fs::read_to_string(&written).expect("the file is in the session folder"),
        content
    );
    let owner = |path: &Path| fs::metadata(path).expect("metadata").uid();
    assert_eq!(
        owner(&written),
        owner(&session_folder),
        "the file isn't owned by the session folder's owner"
    );
}
