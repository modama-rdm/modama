//! `--mount-popup`: the small window the session helper shows after it mounted network drives.
//! The helper sends what to show as JSON lines on stdin (see `modama_common::mount_popup`); the
//! user's answer goes to stdout as one line, and the window closes. If the helper goes away
//! (stdin ends), the window closes as well.

use std::{
    io::{self, BufRead as _, Write as _},
    sync::{Arc, Mutex},
    thread,
};

use eframe::egui::{self, Label, Layout, RichText, ViewportCommand};
use modama_common::mount_popup::{PopupChoice, PopupContent};

use crate::{
    components::buttons::{primary_button, secondary_button},
    styles,
};

pub const WIDTH: f32 = styles::components::error_toast::width::EGUI_POINTS;
/// The initial height; the window then follows its content
pub const INITIAL_HEIGHT: f32 = 192.0;

/// Distance from the monitor's right and bottom edge; the bottom one leaves room for a taskbar
const RIGHT_MARGIN: f32 = 16.0;
const BOTTOM_MARGIN: f32 = 64.0;

pub struct MountPopupApp {
    content: Arc<Mutex<Option<PopupContent>>>,
    on_answer: Box<dyn FnMut(PopupChoice)>,
    answered: bool,
    /// The window size we last asked for, and placed the window for
    placed_for: Option<egui::Vec2>,
}

impl MountPopupApp {
    pub fn new(
        content: Arc<Mutex<Option<PopupContent>>>,
        on_answer: Box<dyn FnMut(PopupChoice)>,
    ) -> Self {
        Self {
            content,
            on_answer,
            answered: false,
            placed_for: None,
        }
    }

    fn answer(&mut self, ctx: &egui::Context, choice: PopupChoice) {
        if !self.answered {
            self.answered = true;
            (self.on_answer)(choice);
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// Fit the window to `height` and put it at the bottom right of the monitor (where the
    /// window system lets us; Wayland doesn't)
    fn place(&mut self, ctx: &egui::Context, height: f32) {
        let size = egui::vec2(WIDTH, height.ceil());
        if self
            .placed_for
            .is_some_and(|placed| (placed - size).length() < 1.0)
        {
            return;
        }
        let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) else {
            return;
        };
        self.placed_for = Some(size);
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(
            (monitor.x - size.x - RIGHT_MARGIN).max(0.0),
            (monitor.y - size.y - BOTTOM_MARGIN).max(0.0),
        )));
    }
}

impl eframe::App for MountPopupApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // the same item spacing as the main window (see `AgentGuiApp::ui`)
        ui.style_mut().spacing.item_spacing = egui::vec2(
            styles::spacing::app_item_gap_x::EGUI_POINTS,
            styles::spacing::app_item_gap_y::EGUI_POINTS,
        );
        let ctx = ui.ctx().clone();
        let content = self.content.lock().ok().and_then(|content| content.clone());
        let padding = egui::vec2(
            styles::components::content_card_padding_x::padding::EGUI_POINTS,
            styles::components::content_card_padding_y::padding::EGUI_POINTS,
        );
        let frame = egui::Frame::new()
            .fill(styles::color::surface::VALUE.resolve(ui.visuals()))
            .inner_margin(egui::Margin::symmetric(padding.x as i8, padding.y as i8));
        let response = egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            let Some(content) = content else {
                return (None, None);
            };
            ui.add(Label::new(
                RichText::new(if content.mounts.len() == 1 {
                    "Ingest share connected"
                } else {
                    "Ingest shares connected"
                })
                .font(styles::typography::section_heading::font_id()),
            ));
            for mount in &content.mounts {
                ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
                ui.add(Label::new(
                    RichText::new(&mount.session).font(styles::typography::mono_medium::font_id()),
                ));
                ui.add(Label::new(
                    RichText::new(format!(
                        "{} · {}",
                        mount.project,
                        mount.mount_path.display()
                    ))
                    .font(styles::typography::supporting_text::font_id())
                    .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
                ));
            }
            // like the confirmation dialog and the autostart offer
            ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
            // top-aligned: a centered row would take all the height there is, and the window,
            // which follows the content, could never shrink when mounts leave the popup
            let buttons = ui.with_layout(Layout::right_to_left(egui::Align::Min), |ui| {
                let open = ui.add(primary_button("Open ingest share", ui)).clicked();
                let dismiss = ui.add(secondary_button("Dismiss", ui)).clicked();
                if open {
                    Some(PopupChoice::Open)
                } else if dismiss {
                    Some(PopupChoice::Dismiss)
                } else {
                    None
                }
            });
            // the window follows the content: up to the buttons' bottom, plus the padding below
            // (not the panel's, which is as tall as the window already is)
            (buttons.inner, Some(buttons.response.rect.max.y + padding.y))
        });
        let (choice, height) = response.inner;
        if let Some(choice) = choice {
            self.answer(&ctx, choice);
        }
        if let Some(height) = height {
            self.place(&ctx, height);
        }
    }
}

/// Write the answer for the helper
fn answer_on_stdout(choice: PopupChoice) {
    let result = serde_json::to_string(&choice)
        .map_err(io::Error::other)
        .and_then(|line| {
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{line}")?;
            stdout.flush()
        });
    if let Err(e) = result {
        log::error!("could not answer the session helper: {e}");
    }
}

/// Follow the helper's updates on stdin; close the window once the helper is gone
fn read_helper(content: Arc<Mutex<Option<PopupContent>>>, ctx: egui::Context) {
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        match serde_json::from_str::<PopupContent>(&line) {
            Ok(update) => {
                if let Ok(mut content) = content.lock() {
                    *content = Some(update);
                }
                ctx.request_repaint();
            }
            Err(e) => log::warn!("ignoring an invalid message from the session helper: {e}"),
        }
    }
    ctx.send_viewport_cmd(ViewportCommand::Close);
}

/// Show the popup until the user answers or the helper goes away
pub fn run() -> eframe::Result {
    let content = Arc::new(Mutex::new(None));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("modama-agent")
            .with_inner_size([WIDTH, INITIAL_HEIGHT])
            .with_resizable(false)
            .with_minimize_button(false)
            .with_maximize_button(false)
            .with_always_on_top(),
        // like the GUI's window: no GL backend where a primary backend works
        wgpu_options: crate::wgpu_setup::wgpu_options(),
        ..Default::default()
    };
    eframe::run_native(
        "modama-mount-popup",
        options,
        Box::new(move |cctx| {
            crate::wgpu_setup::log_adapter(cctx);
            egui_extras::install_image_loaders(&cctx.egui_ctx);
            styles::theme::install(&cctx.egui_ctx);
            let reader_content = Arc::clone(&content);
            let ctx = cctx.egui_ctx.clone();
            thread::spawn(move || read_helper(reader_content, ctx));
            Ok(Box::new(MountPopupApp::new(
                content,
                Box::new(answer_on_stdout),
            )))
        }),
    )
}
