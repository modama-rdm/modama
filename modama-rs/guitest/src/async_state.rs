#![allow(clippy::unwrap_used)]
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::Sender,
};

pub type RequestId = u64;

#[derive(Debug, Default)]
pub enum AsyncState<T> {
    #[default]
    Idle,
    Pending {
        request_id: RequestId,
    },
    Ready {
        payload: T,
    },
    Error, // FIXME: payload; generic E maybe?
}

#[derive(Debug, Clone)]
pub struct AsyncRequest<R>
where
    R: Clone,
{
    pub request_id: RequestId,
    pub request_payload: R,
}

#[derive(Debug, Clone)]
pub struct AsyncResponse<T>
where
    T: Clone,
{
    pub request_id: RequestId,
    pub result_payload: T,
}

static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_request_id() -> RequestId {
    REQUEST_ID_COUNTER.fetch_add(1, Ordering::SeqCst) + 1
}

impl<T: Clone> AsyncState<T> {
    /// Package the request `R` into a `AsyncRequest` and send it to the bg thread
    pub fn send_request<R: Clone>(&mut self, request: R, channel: &Sender<AsyncRequest<R>>) {
        let msg = AsyncRequest {
            request_id: next_request_id(),
            request_payload: request,
        };
        let id = msg.request_id;
        channel.send(msg).unwrap();
        *self = Self::Pending { request_id: id };
    }

    /// Handle a response and check if it matches our request id,
    /// then modify ourselves to include the payload
    pub fn handle(&mut self, msg: &AsyncResponse<T>) {
        if let Self::Pending { request_id } = &self {
            if *request_id == msg.request_id {
                *self = Self::Ready {
                    payload: msg.result_payload.clone(),
                };
            }
        }
    }
}
