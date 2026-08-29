use std::path::PathBuf;

use egui_kittest::{SnapshotOptions, SnapshotResults, try_image_snapshot_options};
use modama_agent_gui::guide_screenshots::{self, GuideScene};

fn guide_image_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide/src/user/images/agent")
}

#[test]
fn sentinel_wgpu_debug_disabled() {
    let val = std::env::var("WGPU_DEBUG").expect("WGPU_DEBUG should be set");
    assert_eq!(val, "0");
}

#[test]
fn guide_screenshots_match_the_mdbook_assets() {
    let options = SnapshotOptions::new().output_path(guide_image_dir());
    let mut results = SnapshotResults::new();

    for scene in GuideScene::ALL {
        match guide_screenshots::render(scene) {
            Ok(image) => results.add(try_image_snapshot_options(
                &image,
                scene.snapshot_name(),
                &options,
            )),
            Err(error) => panic!("could not render {}: {error}", scene.filename()),
        }
    }

    results.unwrap();
}
