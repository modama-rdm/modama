#![cfg(test)]

use std::sync::{Arc, Mutex};

use eframe::egui;
use egui_kittest::{Harness, SnapshotResults, kittest::Queryable};
use modama_agent_gui::{
    mount_popup::{INITIAL_HEIGHT, MountPopupApp, WIDTH},
    styles,
};
use modama_common::mount_popup::{PopupChoice, PopupContent, PopupMount};

fn mount(session: &str, path: &str) -> PopupMount {
    PopupMount {
        project: "representative-project".to_owned(),
        session: session.to_owned(),
        mount_path: path.into(),
    }
}

fn popup_harness(
    mounts: Vec<PopupMount>,
    height: f32,
    answers: Arc<Mutex<Vec<PopupChoice>>>,
) -> Harness<'static, MountPopupApp> {
    let content = Arc::new(Mutex::new(Some(PopupContent { mounts })));
    let mut harness = Harness::new_eframe(move |cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        styles::theme::install(&cc.egui_ctx);
        let answers = Arc::clone(&answers);
        MountPopupApp::new(
            content,
            Box::new(move |choice| {
                if let Ok(mut answers) = answers.lock() {
                    answers.push(choice);
                }
            }),
        )
    });
    harness
        .set_size(egui::vec2(WIDTH, height))
        .set_pixels_per_point(1.0);
    harness.ctx.set_theme(egui::Theme::Light);
    harness.run_steps(3);
    harness
}

#[test]
fn mount_popup_snapshots() {
    let mut results = SnapshotResults::new();

    let mut one = popup_harness(
        vec![mount("2026-07-29-ingest", "Z:\\")],
        INITIAL_HEIGHT,
        Arc::default(),
    );
    one.snapshot("mount_popup");
    results.extend_harness(&mut one);

    let mut two = popup_harness(
        vec![
            mount("2026-07-29-ingest", "Z:\\"),
            mount("2026-07-30-ingest", "Y:\\"),
        ],
        INITIAL_HEIGHT + 64.0,
        Arc::default(),
    );
    two.snapshot("mount_popup_two");
    results.extend_harness(&mut two);
}

#[test]
fn buttons_answer_once() {
    let answers = Arc::new(Mutex::new(Vec::new()));
    let mut harness = popup_harness(
        vec![mount("2026-07-29-ingest", "Z:\\")],
        INITIAL_HEIGHT,
        Arc::clone(&answers),
    );

    harness.get_by_label("Open ingest share").click();
    harness.run_steps(2);
    harness.get_by_label("Dismiss").click();
    harness.run_steps(2);

    assert_eq!(*answers.lock().unwrap(), vec![PopupChoice::Open]);
}
