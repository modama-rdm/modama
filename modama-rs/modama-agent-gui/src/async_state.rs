#![allow(clippy::unwrap_used)]
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
};

use eframe::egui::Context;

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
    Completed,
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
            AsyncState::Completed => {}
        }
        self
    }

    pub fn take_ready(&mut self) -> Option<T> {
        if let Self::Ready { payload } = self {
            let result = payload.clone();
            *self = Self::Completed;
            Some(result)
        } else {
            None
        }
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending { .. })
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
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
            AsyncState::Completed => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_helper_only_matches_idle_state() {
        let idle = AsyncState::<u8>::Idle;
        let ready = AsyncState::Ready { payload: 1 };
        let completed = AsyncState::<u8>::Completed;

        assert!(idle.is_idle());
        assert!(!ready.is_idle());
        assert!(!completed.is_idle());
    }

    #[test]
    fn taking_ready_payload_marks_request_completed() {
        let mut state = AsyncState::Ready {
            payload: Vec::<u8>::new(),
        };

        assert_eq!(state.take_ready(), Some(Vec::new()));
        assert!(matches!(state, AsyncState::Completed));
    }

    #[test]
    fn fallible_payloads_are_handled_by_the_payload_type() {
        let mut state = AsyncState::Ready {
            payload: Result::<(), &str>::Err("request failed"),
        };

        assert_eq!(state.take_ready(), Some(Err("request failed")));
        assert!(matches!(state, AsyncState::Completed));
    }
}
