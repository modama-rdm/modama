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
                info!("sending state update: {:?}", new_state);
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
    use std::{collections::HashMap, ffi::CString, str::FromStr, sync::Arc, time::Duration};

    use itertools::Itertools;
    use modama_common::{
        logging::init_logging, messages::Share, requests::CreateShareRequest, test_helpers::TempDir,
    };
    use reqwest::ClientBuilder;
    use tokio::{sync::broadcast, time::timeout};
    use url::Url;

    use crate::{
        server::{spawn_smbdrop_task, ApiState, CreateShareResponse, ShareAPIErrorResponse},
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
            .get_or_create_user_with_password(
                1001,
                resp_share.share_id.clone(),
                Some(&e0.users.get("1001").unwrap().password),
            )
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

    #[tokio::test]
    async fn test_config_is_written() {
        init_logging();

        let configs_path = TempDir::new("share_manager_configs");
        let dest_path = TempDir::new("dest_path_for_share");
        let bind_mount_prefix = TempDir::new("bind_mount_prefix");

        let (apistate_events_tx, _apistate_events_rx) = broadcast::channel(1024);
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
        let resp_obj: CreateShareResponse = resp.json().await.unwrap();
        assert!(matches!(resp_obj, CreateShareResponse::Ok { share: _ }));

        assert!(configs_path.join("shares_config.json").exists());
        assert!(configs_path.join("smb_dynamic.conf").exists());

        handle.abort();
    }

    #[tokio::test]
    async fn test_list_shares() {
        init_logging();

        let configs_path = TempDir::new("share_manager_configs");
        let dest_path = TempDir::new("dest_path_for_share");
        let bind_mount_prefix = TempDir::new("bind_mount_prefix");

        let (apistate_events_tx, _apistate_events_rx) = broadcast::channel(1024);
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
        let create_resp = client
            .execute(client.post(url).json(&request).build().unwrap())
            .await
            .unwrap();
        let create_resp_obj: CreateShareResponse = create_resp.json().await.unwrap();

        assert!(matches!(
            create_resp_obj,
            CreateShareResponse::Ok { share: _ }
        ));

        let resp_share = match create_resp_obj {
            CreateShareResponse::Ok { share } => {
                let share_clone = share.clone();
                assert_eq!(share.uid, 1001);
                assert_eq!(share.gid, 1001);
                assert_eq!(share.resource.unwrap().as_str(), "holo");
                assert_eq!(share.dest_path, dest_path.to_string_lossy());
                share_clone
            }
        };

        let mut url = api_base_url.clone();
        url.set_path("/share/");
        let resp_list = client
            .execute(client.get(url).build().unwrap())
            .await
            .unwrap();

        let resp_list_obj: HashMap<String, Share> = resp_list.json().await.unwrap();

        assert_eq!(
            resp_list_obj.keys().map(|k| k.to_owned()).collect_vec(),
            vec![resp_share.share_id.clone()],
        );

        assert_eq!(
            resp_list_obj.get(&resp_share.share_id).unwrap().clone(),
            resp_share,
        );

        handle.abort();
    }

    #[tokio::test]
    async fn test_delete_share_error() {
        init_logging();

        let configs_path = TempDir::new("share_manager_configs");
        let _dest_path = TempDir::new("dest_path_for_share");
        let bind_mount_prefix = TempDir::new("bind_mount_prefix");

        let (apistate_events_tx, _apistate_events_rx) = broadcast::channel(1024);
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

        // NOTE: there are no shares at all, and the share configuration file
        // doesn't exist, which should still work (first launch scenario)

        // try to delete a share with a nonexisting share id:
        let mut url = api_base_url.clone();
        url.set_path("/share/nonexisting/");
        let resp_delete = client
            .execute(client.delete(url).build().unwrap())
            .await
            .unwrap();

        assert_eq!(resp_delete.status(), 404);

        let err: ShareAPIErrorResponse = resp_delete.json().await.unwrap();
        println!("{err:?}");

        assert!(matches!(err, ShareAPIErrorResponse::Error { msg: _ }));

        handle.abort();
    }

    #[tokio::test]
    async fn test_delete_share_success() {
        init_logging();

        let configs_path = TempDir::new("share_manager_configs");
        let dest_path = TempDir::new("dest_path_for_share");
        let bind_mount_prefix = TempDir::new("bind_mount_prefix");

        let (apistate_events_tx, _apistate_events_rx) = broadcast::channel(1024);
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
        let create_resp = client
            .execute(client.post(url).json(&request).build().unwrap())
            .await
            .unwrap();
        let create_resp_obj: CreateShareResponse = create_resp.json().await.unwrap();

        match create_resp_obj {
            CreateShareResponse::Ok { share } => {
                // try to delete the share we just created:
                let mut url = api_base_url.clone();
                url.set_path(&format!("/share/{}/", share.share_id));
                let resp_delete = client
                    .execute(client.delete(url).build().unwrap())
                    .await
                    .unwrap();
                assert_eq!(resp_delete.status(), 200);
            }
        }

        handle.abort();
    }

    #[tokio::test]
    async fn test_delete_share_heal_inconsistent_state() {
        init_logging();

        let configs_path = TempDir::new("share_manager_configs");
        let bind_mount_prefix = TempDir::new("bind_mount_prefix");

        let (apistate_events_tx, _apistate_events_rx) = broadcast::channel(1024);
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

        // create a deliberately inconsistent state:
        let share_id = String::from("b4fdcdf6-e870-4ac5-9942-78463f051c5c");
        let username = "shareb4fdcdf6e8704ac5994278463f";

        let share_id_2 = String::from("Y4fdcdf6-e870-4ac5-9942-78463f051c5c");
        let mut state = share_manager.get_state().await.unwrap();
        let share = Share {
            resource: Some("res".to_owned()),
            share_id: share_id.clone(),
            share_name: share_id.clone(),
            dest_path: "/adhoc/the-project/test-session/".to_owned(),
            created_timestamp: 0,
            username: username.to_owned(),
            password: "share-pass".to_owned(),
            // this uid exists, but there's an additional one in the users key of the config:
            uid: 1001,
            gid: 1001,
        };
        state.add_share(share.clone()).await.unwrap();
        state
            .get_or_create_user(1001, share_id.clone())
            .await
            .unwrap();
        // XXX one additional user here:
        state
            .get_or_create_user(1002, share_id_2.clone())
            .await
            .unwrap();

        share_manager
            .with_locked_config(async |config_file| {
                config_file.write_config_no_check(&state).await.unwrap();
            })
            .await
            .unwrap();

        share_manager.start().await.unwrap();

        let api_state = ApiState::new(share_manager, apistate_events_tx);
        let (handle, port) = spawn_smbdrop_task(api_state.clone(), "127.0.0.1:0").await.unwrap();
        let api_base_url = Url::parse(format!("http://127.0.0.1:{port}").as_str()).unwrap();

        let client = ClientBuilder::new().build().unwrap();

        // check that the API can access the share without issues:
        let mut url = api_base_url.clone();
        url.set_path("/share/");
        let resp = client
            .execute(client.get(url).build().unwrap())
            .await
            .unwrap();
        let resp_shares: HashMap<String, Share> = resp.json().await.unwrap();

        assert_eq!(resp_shares.len(), 1);
        let resp_share = resp_shares.values().next().unwrap();
        assert_eq!(resp_share, &share);

        {
            let new_state = api_state.share_manager.get_state().await.unwrap();
            // two users, but one is not used by any share:
            assert_eq!(new_state.users.len(), 2);
        }

        // try to delete the share we just created:
        let mut url = api_base_url.clone();
        url.set_path(&format!("/share/{}/", share.share_id));
        let resp_delete = client
            .execute(client.delete(url).build().unwrap())
            .await
            .unwrap();
        assert_eq!(resp_delete.status(), 200);

        {
            let new_state = api_state.share_manager.get_state().await.unwrap();
            assert_eq!(new_state.users, HashMap::default());
        }

        // check that the API no longer returns that share:
        let mut url = api_base_url.clone();
        url.set_path("/share/");
        let resp = client
            .execute(client.get(url).build().unwrap())
            .await
            .unwrap();
        let resp_shares: HashMap<String, Share> = resp.json().await.unwrap();
        assert_eq!(resp_shares.len(), 0);

        // after all this, there should be no users in the state:
        {
            let new_state = api_state.share_manager.get_state().await.unwrap();
            assert_eq!(new_state.users, HashMap::default());
        }

        handle.abort();
    }

    #[tokio::test]
    async fn test_create_share_heal_inconsistent_state() {
        init_logging();

        let configs_path = TempDir::new("share_manager_configs");
        let dest_path = TempDir::new("dest_path_for_share");
        let bind_mount_prefix = TempDir::new("bind_mount_prefix");

        let (apistate_events_tx, _apistate_events_rx) = broadcast::channel(1024);
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

        // create a deliberately inconsistent state: add an unused user
        let share_id = String::from("b4fdcdf6-e870-4ac5-9942-78463f051c5c");
        let mut state = share_manager.get_state().await.unwrap();
        state
            .get_or_create_user(1002, share_id.clone())
            .await
            .unwrap();
        share_manager
            .with_locked_config(async |config_file| {
                config_file.write_config_no_check(&state).await.unwrap();
            })
            .await
            .unwrap();

        {
            let new_state = share_manager.get_state().await.unwrap();
            // one superfluous user:
            assert_eq!(new_state.users.len(), 1);
            assert_eq!(new_state.shares.len(), 0);
        }

        share_manager.start().await.unwrap();

        let api_state = ApiState::new(share_manager, apistate_events_tx);
        let (handle, port) = spawn_smbdrop_task(api_state.clone(), "127.0.0.1:0").await.unwrap();
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

        let resp_obj: CreateShareResponse = resp.json().await.unwrap();

        let _resp_share = match resp_obj {
            CreateShareResponse::Ok { share } => {
                let share_clone = share.clone();
                assert_eq!(share.uid, 1001);
                assert_eq!(share.gid, 1001);
                assert_eq!(share.resource.unwrap().as_str(), "holo");
                assert_eq!(share.dest_path, dest_path.to_string_lossy());
                share_clone
            }
        };

        {
            let new_state = api_state.share_manager.get_state().await.unwrap();
            // no superfluous user anymore:
            assert_eq!(new_state.users.len(), 1);
            assert_eq!(new_state.shares.len(), 1);
        }

        handle.abort();
    }
}
