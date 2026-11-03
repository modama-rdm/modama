#[allow(clippy::all)]
pub mod generated {
    pub mod types {
        include!(concat!(env!("OUT_DIR"), "/generated/types.rs"));
    }

    pub mod client {
        include!(concat!(env!("OUT_DIR"), "/generated/client.rs"));
    }

    pub mod info {
        include!(concat!(env!("OUT_DIR"), "/generated/info.rs"));
    }

    pub use client::*;
    pub use info::*;
    pub use types::*;
}

pub use generated::client::HttpClient as GeneratedClient;
pub use generated::types::*;

use std::sync::Arc;

use reqwest::cookie::Jar;

pub use generated::info::API_VERSION;

/// Typed client for the local modama-agent API. It keeps the agent's session cookie in memory,
/// like a browser does; a new client starts signed out.
#[derive(Clone)]
pub struct AgentClient {
    inner: GeneratedClient,
}

// The wrappers pass the generated client's `ApiOpError` through unchanged; it's large because it
// carries the response, and boxing it would only add a layer around every call site.
#[allow(clippy::result_large_err)]
impl AgentClient {
    /// `base_url` may end in `/` (as `Url::as_str()` always does): the generated client appends
    /// paths that start with `/`, so trailing slashes are dropped here.
    pub fn new(base_url: impl Into<String>) -> Self {
        modama_common::install_rustls_crypto_provider();
        let inner = match reqwest::Client::builder()
            .cookie_provider(Arc::new(Jar::default()))
            .build()
        {
            Ok(client) => GeneratedClient::with_client(client),
            Err(error) => {
                // without cookies, every authenticated call ends up at the login page again
                log::error!("could not set up the agent client with cookies: {error}");
                GeneratedClient::new()
            }
        };
        Self {
            inner: inner.with_base_url(base_url.into().trim_end_matches('/')),
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
        request: APILoginRequest,
    ) -> Result<APILoginResponse, generated::client::ApiOpError<generated::client::LoginApiError>>
    {
        self.inner.login(API_VERSION, request).await
    }

    pub async fn logout(
        &self,
    ) -> Result<(), generated::client::ApiOpError<generated::client::LogoutApiError>> {
        self.inner.logout(API_VERSION).await
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
    use super::{APILoginRequest, AgentClient};

    #[tokio::test]
    async fn login_keeps_the_session_cookie_for_later_calls()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut server = mockito::Server::new_async().await;
        let login = server
            .mock("POST", "/api/login/")
            .match_header("x-api-version", "1.0.0")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "username": "alice", "password": "secret",
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header(
                "set-cookie",
                "modama_agent_session=abc; HttpOnly; SameSite=Strict; Path=/",
            )
            .with_body(r#"{"username":"alice"}"#)
            .create_async()
            .await;
        let delete = server
            .mock("DELETE", "/api/shares/share-123/")
            .match_header("cookie", "modama_agent_session=abc")
            .match_header("x-api-version", "1.0.0")
            .with_status(204)
            .create_async()
            .await;

        let client = AgentClient::new(server.url());
        let response = client
            .login(APILoginRequest {
                username: "alice".to_owned(),
                password: "secret".to_owned(),
            })
            .await?;
        assert_eq!(response.username, "alice");
        client.delete_share("share-123").await?;

        login.assert_async().await;
        delete.assert_async().await;
        Ok(())
    }

    #[tokio::test]
    async fn identity_sends_api_version_without_credentials()
    -> Result<(), Box<dyn std::error::Error>> {
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

    /// `Url::as_str()` always ends in `/`, and the session helper's tray passes exactly that.
    #[tokio::test]
    async fn trailing_slash_in_base_url_does_not_double_the_slash()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/identity/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"type":"Anonymous","hostname":"workstation"}"#)
            .create_async()
            .await;

        AgentClient::new(format!("{}/", server.url()))
            .get_identity()
            .await?;
        mock.assert_async().await;
        Ok(())
    }
}
