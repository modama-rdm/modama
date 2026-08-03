use std::{
    any::Any,
    error::Error,
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
};

use eframe::egui::Context;

pub struct TaskPanic(Box<dyn Any + Send + 'static>);

impl fmt::Debug for TaskPanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskPanic")
            .field("payload_type", &(*self.0).type_id())
            .finish()
    }
}

impl fmt::Display for TaskPanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("background task panicked")
    }
}

impl Error for TaskPanic {}

#[derive(Debug, thiserror::Error)]
pub enum PollError {
    #[error("background task panicked")]
    TaskPanicked(#[source] TaskPanic),
    #[error("pending task has no join handle")]
    MissingHandle,
}

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

    pub fn poll(&mut self) -> Result<&mut Self, PollError> {
        match self {
            AsyncState::Idle => {}
            AsyncState::Pending {
                handle,
                cancelled: _,
            } => {
                if let Some(inner_handle) = handle.take() {
                    if inner_handle.is_finished() {
                        match inner_handle.join() {
                            Ok(res) => *self = Self::Ready { payload: res },
                            Err(payload) => {
                                *self = Self::Completed;
                                return Err(PollError::TaskPanicked(TaskPanic(payload)));
                            }
                        }
                    } else {
                        *handle = Some(inner_handle);
                    }
                } else {
                    *self = Self::Completed;
                    return Err(PollError::MissingHandle);
                }
            }
            AsyncState::Ready { payload: _ } => {}
            AsyncState::Completed => {}
        }
        Ok(self)
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

    pub fn reset(&mut self) {
        *self = Self::Idle;
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
    fn panicking_tasks_return_an_error_and_become_retryable() {
        let mut state = AsyncState::<()>::Pending {
            handle: Some(std::thread::spawn(|| panic!("test panic"))),
            cancelled: Arc::new(AtomicBool::new(false)),
        };

        let error = loop {
            match state.poll() {
                Ok(_) => std::thread::yield_now(),
                Err(error) => break error,
            }
        };
        assert!(matches!(error, PollError::TaskPanicked(_)));
        assert!(!state.is_pending());

        state.reset();
        assert!(state.is_idle());
    }

    #[test]
    fn missing_handles_return_an_error_and_become_terminal() {
        let mut state = AsyncState::<()>::Pending {
            handle: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        };

        assert!(matches!(state.poll(), Err(PollError::MissingHandle)));
        assert!(matches!(state, AsyncState::Completed));
    }

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
