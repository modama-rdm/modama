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
use tokio::sync::broadcast;

use crate::share_manager::{ShareError, ShareManager, SmbDropState};

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

#[derive(Serialize, Deserialize, Debug)]
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
                        msg: format!("error creating share: {err}"),
                    }),
                )
                    .into_response()
            }
        }
    }
}

async fn list_shares(State(state): State<ApiState>) -> Result<impl IntoResponse, ShareAPIError> {
    let shares = state.share_manager.list_shares().await?;
    Ok(Json(shares).into_response())
}

async fn create_share(
    State(state): State<ApiState>,
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

async fn delete_share(
    State(state): State<ApiState>,
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

pub async fn serve_smbdrop_api(api_state: ApiState, listen_addr: &str) -> AnyResult<()> {
    let app = Router::new()
        .route("/share/", get(list_shares))
        .route("/share/", post(create_share))
        .route("/share/:share_id/", delete(delete_share))
        .with_state(api_state);
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
