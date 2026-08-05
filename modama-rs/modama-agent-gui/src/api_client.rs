#[allow(clippy::all)]
pub mod generated {
    pub mod types {
        include!(concat!(env!("OUT_DIR"), "/generated/types.rs"));
    }

    pub mod client {
        include!(concat!(env!("OUT_DIR"), "/generated/client.rs"));
    }

    pub use client::*;
    pub use types::*;
}

pub use generated::client::HttpClient as GeneratedClient;
pub use generated::types::*;

use base64::{engine::general_purpose::STANDARD, Engine};

pub const API_VERSION: &str = "1.0.0";

/// Typed client for the local modama-agent API.
#[derive(Clone)]
pub struct AgentClient {
    inner: GeneratedClient,
}

impl AgentClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            inner: GeneratedClient::new().with_base_url(base_url),
        }
    }

    pub fn with_basic_auth(&self, username: impl AsRef<str>, password: impl AsRef<str>) -> Self {
        let credentials = format!("{}:{}", username.as_ref(), password.as_ref());
        let authorization = format!("Basic {}", STANDARD.encode(credentials));
        Self {
            inner: self
                .inner
                .clone()
                .with_header("Authorization", authorization),
        }
    }

    pub async fn get_identity(
        &self,
    ) -> Result<
        APIAgentIdentity,
        generated::client::ApiOpError<generated::client::GetIdentityApiError>,
    > {
        self.inner.get_identity(API_VERSION).await
    }

    pub async fn get_foreground_share(
        &self,
    ) -> Result<
        APICurrentForegroundShare,
        generated::client::ApiOpError<generated::client::GetForegroundShareApiError>,
    > {
        self.inner.get_foreground_share(API_VERSION).await
    }

    pub async fn health(
        &self,
    ) -> Result<
        APIHealthCheckResponse,
        generated::client::ApiOpError<generated::client::HealthApiError>,
    > {
        self.inner.health(API_VERSION).await
    }

    pub async fn login(
        &self,
    ) -> Result<APILoginResponse, generated::client::ApiOpError<generated::client::LoginApiError>>
    {
        self.inner.login(API_VERSION).await
    }

    pub async fn list_projects(
        &self,
    ) -> Result<
        APIListProjectsResponse,
        generated::client::ApiOpError<generated::client::ListProjectsApiError>,
    > {
        self.inner.list_projects(API_VERSION).await
    }

    pub async fn list_sessions_in_project(
        &self,
        project_name: impl AsRef<str>,
    ) -> Result<
        APIListSessionsResponse,
        generated::client::ApiOpError<generated::client::ListSessionsInProjectApiError>,
    > {
        self.inner
            .list_sessions_in_project(project_name, API_VERSION)
            .await
    }

    pub async fn create_session_in_project(
        &self,
        project_name: impl AsRef<str>,
        request: APICreateSessionRequest,
    ) -> Result<
        APICreateSessionResponse,
        generated::client::ApiOpError<generated::client::CreateSessionInProjectApiError>,
    > {
        self.inner
            .create_session_in_project(project_name, API_VERSION, request)
            .await
    }

    pub async fn start_ingest(
        &self,
        project_name: impl AsRef<str>,
        session_name: impl AsRef<str>,
    ) -> Result<
        APICreateShareResponse,
        generated::client::ApiOpError<generated::client::StartIngestApiError>,
    > {
        self.inner
            .start_ingest(project_name, session_name, API_VERSION)
            .await
    }

    pub async fn delete_share(
        &self,
        share_id: impl AsRef<str>,
    ) -> Result<(), generated::client::ApiOpError<generated::client::DeleteShareApiError>> {
        self.inner.delete_share(share_id, API_VERSION).await
    }
}

#[cfg(test)]
mod tests {
    use super::AgentClient;

    #[tokio::test]
    async fn login_sends_basic_auth_and_api_version() -> Result<(), Box<dyn std::error::Error>> {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/login/")
            .match_header("authorization", "Basic YWxpY2U6c2VjcmV0")
            .match_header("x-api-version", "1.0.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"username":"alice"}"#)
            .create_async()
            .await;

        let client = AgentClient::new(server.url()).with_basic_auth("alice", "secret");
        let response = client.login().await?;

        assert_eq!(response.username, "alice");
        mock.assert_async().await;
        Ok(())
    }

    #[tokio::test]
    async fn identity_sends_api_version_without_credentials(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/identity/")
            .match_header("x-api-version", "1.0.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"type":"Anonymous","hostname":"workstation"}"#)
            .create_async()
            .await;

        AgentClient::new(server.url()).get_identity().await?;
        mock.assert_async().await;
        Ok(())
    }

    #[tokio::test]
    async fn delete_accepts_204_and_sends_auth_and_version(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("DELETE", "/api/shares/share-123/")
            .match_header("authorization", "Basic YWxpY2U6c2VjcmV0")
            .match_header("x-api-version", "1.0.0")
            .with_status(204)
            .create_async()
            .await;

        AgentClient::new(server.url())
            .with_basic_auth("alice", "secret")
            .delete_share("share-123")
            .await?;
        mock.assert_async().await;
        Ok(())
    }
}
