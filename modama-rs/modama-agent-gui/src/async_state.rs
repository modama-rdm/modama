use std::sync::mpsc::Sender;

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
    Error, // FIXME: payload
}

trait HasRequestId {
    fn request_id(&self) -> RequestId;
}

impl<T: HasRequestId + Clone> AsyncState<T> {
    fn send_request<R>(&mut self, request: R, channel: &Sender<R>) {
        channel.send(request).unwrap();
    }

    fn handle(&mut self, msg: &T) {
        if let Self::Pending { request_id } = &self {
            if *request_id == msg.request_id() {
                *self = Self::Ready { payload: msg.clone() };
            }
        }
    }
}
