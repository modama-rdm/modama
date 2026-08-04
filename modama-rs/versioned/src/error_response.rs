//! Error responses for version negotiation failures.

use crate::VersionRange;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Invalid or conflicting versioned route configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteConfigError {
    #[error("invalid route path '{path}': {reason}")]
    InvalidPath { path: String, reason: String },
    #[error(
        "conflicting routes '{first_path}' ({first_range}) and '{second_path}' \
         ({second_range}) for method {method:?}"
    )]
    Conflict {
        first_path: String,
        second_path: String,
        method: Option<String>,
        first_range: VersionRange,
        second_range: VersionRange,
    },
    #[error(
        "conflicting OpenAPI schema '{schema_name}' from '{first_source}' and '{second_source}'"
    )]
    SchemaConflict {
        schema_name: String,
        first_source: String,
        second_source: String,
    },
}

/// Error response for version negotiation failures.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VersioningError {
    UnsupportedVersion {
        error: String,
        supported: Vec<String>,
        header: String,
    },
    InvalidVersion {
        error: String,
        supported: Vec<String>,
        version: String,
        header: String,
    },
    MissingVersion {
        error: String,
        supported: Vec<String>,
        header: String,
    },
}

impl std::fmt::Display for VersioningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion { error, .. } => {
                write!(f, "{}", error)
            }
            Self::InvalidVersion { error, .. } => {
                write!(f, "{}", error)
            }
            Self::MissingVersion { error, .. } => {
                write!(f, "{}", error)
            }
        }
    }
}

impl IntoResponse for VersioningError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::UnsupportedVersion { .. } => StatusCode::BAD_REQUEST,
            Self::InvalidVersion { .. } => StatusCode::BAD_REQUEST,
            Self::MissingVersion { .. } => StatusCode::BAD_REQUEST,
        };

        (status, Json(self)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unsupported_version_error() {
        let error = VersioningError::UnsupportedVersion {
            error: "Unsupported API version".to_string(),
            supported: vec!["1.0.0..2.0.0".to_string(), "2.0.0..3.0.0".to_string()],
            header: "X-API-Version".to_string(),
        };

        let response = error.into_response();
        let (parts, body) = response.into_parts();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);

        let bytes = axum::body::to_bytes(body, 1024).await.unwrap();
        let json: VersioningError = serde_json::from_slice(&bytes).unwrap();
        match json {
            VersioningError::UnsupportedVersion {
                error,
                supported,
                header,
            } => {
                assert_eq!(error, "Unsupported API version");
                assert_eq!(supported, vec!["1.0.0..2.0.0", "2.0.0..3.0.0"]);
                assert_eq!(header, "X-API-Version");
            }
            _ => panic!("Expected UnsupportedVersion variant"),
        }
    }

    #[tokio::test]
    async fn test_invalid_version_error() {
        let error = VersioningError::InvalidVersion {
            error: "Invalid version format: invalid".to_string(),
            supported: vec![],
            version: "invalid".to_string(),
            header: "X-API-Version".to_string(),
        };

        let response = error.into_response();
        let (parts, body) = response.into_parts();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);

        let bytes = axum::body::to_bytes(body, 1024).await.unwrap();
        let json: VersioningError = serde_json::from_slice(&bytes).unwrap();
        match json {
            VersioningError::InvalidVersion { error, version, .. } => {
                assert!(error.contains("Invalid version format"));
                assert_eq!(version, "invalid");
            }
            _ => panic!("Expected InvalidVersion variant"),
        }
    }

    #[tokio::test]
    async fn test_missing_version_error() {
        let error = VersioningError::MissingVersion {
            error: "Missing required header: X-API-Version".to_string(),
            supported: vec![],
            header: "X-API-Version".to_string(),
        };

        let response = error.into_response();
        let (parts, body) = response.into_parts();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);

        let bytes = axum::body::to_bytes(body, 1024).await.unwrap();
        let json: VersioningError = serde_json::from_slice(&bytes).unwrap();
        match json {
            VersioningError::MissingVersion { error, header, .. } => {
                assert!(error.contains("Missing required header"));
                assert_eq!(header, "X-API-Version");
            }
            _ => panic!("Expected MissingVersion variant"),
        }
    }

    #[test]
    fn test_versioning_error_display() {
        let err = VersioningError::UnsupportedVersion {
            error: "Unsupported API version".to_string(),
            supported: vec!["1.0.0".to_string()],
            header: "X-API-Version".to_string(),
        };
        assert_eq!(err.to_string(), "Unsupported API version");

        let err = VersioningError::InvalidVersion {
            error: "Invalid format".to_string(),
            supported: vec![],
            version: "bad".to_string(),
            header: "X-API-Version".to_string(),
        };
        assert_eq!(err.to_string(), "Invalid format");

        let err = VersioningError::MissingVersion {
            error: "Missing header".to_string(),
            supported: vec![],
            header: "X-API-Version".to_string(),
        };
        assert_eq!(err.to_string(), "Missing header");
    }
}
