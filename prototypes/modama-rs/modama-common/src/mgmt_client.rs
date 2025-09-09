use reqwest::{Client, ClientBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::messages::Share;

#[derive(thiserror::Error, Debug)]
pub enum MgmtError {
    #[error("request error: {0}")]
    RequestError(#[from] reqwest::Error),

    #[error("unathorized")]
    Unauthorized,

    #[error("url parse error: {0}")]
    UrlParseError(#[from] url::ParseError),
}
#[derive(Debug, Deserialize, Serialize)]
pub struct CreateShareRequest {
    pub project: String,
    pub dataset: String,
    pub resource: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
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

pub struct MgmtClient {
    client: Client,
    username: String,
    password: String,
    base_url: Url,
}

impl MgmtClient {
    pub fn new(base_url: &str, username: &str, password: &str) -> Result<Self, MgmtError> {
        let client = ClientBuilder::new().build()?;
        Ok(Self {
            client,
            base_url: Url::parse(base_url)?,
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

        if resp.status() == StatusCode::UNAUTHORIZED {
            return Err(MgmtError::Unauthorized);
        }

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

        if resp.status() == StatusCode::UNAUTHORIZED {
            return Err(MgmtError::Unauthorized);
        }

        Ok(resp.json().await?)
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

        if resp.status() == StatusCode::UNAUTHORIZED {
            return Err(MgmtError::Unauthorized);
        }

        Ok(resp.json().await?)
    }
}
