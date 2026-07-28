#![allow(clippy::unwrap_used)]
use std::error::Error;

use eframe::egui;
use guitest::{
    actions::Action,
    pages::{page_b::StateB, page_c::StateC, Page},
};

struct MyApp {
    current_page: Page,
    ctx: eframe::egui::Context,
}

impl MyApp {
    fn new(ctx: eframe::egui::Context) -> Self {
        Self {
            current_page: Page::A {
                inner: Default::default(),
            },
            ctx,
        }
    }

    fn dispatch_action(&mut self, action: Action) {
        if !action.is_none() {
            self.ctx.request_repaint();
        }
        match action {
            Action::None => {}
            Action::ConfirmName { name } => {
                self.current_page = Page::C {
                    inner: StateC::new(name),
                };
            }
            Action::GoToEnterName => {
                self.current_page = Page::B {
                    inner: StateB::new(),
                }
            }
        }
    }
}

impl eframe::App for MyApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let action = self.current_page.poll();
        self.dispatch_action(action);
        let action = self.current_page.ui(ui, frame);
        self.dispatch_action(action);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1024., 1024.0]),
        ..Default::default()
    };

    eframe::run_native(
        "modama-agent",
        options,
        Box::new(|cctx| {
            egui_extras::install_image_loaders(&cctx.egui_ctx);
            let app = MyApp::new(cctx.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    )?;

    Ok(())
}
