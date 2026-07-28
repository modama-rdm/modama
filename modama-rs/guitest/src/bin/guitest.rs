#![allow(clippy::unwrap_used)]
use std::{
    error::Error,
    sync::mpsc::{Receiver, Sender, TryRecvError},
    thread::JoinHandle,
    time::Duration,
};

use eframe::egui;
use guitest::async_state::{AsyncRequest, AsyncResponse, AsyncState};

type MessageId = String;

impl Page {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) -> Action {
        match self {
            Page::A { inner } => inner.ui(ui, frame),
            Page::B { inner } => inner.ui(ui, frame),
            Page::C { inner } => inner.ui(ui, frame),
        }
    }

    fn on_background_msg(&mut self, msg: &AsyncResponse<BackgroundResponse>) -> Action {
        match self {
            Page::A { inner } => Action::None,
            Page::B { inner } => inner.on_background_message(msg),
            Page::C { inner } => Action::None,
        }
    }
}

struct MyApp {
    bg: Background,
    current_page: Page,
}

impl MyApp {
    fn new(ctx: eframe::egui::Context) -> Self {
        Self {
            current_page: Page::A {
                inner: Default::default(),
            },
            bg: Background::spawn(ctx),
        }
    }

    fn dispatch_action(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::ConfirmName { name } => {
                self.current_page = Page::C {
                    inner: StateC { name },
                };
            }
            Action::GoToEnterName => {
                self.current_page = Page::B {
                    inner: StateB::new(self.bg.to_bg.clone()),
                }
            }
        }
    }

    fn check_for_bg(&mut self) -> Action {
        match self.bg.from_bg.try_recv() {
            Ok(msg) => {
                self.current_page.on_background_msg(&msg)
            }
            Err(TryRecvError::Empty) => {
                // nothing to do here I guess
                Action::None
            }
            Err(TryRecvError::Disconnected) => {
                panic!("background thread is gone");
            }
        }

        /*
        match self.bg.from_bg.try_recv() {
            Ok(BackgroundResponse::CheckName {
                ref name,
                good_name: name_is_good,
            }) => {
                if name_is_good {
                    self.dispatch_action(Action::ConfirmName { name: name.clone() });
                } else {
                    // TODO: need to unblock the UI which is waiting for confirmation from bg...
                    todo!("not good");
                }
                Some(msg.clone())
            }
            Ok(msg @ BackgroundResponse::ShutdownComplete) => {
                eprintln!("bg shut down complete");
                self.bg.join_handle.take().unwrap().join().unwrap();
                Some(msg)
            }
            Err(TryRecvError::Empty) => {
                // nothing to do here I guess
                None
            }
            Err(TryRecvError::Disconnected) => {
                panic!("background thread is gone");
            }
        }*/
    }
}

impl eframe::App for MyApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let action = self.current_page.ui(ui, frame);
        self.dispatch_action(action);
        let bg = self.check_for_bg();
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
