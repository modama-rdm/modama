use anyhow::Result as AnyResult;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use axum::{
    extract::{self, State},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use hyper::{header, StatusCode};
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

async fn list_shares(State(state): State<ApiState>) -> impl IntoResponse {
    let shares = state.share_manager.list_shares().await;
    match shares {
        Ok(shares) => Json(shares).into_response(),
        Err(e) => {
            error!("could not list shares: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain")],
                "could not list shares: {e}",
            )
                .into_response()
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "status")]
pub enum CreateShareResponse {
    #[serde(rename = "ok")]
    Ok { share: Share },
    #[serde(rename = "error")]
    Error { msg: String },
}

async fn create_share(
    State(state): State<ApiState>,
    Json(request): Json<CreateShareRequest>,
) -> impl IntoResponse {
    let dest_path = PathBuf::from(&request.dest_path);
    info!("creating share from request {request:?}");
    match state
        .share_manager
        .create_share(&dest_path, request.uid, request.gid, request.resource)
        .await
    {
        Ok(share) => {
            state.send_state_update().await;
            info!("created share {share:?}");
            (StatusCode::OK, Json(CreateShareResponse::Ok { share }))
        }
        Err(ShareError::ShareNotFound(share_id)) => {
            error!("share '{share_id}' not found");
            (
                StatusCode::NOT_FOUND,
                Json(CreateShareResponse::Error {
                    msg: format!("share '{share_id}' not found"),
                }),
            )
        }
        Err(err) => {
            error!("internal error when creating share: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CreateShareResponse::Error {
                    msg: format!("error creating share: {err}"),
                }),
            )
        }
    }
}

// FIXME: move to modama-common?
#[derive(Serialize, Deserialize, Debug)]
pub struct DeleteResponse {
    status: String,
}

async fn delete_share(
    State(state): State<ApiState>,
    extract::Path(share_id): extract::Path<String>,
) -> impl IntoResponse {
    match state.share_manager.delete_share(&share_id).await {
        Err(crate::share_manager::ShareError::ShareNotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(DeleteResponse {
                status: "error".to_owned(),
            }),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(DeleteResponse {
                status: "error".to_owned(),
            }),
        ),
        Ok(_) => {
            state.send_state_update().await;
            (
                StatusCode::OK,
                Json(DeleteResponse {
                    status: "ok".to_owned(),
                }),
            )
        }
    }
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
