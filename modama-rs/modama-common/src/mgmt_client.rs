use reqwest::{Client, ClientBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
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
        Ok(resp.json().await?)
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
        let body = resp.text().await?;
        serde_json::from_str(&body).map_err(|e| MgmtError::from_serde(e, body))
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
        Ok(resp.json().await?)
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
        let text = resp.text().await?;
        serde_json::from_str(&text).map_err(|e| MgmtError::from_serde(e, text))
    }
}
