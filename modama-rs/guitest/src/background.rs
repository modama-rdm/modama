use std::{sync::mpsc::{Receiver, Sender}, thread::JoinHandle, time::Duration};

use crate::async_state::{AsyncRequest, AsyncResponse};

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum BackgroundRequest {
    CheckName { name: String },
    Shutdown,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum BackgroundResponse {
    CheckName { name: String, good_name: bool },
    ShutdownComplete,
}

/// Background work that is closely related to the current
/// foreground work - most of the time this will be kind-of
/// synchronous, meaning the UI will be disabled while the bg
/// thread is processing. There may be exceptions, like autocomplete
/// or search fields where we obviously want to continue the UI to
/// be used while we fetch suggestions.
fn bg_thread_loop(
    rx: Receiver<AsyncRequest<BackgroundRequest>>,
    tx: Sender<AsyncResponse<BackgroundResponse>>,
    ctx: eframe::egui::Context,
) {
    let mut lmao = 0u64;
    loop {
        let msg = rx.recv().unwrap();
        match msg.request_payload {
            BackgroundRequest::CheckName { name } => {
                std::thread::sleep(Duration::from_millis(500));
                if lmao > 3 {
                    eprintln!("{name} is good");
                    tx.send(AsyncResponse {
                        request_id: msg.request_id,
                        result_payload: BackgroundResponse::CheckName {
                            good_name: true,
                            name,
                        },
                    })
                    .unwrap();
                } else {
                    eprintln!("{name} sucks");
                    tx.send(AsyncResponse {
                        request_id: msg.request_id,
                        result_payload: BackgroundResponse::CheckName {
                            good_name: false,
                            name,
                        },
                    })
                    .unwrap();
                }
                lmao += 1;
            }
            BackgroundRequest::Shutdown => {
                eprintln!("BackgroundRequest::Shutdown received");
                tx.send(AsyncResponse {
                    request_id: msg.request_id,
                    result_payload: BackgroundResponse::ShutdownComplete,
                })
                .unwrap();
                break;
            }
        }
        ctx.request_repaint();
    }
}

struct Background {
    to_bg: Sender<AsyncRequest<BackgroundRequest>>,
    from_bg: Receiver<AsyncResponse<BackgroundResponse>>,
    join_handle: Option<JoinHandle<()>>,
}

impl Background {
    fn spawn(ctx: eframe::egui::Context) -> Self {
        let (to_bg_tx, to_bg_rx) = std::sync::mpsc::channel::<AsyncRequest<BackgroundRequest>>();
        let (from_bg_tx, from_bg_rx) =
            std::sync::mpsc::channel::<AsyncResponse<BackgroundResponse>>();
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


