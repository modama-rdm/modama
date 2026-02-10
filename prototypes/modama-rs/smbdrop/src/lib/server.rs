use anyhow::Result as AnyResult;
use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::{self, State},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use hyper::StatusCode;
use log::{error, info};
use modama_common::messages::Share;
use modama_common::requests::CreateShareRequest;
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, sync::broadcast, task::JoinHandle};

use crate::{
    mount::MountBackend,
    process_manager::ProcessManager,
    share_manager::{ShareError, ShareManager, SmbDropState, UserProvider},
};

#[derive(Clone)]
pub struct ApiState<U: UserProvider, M: MountBackend + 'static, P: ProcessManager> {
    share_manager: Arc<ShareManager<U, M, P>>,
    state_updates_tx: broadcast::Sender<SmbDropState>,
}

impl<U: UserProvider, M: MountBackend, P: ProcessManager> ApiState<U, M, P> {
    pub fn new(
        share_manager: ShareManager<U, M, P>,
        state_updates_tx: broadcast::Sender<SmbDropState>,
    ) -> Self {
        Self {
            share_manager: Arc::new(share_manager),
            state_updates_tx,
        }
    }

    async fn send_state_update(&self) {
        match self.share_manager.get_state().await {
            Ok(new_state) => {
                info!("sending state update: {:?}", &new_state);
                if self.state_updates_tx.send(new_state).is_err() {
                    error!("tried to send state update but no one cared...");
                };
            }
            Err(e) => {
                error!("could not get share manager state (maybe corrupt config?): {e}")
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(tag = "status")]
pub enum CreateShareResponse {
    #[serde(rename = "ok")]
    Ok { share: Share },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "status")]
pub enum ShareAPIErrorResponse {
    #[serde(rename = "error")]
    Error { msg: String },
}

#[derive(Debug, thiserror::Error)]
enum ShareAPIError {
    #[error("share manager error: {0}")]
    Manager(#[from] ShareError),
}

impl IntoResponse for ShareAPIError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Manager(ShareError::ShareNotFound(share_id)) => {
                error!("share '{share_id}' not found");
                (
                    StatusCode::NOT_FOUND,
                    Json(ShareAPIErrorResponse::Error {
                        msg: format!("share '{share_id}' not found"),
                    }),
                )
                    .into_response()
            }
            Self::Manager(err) => {
                error!("internal error when creating share: {err}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ShareAPIErrorResponse::Error {
                        msg: err.to_string(),
                    }),
                )
                    .into_response()
            }
        }
    }
}

async fn list_shares<U: UserProvider, M: MountBackend, P: ProcessManager>(
    State(state): State<ApiState<U, M, P>>,
) -> Result<impl IntoResponse, ShareAPIError> {
    let shares = state.share_manager.list_shares().await?;
    Ok(Json(shares).into_response())
}

async fn create_share<U: UserProvider, M: MountBackend, P: ProcessManager>(
    State(state): State<ApiState<U, M, P>>,
    Json(request): Json<CreateShareRequest>,
) -> Result<impl IntoResponse, ShareAPIError> {
    let dest_path = PathBuf::from(&request.dest_path);
    info!("creating share from request {request:?}");
    let share = state
        .share_manager
        .create_share(&dest_path, request.uid, request.gid, request.resource)
        .await?;
    state.send_state_update().await;
    info!("created share {share:?}");
    Ok((StatusCode::OK, Json(CreateShareResponse::Ok { share })).into_response())
}

// FIXME: move to modama-common?
#[derive(Serialize, Deserialize, Debug)]
pub struct DeleteResponse {
    status: String,
}

async fn delete_share<U: UserProvider, M: MountBackend, P: ProcessManager>(
    State(state): State<ApiState<U, M, P>>,
    extract::Path(share_id): extract::Path<String>,
) -> Result<impl IntoResponse, ShareAPIError> {
    state.share_manager.delete_share(&share_id).await?;
    state.send_state_update().await;
    Ok((
        StatusCode::OK,
        Json(DeleteResponse {
            status: "ok".to_owned(),
        }),
    )
        .into_response())
}

pub async fn serve_smbdrop_api<
    U: UserProvider,
    M: MountBackend + 'static + Clone,
    P: ProcessManager + 'static,
>(
    api_state: ApiState<U, M, P>,
    listener: TcpListener,
) -> AnyResult<()> {
    let app = Router::new()
        .route("/share/", get(list_shares))
        .route("/share/", post(create_share))
        .route("/share/{share_id}/", delete(delete_share))
        .with_state(api_state);
    axum::serve(listener, app).await?;

    Ok(())
}

pub async fn spawn_smbdrop_task<
    U: UserProvider,
    M: MountBackend + Clone,
    P: ProcessManager + 'static,
>(
    api_state: ApiState<U, M, P>,
    listen_addr: &str,
) -> AnyResult<(JoinHandle<AnyResult<()>>, u16)> {
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    let port = listener.local_addr()?.port();
    let handle = tokio::spawn(async move { serve_smbdrop_api(api_state, listener).await });

    Ok((handle, port))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::{
        ffi::{CStr, CString},
        str::FromStr,
        sync::Arc,
        time::Duration,
    };

    use log::error;
    use modama_common::{
        logging::init_logging, messages::Share, requests::CreateShareRequest, test_helpers::TempDir,
    };
    use reqwest::ClientBuilder;
    use serde_json::Value;
    use tokio::{sync::broadcast, time::timeout};
    use url::Url;

    use crate::{
        process_manager::ProcessManager,
        server::{spawn_smbdrop_task, ApiState, CreateShareResponse},
        share_manager::{ShareManager, SmbDropState, User, UserProvider},
        test_helpers::{MockMountBackend, MockProcessManager, MockUserProvider},
    };

    #[tokio::test]
    async fn test_mock_user_provider() {
        // simple tests to make sure our mock works correctly
        let user_provider = MockUserProvider::new(vec![("testuser".to_owned(), 1001)]);

        user_provider
            .add_system_user(&User {
                uid: 1001,
                username: "testuser".to_owned(),
                password: "testpw".to_owned(),
            })
            .await
            .unwrap();
        assert!(user_provider.passwd_from_uid(1001).is_ok());

        let name: CString = CString::from_str("testuser").unwrap();
        assert!(user_provider.passwd_from_name(name.as_c_str()).is_ok());
    }

    #[tokio::test]
    async fn test_create_share() {
        init_logging();

        let configs_path = TempDir::new("share_manager_configs");
        let dest_path = TempDir::new("dest_path_for_share");
        let bind_mount_prefix = TempDir::new("bind_mount_prefix");

        let (apistate_events_tx, mut apistate_events_rx) = broadcast::channel(1024);
        let user_provider = MockUserProvider::new(vec![]);
        // let user_provider = MockUserProvider::new(vec![("testuser".to_owned(), 1001)]);
        let mount_backend: Arc<MockMountBackend> = Arc::new(MockMountBackend::new());
        let process_manager: MockProcessManager = MockProcessManager::new();
        let mut share_manager = ShareManager::new(
            bind_mount_prefix.to_owned(),
            configs_path.join("shares_config.json"),
            configs_path.join("smb_dynamic.conf"),
            user_provider,
            mount_backend,
            process_manager.clone(),
        )
        .await;

        share_manager.start().await.unwrap();

        let api_state = ApiState::new(share_manager, apistate_events_tx);
        let (handle, port) = spawn_smbdrop_task(api_state, "127.0.0.1:0").await.unwrap();
        let api_base_url = Url::parse(format!("http://127.0.0.1:{port}").as_str()).unwrap();

        let client = ClientBuilder::new().build().unwrap();

        let request = CreateShareRequest {
            dest_path: dest_path.to_str().unwrap().to_owned(),
            resource: Some("holo".to_owned()),
            uid: 1001,
            gid: 1001,
        };

        let mut url = api_base_url.clone();
        url.set_path("/share/");
        let resp = client
            .execute(client.post(url).json(&request).build().unwrap())
            .await
            .unwrap();

        let pm_cmd = timeout(
            Duration::from_millis(100),
            process_manager
                .q
                .as_ref()
                .lock()
                .await
                .as_mut()
                .unwrap()
                .recv(),
        )
        .await
        .unwrap();
        assert_eq!(
            pm_cmd,
            Some(crate::process_manager::ProcessManagerCommand::Reload)
        );

        // let resp_json: Value = resp.json().await.unwrap();
        // error!("response: {resp_json}");

        let resp_obj: CreateShareResponse = resp.json().await.unwrap();

        assert!(matches!(resp_obj, CreateShareResponse::Ok { share: _ }));

        let resp_share = match resp_obj {
            CreateShareResponse::Ok { share } => {
                let share_clone = share.clone();
                assert_eq!(share.uid, 1001);
                assert_eq!(share.gid, 1001);
                assert_eq!(share.resource.unwrap().as_str(), "holo");
                assert_eq!(share.dest_path, dest_path.to_string_lossy());
                share_clone
            }
        };

        let mut events: Vec<SmbDropState> = Vec::new();

        while let Ok(state) = timeout(Duration::from_millis(100), apistate_events_rx.recv()).await {
            events.push(state.unwrap());
        }

        assert_eq!(events.len(), 1);

        let e0 = events.first().unwrap();

        let mut expected_event = SmbDropState::empty();
        expected_event.add_share(resp_share.clone()).await.unwrap();
        expected_event
            .get_or_create_user_with_password(1001, resp_share.share_id.clone(), Some(
                &e0.users.get("1001").unwrap().password,
            ))
            .await
            .unwrap();

        println!("{e0:?}");
        println!("{expected_event:?}");

        assert_eq!(
            e0.users.get("1001").unwrap().password,
            expected_event.users.get("1001").unwrap().password
        );

        assert_eq!(e0, &expected_event);

        handle.abort();
    }
}
