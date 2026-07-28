#![allow(clippy::unwrap_used)]
// TODO: error handling, remove the allow
use std::thread::JoinHandle;

use eframe::egui::Context;

pub type RequestId = u64;

#[derive(Debug, Default)]
pub enum AsyncState<T> {
    #[default]
    Idle,
    Pending {
        handle: Option<JoinHandle<T>>,
    },
    Ready {
        payload: T,
    },
    Error, // FIXME: payload; generic E maybe?
}

impl<T: Clone + Send + 'static> AsyncState<T> {
    /// Spawn `task` as a background  and send it to the bg thread
    pub fn spawn<F>(&mut self, task: F, ctx: Context)
    where
        F: FnOnce() -> T + Send + 'static,
    {
        let handle = std::thread::spawn(move || {
            let res = task();
            ctx.request_repaint();
            res
        });
        *self = Self::Pending {
            handle: Some(handle),
        };
    }

    pub fn poll(&mut self) -> &mut Self {
        match self {
            AsyncState::Idle => {}
            AsyncState::Pending { handle } => {
                if let Some(inner_handle) = handle.take() {
                    if inner_handle.is_finished() {
                        let res = inner_handle.join().unwrap();
                        *self = Self::Ready { payload: res };
                    } else {
                        *handle = Some(inner_handle)
                    }
                } else {
                    todo!("error handling");
                }
            }
            AsyncState::Ready { payload: _ } => {
                // already Ready, no need to check
            }
            AsyncState::Error => todo!("error handling"),
        }
        self
    }
}
