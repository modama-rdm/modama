use std::{collections::HashMap, path::PathBuf, sync::Arc};

use axum::{
    extract::{self, State},
    routing::{delete, get, post},
    Json, Router,
};
use modama_common::messages::Share;
use modama_common::requests::CreateShareRequest;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::share_manager::{ShareManager, SmbDropState};

#[derive(Clone)]
pub struct ApiState {
    share_manager: Arc<ShareManager>,
    state_updates_tx: broadcast::Sender<SmbDropState>,
}

impl ApiState {
    pub fn new(
        share_manager: ShareManager,
        state_updates_tx: broadcast::Sender<SmbDropState>,
    ) -> Self {
        Self {
            share_manager: Arc::new(share_manager),
            state_updates_tx,
        }
    }

    async fn send_state_update(&self) {
        self.state_updates_tx
            .send(self.share_manager.get_state().await.unwrap())
            .unwrap();
    }
}

async fn list_shares(State(state): State<ApiState>) -> Json<HashMap<String, Share>> {
    Json(state.share_manager.list_shares().await.unwrap())
}

async fn create_share(
    State(state): State<ApiState>,
    Json(request): Json<CreateShareRequest>,
) -> Json<Share> {
    // NOTE: this is the non-resource-specific API endpoint, so we don't set a
    // resource for the created ingest share.
    let dest_path = PathBuf::from(&request.dest_path);
    let share = state
        .share_manager
        .create_share(&dest_path, request.uid, request.gid, None)
        .await
        .unwrap();
    state.send_state_update().await;
    Json(share)
}

// FIXME: move to modama-common if we decide to keep it
#[derive(Serialize, Deserialize, Debug)]
struct DeleteResponse {
    status: String,
}

async fn delete_share(
    State(state): State<ApiState>,
    extract::Path(share_id): extract::Path<String>,
) -> Json<DeleteResponse> {
    state.share_manager.delete_share(&share_id).await.unwrap();
    state.send_state_update().await;
    // FIXME: return type: can we get rid of that? it's a bit silly...
    Json(DeleteResponse {
        status: "ok".to_owned(),
    })
}

pub async fn serve_smbdrop_api(api_state: ApiState, listen_addr: &str) {
    let app = Router::new()
        .route("/share/", get(list_shares))
        .route("/share/", post(create_share))
        .route("/share/:share_id", delete(delete_share))
        .with_state(api_state);
    let listener = tokio::net::TcpListener::bind(listen_addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

pub async fn serve_smbdrop_privileged_api(api_state: ApiState, listen_addr: &str) {
    todo!("TLS setup; serve");
}
