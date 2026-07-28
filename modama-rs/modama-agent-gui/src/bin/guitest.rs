#![allow(clippy::unwrap_used)]
use std::{
    error::Error,
    sync::mpsc::{Receiver, Sender, TryRecvError},
    thread::JoinHandle,
    time::Duration,
};

use eframe::egui;

fn bleh() {
    let a = AsyncState::default();
}

#[derive(Default)]
struct StateA {}

#[derive(PartialEq, Eq, Debug)]
enum Action {
    /// UI should stay at the current state and view/page
    None,

    /// Navigate to the "enter name" view
    GoToEnterName,

    /// `name` is verified, go to name confirmation page
    ConfirmName { name: String },
}

type MessageId = String;

#[derive(PartialEq, Eq, Debug)]
struct BackgroundRequestMessage {
    message_id: MessageId,
    request: BackgroundRequest,
}

#[derive(PartialEq, Eq, Debug)]
enum BackgroundRequest {
    CheckName { name: String },
    Shutdown,
}

impl BackgroundRequest {
    fn into_message(self, msg_id: MessageId) -> BackgroundRequestMessage {
        BackgroundRequestMessage { message_id: msg_id, request: self }
    }
}

#[derive(PartialEq, Eq, Debug)]
struct BackgroundResponseMessage {
    message_id: MessageId,
    response: BackgroundResponse,
}

#[derive(PartialEq, Eq, Debug, Clone)]
enum BackgroundResponse {
    CheckName { name: String, good_name: bool },
    ShutdownComplete,
}

impl BackgroundResponse {
    fn into_message(self, message_id: MessageId) -> BackgroundResponseMessage {
        BackgroundResponseMessage { message_id, response: self }
    }
}

/// Background work that is closely related to the current
/// foreground work - most of the time this will be kind-of
/// synchronous, meaning the UI will be disabled while the bg
/// thread is processing. There may be exceptions, like autocomplete
/// or search fields where we obviously want to continue the UI to
/// be used while we fetch suggestions.
fn bg_thread_loop(
    rx: Receiver<BackgroundRequestMessage>,
    tx: Sender<BackgroundResponseMessage>,
    ctx: eframe::egui::Context,
) {
    let mut lmao = 0u64;
    loop {
        let msg = rx.recv().unwrap();
        match msg.request {
            BackgroundRequest::CheckName { name } => {
                std::thread::sleep(Duration::from_millis(500));
                if lmao > 3 {
                    eprintln!("{name} is good");
                    tx.send(BackgroundResponse::CheckName {
                        good_name: true,
                        name,
                    }.into_message(msg.message_id))
                    .unwrap();
                } else {
                    eprintln!("{name} sucks");
                    tx.send(BackgroundResponse::CheckName {
                        good_name: false,
                        name,
                    }.into_message(msg.message_id))
                    .unwrap();
                }
                lmao += 1;
            }
            BackgroundRequest::Shutdown => {
                eprintln!("BackgroundRequest::Shutdown received");
                tx.send(BackgroundResponse::ShutdownComplete.into_message(msg.message_id)).unwrap();
                break;
            }
        }
        ctx.request_repaint();
    }
}

struct Background {
    to_bg: Sender<BackgroundRequestMessage>,
    from_bg: Receiver<BackgroundResponseMessage>,
    join_handle: Option<JoinHandle<()>>,
}

impl Background {
    fn spawn(ctx: eframe::egui::Context) -> Self {
        let (to_bg_tx, to_bg_rx) = std::sync::mpsc::channel::<BackgroundRequestMessage>();
        let (from_bg_tx, from_bg_rx) = std::sync::mpsc::channel::<BackgroundResponseMessage>();
        let join_handle = Some(std::thread::spawn(move || {
            bg_thread_loop(to_bg_rx, from_bg_tx, ctx);
        }));
        Self {
            to_bg: to_bg_tx,
            from_bg: from_bg_rx,
            join_handle,
        }
    }
}

impl StateA {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        if ui.button("Next").clicked() {
            Action::GoToEnterName
        } else {
            Action::None
        }
    }
}

struct StateB {
    new_name: String,
    busy: bool,
    to_bg: Sender<BackgroundRequestMessage>,
}

impl StateB {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.text_edit_singleline(&mut self.new_name);
            if ui.button("Next").clicked() {
                self.busy = true;
                let msg_id = todo!();
                self.to_bg
                    .send(BackgroundRequest::CheckName {
                        name: self.new_name.clone(),
                    }.into_message(msg_id))
                    .unwrap();
                Action::None
            } else {
                Action::None
            }
        })
        .inner
    }
}

#[derive(Default)]
struct StateC {
    name: String,
}

impl StateC {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        ui.label(&self.name);
        Action::None
    }
}

enum Page {
    A { inner: StateA },
    B { inner: StateB },
    C { inner: StateC },
}

impl Page {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) -> Action {
        match self {
            Page::A { inner } => inner.ui(ui, frame),
            Page::B { inner } => inner.ui(ui, frame),
            Page::C { inner } => inner.ui(ui, frame),
        }
    }

    fn on_background_msg(&mut self, msg: &BackgroundResponse) -> Action {
        todo!("dispatch background message");
        Action::None
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
                    inner: StateB {
                        busy: false,
                        new_name: "".to_owned(),
                        to_bg: self.bg.to_bg.clone(),
                    },
                }
            }
        }
    }

    fn check_for_bg(&mut self) -> Option<BackgroundResponseMessage> {
        match self.bg.from_bg.try_recv() {
            // TODO: hanle any "global" messages here
            Ok(msg) => Some(msg),
            Err(TryRecvError::Empty) => {
                // nothing to do here I guess
                None
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
