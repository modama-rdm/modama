use reqwest::{Client, ClientBuilder, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::{messages::Share, version::modama_git_version};

#[derive(thiserror::Error, Debug)]
pub enum MgmtError {
    #[error("request error: {0}")]
    RequestError(#[from] reqwest::Error),

    #[error("deserialize: {source} body={body}")]
    Deserialize {
        source: serde_json::Error,
        body: String,
    },

    #[error("unathorized")]
    Unauthorized,

    #[error("object not found")]
    NotFound,

    #[error("error {errno}: {message}")]
    Backend { errno: i64, message: String },

    #[error("response format error: {0}")]
    Formatting(String),

    #[error("url parse error: {0}")]
    UrlParseError(#[from] url::ParseError),
}

impl MgmtError {
    fn from_serde(value: serde_json::Error, body: String) -> Self {
        Self::Deserialize {
            source: value,
            body,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateShareRequest {
    pub project: String,
    pub dataset: String,
    pub resource: Option<String>,
}

/// Corresponds to CREATE_DATASET_REQUEST_SCHEMA
/// in the permissions API
#[derive(Debug, Deserialize, Serialize)]
pub struct CreateDatasetRequest {
    pub name: String,
    pub owner: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AdhocProject {
    pub relpath: String,
    pub name: String,
    pub owner: String,
    pub members: Vec<String>,
    pub ctime: f64,
    pub mtime: f64,
    pub gid: u32,
    pub usage_bytes: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ListProjectsResponse {
    pub errno: u64,
    pub projects: Vec<AdhocProject>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DataInProject {
    pub name: String,
    pub immutable: bool,
    pub ctime: f64,
    pub mtime: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProjectDetailsResponse {
    pub errno: u64,
    pub project: AdhocProject,
    pub datasets: Vec<DataInProject>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateDatasetResponse {
    pub errno: u64,
    pub message: Option<String>,
}

pub struct MgmtClient {
    client: Client,
    username: String,
    password: String,
    base_url: Url,
}

fn maybe_status_to_err(resp: &Response) -> Result<(), MgmtError> {
    if resp.status() == StatusCode::UNAUTHORIZED {
        return Err(MgmtError::Unauthorized);
    }

    if resp.status() == StatusCode::NOT_FOUND {
        return Err(MgmtError::NotFound);
    }

    Ok(())
}

fn get_errno_msg(value: &Value) -> Result<(i64, Option<String>), MgmtError> {
    let map = value
        .as_object()
        .ok_or_else(|| MgmtError::Formatting("response json must be an object".to_owned()))?;

    let errno_num = if let Some(num) = map.get("errno") {
        num
    } else {
        // we don't have an errno, we must assume all is good
        // if the status code is also good.
        return Ok((0, None));
    };

    let errno = errno_num
        .as_i64()
        .ok_or_else(|| MgmtError::Formatting("errno must fit into i64".to_owned()))?;

    let message = if errno != 0 {
        let msg_value = map
            .get("message")
            .ok_or_else(|| MgmtError::Formatting("response must have a message key".to_owned()))?;

        Some(
            msg_value
                .as_str()
                .ok_or_else(|| MgmtError::Formatting("message must be a string".to_owned()))?
                .to_owned(),
        )
    } else {
        None
    };

    Ok((errno, message))
}

async fn parse_to_response<T: DeserializeOwned>(resp: Response) -> Result<T, MgmtError> {
    let body = resp.text().await?;
    let value: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| MgmtError::from_serde(e, body.clone()))?;
    let (errno, message) = get_errno_msg(&value)?;
    if let Some(msg) = message {
        return Err(MgmtError::Backend {
            errno,
            message: msg,
        });
    }
    serde_json::from_str(&body).map_err(|e| MgmtError::from_serde(e, body.clone()))
}

impl MgmtClient {
    pub fn new(base_url: &Url, username: &str, password: &str) -> Result<Self, MgmtError> {
        let client = ClientBuilder::new()
            .user_agent(format!("modama-agent/{}", modama_git_version()))
            .build()?;
        Ok(Self {
            client,
            base_url: base_url.to_owned(),
            username: username.to_owned(),
            password: password.to_owned(),
        })
    }

    pub async fn list_projects(&self) -> Result<ListProjectsResponse, MgmtError> {
        let full_url = self.base_url.join("/api/project/")?;
        let resp = self
            .client
            .get(full_url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await?;
        maybe_status_to_err(&resp)?;
        parse_to_response(resp).await
    }

    pub async fn project_details(
        &self,
        project: &str,
    ) -> Result<ProjectDetailsResponse, MgmtError> {
        let full_url = self.base_url.join(&format!("/api/project/{project}/"))?;
        let resp = self
            .client
            .get(full_url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await?;
        maybe_status_to_err(&resp)?;
        parse_to_response(resp).await
    }

    pub async fn create_share(
        &self,
        project: &str,
        dataset: &str,
        resource: &str,
        agent_token: &str,
    ) -> Result<Share, MgmtError> {
        let full_url = self.base_url.join("/api/share/")?;
        let request = CreateShareRequest {
            project: project.to_owned(),
            dataset: dataset.to_owned(),
            resource: Some(resource.to_owned()),
        };

        let resp = self
            .client
            .post(full_url)
            .basic_auth(&self.username, Some(&self.password))
            .header("X-Agent-Token", agent_token)
            .json(&request)
            .send()
            .await?;
        maybe_status_to_err(&resp)?;
        parse_to_response(resp).await
    }

    pub async fn create_dataset(
        &self,
        project: &str,
        dataset: &str,
        owner: &str,
    ) -> Result<CreateDatasetResponse, MgmtError> {
        let full_url = self
            .base_url
            .join(&format!("/api/project/{project}/dataset/"))?;
        let request = CreateDatasetRequest {
            name: dataset.to_owned(),
            owner: owner.to_owned(),
        };

        let resp = self
            .client
            .post(full_url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&request)
            .send()
            .await?;
        maybe_status_to_err(&resp)?;
        parse_to_response(resp).await
    }
}
