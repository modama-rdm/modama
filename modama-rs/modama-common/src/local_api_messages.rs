use serde::{Deserialize, Serialize};

use crate::messages::Share;

/// Local system users can ask the agent for the current foreground share. This
/// doesn't have to contain credentials, most important is some kind of identity
/// and the path where the share is mounted.
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum APICurrentForegroundShare {
    None,
    Share {
        /// The path in the data management that is exported with this Share
        dest_path: String,

        /// Where the share is mounted on the local system
        mount_path: String,
    },
}

/// Response when successfully creating a new ingest share.
#[derive(Serialize, Deserialize, Clone)]
pub struct APICreateShareResponse {
    pub share: Share,
}

/// Response listing projects the current user has access to,
/// with the intent of creating a session or 
#[derive(Serialize, Deserialize, Clone)]
pub struct APIListProjectsResponse {
    pub projects: Vec<String>,
}

/// Response listing existing sessions in the given project
#[derive(Serialize, Deserialize, Clone)]
pub struct APIListSessionsResponse {
    pub project: String,
    pub sessions: Vec<String>,
}

/// Create a new session in the given project
/// (`project_name` is part of the URL)
#[derive(Serialize, Deserialize, Clone)]
pub struct APICreateSessionRequest {
    pub session: String,
}

/// Response when successfully creating a new session
/// in a project
#[derive(Serialize, Deserialize, Clone)]
pub struct APICreateSessionResponse {
    pub project: String,
    pub session: String,
}

// TODO: what info do we need in here?
#[derive(Serialize, Deserialize, Clone)]
pub struct APILoginResponse {
    pub username: String,
}

/// Health check API endpoint response
#[derive(Serialize, Clone)]
#[serde(tag = "type")]
pub enum APIHealthCheckResponse {
    Ok
}
