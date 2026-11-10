use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The identity configured for this local agent.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, ToSchema)]
#[serde(tag = "type")]
pub enum APIAgentIdentity {
    Anonymous {
        hostname: String,
    },
    AgentId {
        agent_name: String,
        resource_name: String,
    },
}

/// Local system users can ask the agent for the current foreground share. This
/// doesn't have to contain credentials, most important is some kind of identity
/// and the path where the share is mounted.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, ToSchema)]
#[serde(tag = "type")]
pub enum APICurrentForegroundShare {
    /// No foreground share is desired.
    None,

    /// A foreground share is desired, but it can't be used in the caller's login session (yet).
    Unavailable {
        /// Stable identifier used to close this share.
        share_id: String,

        /// Project derived from the share destination.
        project: String,

        /// Session derived from the share destination.
        session: String,

        /// The path in the data management that is exported with this Share.
        dest_path: String,

        /// Why the share is unavailable, written for the person running the client.
        message: String,

        /// Login name of the user who started the share; null for shares from an smbdrop
        /// that didn't record it yet
        created_by: Option<String>,
    },

    /// The foreground share is mounted; write here.
    Mounted {
        /// Stable identifier used to close this share.
        share_id: String,

        /// Project derived from the share destination.
        project: String,

        /// Session derived from the share destination.
        session: String,

        /// The path in the data management that is exported with this Share
        dest_path: String,

        /// Where the share is mounted on the local system
        mount_path: String,

        /// Login name of the user who started the share; null for shares from an smbdrop
        /// that didn't record it yet
        created_by: Option<String>,
    },
}

/// Response when successfully creating a new ingest share.
#[derive(Serialize, Deserialize, Clone, ToSchema)]
pub struct APICreateShareResponse {
    pub share_id: String,
    pub project: String,
    pub session: String,
    pub dest_path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, ToSchema)]
pub struct APIProjectSummary {
    pub name: String,
    pub owner: String,
    pub members: Vec<String>,
    /// Last modification as an integer Unix timestamp.
    pub modified_at: i64,
}

/// Response listing projects the current user has access to,
/// with the intent of creating a session or
#[derive(Serialize, Deserialize, Clone, ToSchema)]
pub struct APIListProjectsResponse {
    pub projects: Vec<APIProjectSummary>,
}

/// Response listing existing sessions in the given project
#[derive(Serialize, Deserialize, Clone, ToSchema)]
pub struct APIListSessionsResponse {
    pub project: String,
    pub sessions: Vec<String>,
    /// The same sessions with their last modification. Missing in answers of older agents.
    #[serde(default)]
    pub session_summaries: Vec<APISessionSummary>,
}

/// A session in a project
#[derive(Serialize, Deserialize, Clone, ToSchema)]
pub struct APISessionSummary {
    pub name: String,
    /// Last modification of the session folder as an integer Unix timestamp.
    pub modified_at: i64,
}

/// Create a new session in the given project
/// (`project_name` is part of the URL)
#[derive(Serialize, Deserialize, Clone, ToSchema)]
pub struct APICreateSessionRequest {
    pub session: String,
}

/// Response when successfully creating a new session
/// in a project
#[derive(Serialize, Deserialize, Clone, ToSchema)]
pub struct APICreateSessionResponse {
    pub project: String,
    pub session: String,
}

/// Sign in with the management interface's username and password
#[derive(Serialize, Deserialize, Clone, ToSchema)]
pub struct APILoginRequest {
    pub username: String,
    pub password: String,
}

/// Answer to a successful sign-in; the session itself travels in the `modama_agent_session`
/// cookie
#[derive(Serialize, Deserialize, Clone, ToSchema)]
pub struct APILoginResponse {
    pub username: String,
}

/// Health check API endpoint response
#[derive(Serialize, Clone, ToSchema)]
#[serde(tag = "type")]
pub enum APIHealthCheckResponse {
    Ok,
    NotAvailable,
}
