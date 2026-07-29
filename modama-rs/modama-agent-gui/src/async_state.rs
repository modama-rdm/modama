#![allow(clippy::unwrap_used)]
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
};

use eframe::egui::Context;

pub type RequestId = u64;

#[derive(Debug, Default)]
pub enum AsyncState<T> {
    #[default]
    Idle,
    Pending {
        handle: Option<JoinHandle<T>>,
        cancelled: Arc<AtomicBool>,
    },
    Ready {
        payload: T,
    },
    Error,
}

impl<T: Clone + Send + 'static> AsyncState<T> {
    /// Spawn `task` as a background thread.
    ///
    /// Once the task has run, a redraw will be requested.
    pub fn spawn<F>(&mut self, task: F, ctx: Context)
    where
        F: FnOnce() -> T + Send + 'static,
    {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_in_thread = Arc::clone(&cancelled);
        let handle = std::thread::spawn(move || {
            let res = task();
            if !cancelled_in_thread.load(Ordering::Relaxed) {
                ctx.request_repaint();
            }
            res
        });
        *self = Self::Pending {
            handle: Some(handle),
            cancelled,
        };
    }

    pub fn poll(&mut self) -> &mut Self {
        match self {
            AsyncState::Idle => {}
            AsyncState::Pending {
                handle,
                cancelled: _,
            } => {
                if let Some(inner_handle) = handle.take() {
                    if inner_handle.is_finished() {
                        let res = inner_handle.join().unwrap();
                        *self = Self::Ready { payload: res };
                    } else {
                        *handle = Some(inner_handle);
                    }
                } else {
                    todo!("error handling");
                }
            }
            AsyncState::Ready { payload: _ } => {}
            AsyncState::Error => todo!("error handling"),
        }
        self
    }

    pub fn take_ready(&mut self) -> Option<T> {
        if let Self::Ready { payload } = self {
            let result = payload.clone();
            *self = Self::Idle;
            Some(result)
        } else {
            None
        }
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending { .. })
    }
}

impl<T> Drop for AsyncState<T> {
    fn drop(&mut self) {
        match self {
            AsyncState::Idle => {}
            AsyncState::Pending {
                handle: _,
                cancelled,
            } => {
                cancelled.store(true, Ordering::Relaxed);
            }
            AsyncState::Ready { payload: _ } => {}
            AsyncState::Error => {}
        }
    }
}
