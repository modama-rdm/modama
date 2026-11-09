//! What a signed-in user can do about ingests from this computer: create a session, open an
//! ingest share for it, and stop an ingest share. The HTML pages and the JSON API both call
//! these; they only translate the requests and render the outcomes.
//!
//! Each action fetches what it needs (this agent's token, the management API), makes its calls
//! with the HTTP client's sign-in, which the management API may renew on the way. The
//! management API decides what is allowed: which names are valid, which sessions exist, and
//! whose ingest shares the user may stop.

use std::sync::Arc;

use modama_common::tokens::{TokenError, TokenProvider};
use url::Url;

use crate::mgmt_api::{MgmtApi, MgmtApiConnection, MgmtApiError, SignIn};

/// A convenience wrapper around the management API client,
/// providing ingest-related methods
pub struct IngestActions<'a> {
    pub mgmt_api: &'a Arc<MgmtApi>,
    /// The configured `mgmt_api_url`
    pub mgmt_api_url: &'a Url,
    pub token_provider: &'a Arc<dyn TokenProvider + Send + Sync>,
}

/// An ingest share that is open now, without its credentials
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenIngest {
    pub share_id: String,
    pub project: String,
    pub session: String,
    pub dest_path: String,
    /// None if the management API doesn't report it yet
    pub canonical_path: Option<String>,
}

/// What the management API reported as missing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    Project,
    Session,
    Share,
    Other,
}

impl Missing {
    fn from_code(code: &str) -> Self {
        match code {
            "project_not_found" => Self::Project,
            "session_not_found" => Self::Session,
            "share_not_found" => Self::Share,
            _ => Self::Other,
        }
    }
}

/// Why an action was not done
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    /// The user isn't signed in, or the management API ended their sign-in
    #[error("not signed in, or the session has ended")]
    NotSignedIn,

    /// The project, session or ingest share doesn't exist
    #[error("{message}")]
    NotFound { what: Missing, message: String },

    /// Not the user's to do, for example stopping an ingest share someone else opened, or
    /// adding to a project they aren't a member of
    #[error("{message}")]
    Forbidden { message: String },

    /// The management API refused the request as it stands, for example an invalid session name
    #[error("the management API refused the request ({status}): {message}")]
    Refused { status: u16, message: String },

    /// This agent has no token to open an ingest share with
    #[error("token error: {0}")]
    Token(#[from] TokenError),

    /// The management API couldn't be reached or didn't answer as expected
    #[error("management API: {0}")]
    Failed(MgmtApiError),
}

impl From<MgmtApiError> for ActionError {
    fn from(error: MgmtApiError) -> Self {
        match error {
            MgmtApiError::Unauthorized => Self::NotSignedIn,
            MgmtApiError::Missing { code, message } => Self::NotFound {
                what: Missing::from_code(&code),
                message,
            },
            MgmtApiError::Forbidden { message, .. } => Self::Forbidden { message },
            MgmtApiError::Backend { status, message } if (400..500).contains(&status) => {
                Self::Refused { status, message }
            }
            error => Self::Failed(error),
        }
    }
}

impl IngestActions<'_> {
    async fn mgmt(&self) -> Result<MgmtApiConnection, MgmtApiError> {
        self.mgmt_api.connect(self.mgmt_api_url).await
    }

    /// Create session `name` in `project`. The management API checks the name, and answers
    /// for a session that exists already as for a new one.
    pub async fn create_session(
        &self,
        sign_in: &mut SignIn,
        project: &str,
        name: &str,
    ) -> Result<(), ActionError> {
        self.mgmt()
            .await?
            .create_session(sign_in, project, name)
            .await?;
        Ok(())
    }

    /// Open an ingest share for `session` in `project`, bound to this agent's resource; if the
    /// user has one open for the same session and resource, that one
    pub async fn start_ingest(
        &self,
        sign_in: &mut SignIn,
        project: &str,
        session: &str,
    ) -> Result<OpenIngest, ActionError> {
        let token = Arc::clone(self.token_provider).fetch_token().await?;
        let share = self
            .mgmt()
            .await?
            .open_ingest_share(sign_in, project, session, &token.resource, &token.token)
            .await?;
        Ok(OpenIngest {
            share_id: share.share_id,
            project: share.project,
            session: share.session,
            dest_path: share.dest_path,
            canonical_path: share.canonical_path.flatten(),
        })
    }

    /// Stop the ingest share `share_id`, whichever it is; the management API lets only the
    /// user who opened it stop it
    pub async fn stop_ingest(
        &self,
        sign_in: &mut SignIn,
        share_id: &str,
    ) -> Result<(), ActionError> {
        self.mgmt()
            .await?
            .close_ingest_share(sign_in, share_id)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ActionError, Missing};
    use crate::mgmt_api::MgmtApiError;

    #[test]
    fn management_api_errors_become_outcomes() {
        let missing = |code: &str| MgmtApiError::Missing {
            code: code.to_owned(),
            message: "gone".to_owned(),
        };
        assert!(matches!(
            ActionError::from(MgmtApiError::Unauthorized),
            ActionError::NotSignedIn
        ));
        for (code, what) in [
            ("project_not_found", Missing::Project),
            ("session_not_found", Missing::Session),
            ("share_not_found", Missing::Share),
            ("path_not_found", Missing::Other),
        ] {
            assert!(matches!(
                ActionError::from(missing(code)),
                ActionError::NotFound { what: found, .. } if found == what
            ));
        }
        assert!(matches!(
            ActionError::from(MgmtApiError::Forbidden {
                code: "not_share_owner".to_owned(),
                message: "Only the owner can close it.".to_owned(),
            }),
            ActionError::Forbidden { message } if message == "Only the owner can close it."
        ));
        assert!(matches!(
            ActionError::from(MgmtApiError::Backend {
                status: 409,
                message: "taken".to_owned(),
            }),
            ActionError::Refused { status: 409, .. }
        ));
        for failure in [
            MgmtApiError::Backend {
                status: 500,
                message: "oops".to_owned(),
            },
            MgmtApiError::NoRoute,
            MgmtApiError::Unreachable("refused".to_owned()),
        ] {
            assert!(matches!(ActionError::from(failure), ActionError::Failed(_)));
        }
    }
}
