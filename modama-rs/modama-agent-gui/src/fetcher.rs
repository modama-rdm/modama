//! Supporting code for connecting to the HTTP API of modama-agent
//!
//! How should this work? Definitely in a background thread,
//! messages or commands probably put into a queue, but some
//! status variables can be shared via Arc<Mutex<..>> so we don't
//! need complicated "protocols" between the GUI and the bg thread.
//!
//! But maybe, just maybe, we are rich enough that we can just spawn
//! a bg thread for each interaction with the API (except for websocket
//! events connection, that lives in a "permanent" background thread)
//!
//! NOTE: these are just some thoughts to be filled in with real code
//! step by step, nothing here is final!

use std::thread::JoinHandle;


struct Meh<FetchResult: Clone + Sync + Send + std::fmt::Debug /* + ... */> {
    jh: JoinHandle<FetchResult>
}

// ok so is the JoinHandle sync/send? yes!
fn assert_sync_send<M: Send + Sync>(_m: &M) {
}
fn a(m: &Meh<()>) {
    assert_sync_send(m)
}

use modama_common::local_api_messages::{APICreateSessionRequest, APIListProjectsResponse};
use reqwest::Url;
use serde::{Deserialize, Serialize};

// background services are shaped like this, except htey have a real error type
// and maybe they don't have to take the API type as parameter, instead
// it can take values as they appear in the GUI or the state
// and we convert here
//
// TODO: missing credentials as context
// TODO: timeout logic, when do we give up? should have an error type that we can use in the gui to
// show a "retry/cancel" kinda dialog, but this might be something for 2027 
//
// results might be different for fetching stuff, see below
fn create_session(req: APICreateSessionRequest) -> Result<(), ()> {

    Ok(())
}

// TODO: credentials
fn list_projects() -> Result<APIListProjectsResponse, ()> {
    todo!("implement me");
}

// when we dispatch a background action, the state should be immediately
// updated for the new frame, so we can't spawn more bg tasks at once
//
//

trait BackgroundTask {
    type Error;
    type R: Send;
    type T: Send;

    fn execute(&mut self, request: Self::R) -> JoinHandle<Result<Self::T, Self::Error>>;
}

struct CreateSessionResult {}

struct CreateSessionTask {
    task: Option<JoinHandle<CreateSessionResult>>,
}

impl CreateSessionTask {
    fn new() -> Self {
        CreateSessionTask { task: None }
    }
}

#[derive(Debug, Clone)]
struct CreateSessionRequest {
    project: String,
    session_name: String,
    credentials: crate::state::Credentials,
}

impl BackgroundTask for CreateSessionTask {
    type Error = ();
    type R = CreateSessionResult;
    type T = ();

    fn execute(&mut self, request: Self::R) -> JoinHandle<Result<Self::T, Self::Error>> {
        assert!(self.task.is_none());

        let join_handle = std::thread::spawn(move || {
            let url = Url::parse("http://localhost:8080").unwrap();
            CreateSessionResult {  }
        });

        Ok(join_handle.unwrap())
    }
}
