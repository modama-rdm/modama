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

use std::{
    any::Any,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
};

struct Meh<FetchResult: Clone + Sync + Send + std::fmt::Debug /* + ... */> {
    jh: JoinHandle<FetchResult>,
}

// ok so is the JoinHandle sync/send? yes!
fn assert_sync_send<M: Send + Sync>(_m: &M) {}
fn a(m: &Meh<()>) {
    assert_sync_send(m)
}

use eframe::egui::Context;
use modama_common::local_api_messages::{APICreateSessionRequest, APIListProjectsResponse};
use reqwest::Url;

use crate::state::{ActiveSession, AgentAppState, AgentGuiState};

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
    type Request: Send;
    type Res: Send;

    fn spawn(&mut self, request: Self::Request) -> JoinHandle<Result<Self::Res, Self::Error>>;
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
    type Request = CreateSessionResult;
    type Res = ();

    fn spawn(&mut self, request: Self::Request) -> JoinHandle<Result<Self::Res, Self::Error>> {
        assert!(self.task.is_none());

        let join_handle = std::thread::spawn(move || {
            let url = Url::parse("http://localhost:8080").unwrap();
            todo!();
            //Ok(CreateSessionResult {})
        });

        todo!();
        //Ok(join_handle)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SpawnError<E>
where
    E: std::error::Error + Sync + Send,
{
    #[error("operation was cancelled")]
    Cancelled,

    #[error("still running")]
    Running,

    #[error("no longer or not yet running")]
    NotRunning,

    #[error("join error")]
    Join(Box<dyn Any + Send + 'static>),

    #[error("inner: {0}")]
    Inner(E),
}

// FIXME: maybe another feasible design is a helper function that combines
// spawning a background job, and updating the state. so two closures:
struct SpawnAndUpdate<U, E>
where
    U: Sync + Send + 'static,
    E: std::error::Error + Sync + Send + 'static,
{
    app_state: Arc<Mutex<AgentAppState>>,
    cancelled: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<Result<U, SpawnError<E>>>>,
    context: Context,
}

impl<U, E> SpawnAndUpdate<U, E>
where
    U: Sync + Send,
    E: std::error::Error + Sync + Send,
{
    pub fn new(app_state: &Arc<Mutex<AgentAppState>>, context: Context) -> Self {
        Self {
            app_state: Arc::clone(app_state),
            cancelled: Arc::new(AtomicBool::new(false)),
            join_handle: None,
            context,
        }
    }

    pub fn spawn_and_update<T>(
        &mut self,
        job: impl Fn() -> T + Send + 'static,
        mut update: impl FnMut(T, &mut AgentAppState) -> Result<U, E> + Send + 'static,
    ) where
        T: Sync + Send,
    {
        // 1) spawn the job
        // 2) when the job is done, acquire mutexes for app and gui states
        //    and call the update function to apply the changes
        // 3) after updating the state, request a repaint
        let state_inner = Arc::clone(&self.app_state);
        let cancel = Arc::clone(&self.cancelled);
        let context = self.context.clone();
        self.join_handle = Some(std::thread::spawn(move || {
            let res = job();
            let update_res = if !cancel.load(Ordering::Relaxed) {
                let mut app_state = state_inner.lock().unwrap();
                update(res, &mut app_state).map_err(SpawnError::Inner)
            } else {
                Err(SpawnError::Cancelled)
            };
            context.request_repaint();
            update_res
        }));
    }

    pub fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn try_join(&mut self) -> Result<U, SpawnError<E>> {
        match self.join_handle.take() {
            Some(handle) => {
                if handle.is_finished() {
                    handle.join().map_err(|e| SpawnError::Join(e))?
                } else {
                    Err(SpawnError::Running)
                }
            }
            None => Err(SpawnError::NotRunning),
        }
    }

    pub fn is_finished(&self) -> bool {
        if let Some(j) = &self.join_handle {
            j.is_finished()
        } else {
            true
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExampleError {}

fn example_gui(app_state: Arc<Mutex<AgentAppState>>, context: Context) {
    let mut s = SpawnAndUpdate::new(&app_state, context);
    s.spawn_and_update(
        || {
            // TODO: do the real thing
            // our example here is activating a session
            let a_s = ActiveSession {
                session: "testsession".to_owned(),
                project: "testproject".to_owned(),
            };
            Ok::<_, ExampleError>(a_s)
        },
        |res, app_state| {
            app_state.session = Some(res?);
            Ok::<_, ExampleError>(())
        },
    );

    // UI "blocking" should be done directly on the state of the thread:
    let _button_is_disabled = !s.is_finished();
}

// TODO: question: what are the concurrency limits? How do we enforce them?
// I think there are two categories of background work:
//
// 1) General information / state update work. this can run in parallel to other work without
//    issues, and there's no problem if it fails (it can communicate "i'm offline / can't reach
//    modama", but doesn't have to block the user's actions otherwise). This can be either triggered
//    by a state change of the UI, or by periodic work ("update list of this user's projects /
//    sessions in their projects")
//
// 2) Fetches directly related to foreground activity of the GUI. These should _not_ run
//    concurrently with other background jobs of this category, or only in a controlled way.
//    For example, creating a session should wait for the request to finish until allowing
//    the next background task (for example creating an ingest share for that session).
