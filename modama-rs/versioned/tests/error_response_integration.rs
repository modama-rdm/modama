//! Integration tests for VersioningError JSON response structure.
//!
//! These tests verify that all error response fields match the documented schema.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;
use versioned::{VersionedRouterBuilder, versioned_route, versions};

#[utoipa::path(get, path = "/test")]
async fn test_handler() -> &'static str {
    "ok"
}

fn create_test_builder() -> VersionedRouterBuilder<()> {
    VersionedRouterBuilder::new()
        .route(versions!("1.0.0..2.0.0"), versioned_route!(test_handler))
        .route(versions!("2.0.0.."), versioned_route!(test_handler))
}

async fn make_request(version: Option<&str>) -> (StatusCode, Value) {
    let builder = create_test_builder();
    let app = builder.build(()).unwrap();

    let mut req_builder = Request::builder().method("GET").uri("/test");

    if let Some(v) = version {
        req_builder = req_builder.header("X-API-Version", v);
    }

    let request = req_builder
        .body(Body::empty())
        .expect("should be able to build HTTP request");
    let response = app.oneshot(request).await.expect("request should succeed");

    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, 1024)
        .await
        .expect("body read should not fail");
    let json: Value = serde_json::from_slice(&bytes).expect("parse JSON");

    (parts.status, json)
}

#[tokio::test]
async fn test_missing_version_error_structure() {
    let (status, json) = make_request(None).await;

    // Verify status code
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Verify error type
    assert_eq!(json["type"], "missing_version");

    // Verify error message contains expected text
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("Missing required header")
    );
    assert!(json["error"].as_str().unwrap().contains("X-API-Version"));

    // Verify supported array is present and non-empty
    assert!(json["supported"].is_array());
    assert!(!json["supported"].as_array().unwrap().is_empty());

    // Verify header field
    assert_eq!(json["header"], "X-API-Version");
}

#[tokio::test]
async fn test_invalid_version_error_structure() {
    let (status, json) = make_request(Some("not-a-version")).await;

    // Verify status code
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Verify error type
    assert_eq!(json["type"], "invalid_version");

    // Verify error message contains expected text
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("Invalid version format")
    );
    assert!(json["error"].as_str().unwrap().contains("not-a-version"));

    // Verify supported array is present
    assert!(json["supported"].is_array());

    // Verify version field contains the invalid input
    assert_eq!(json["version"], "not-a-version");

    // Verify header field
    assert_eq!(json["header"], "X-API-Version");
}

#[tokio::test]
async fn test_version_modifiers_are_invalid() {
    for version in ["1.0.0-alpha", "1.0.0+build"] {
        let (status, json) = make_request(Some(version)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["type"], "invalid_version");
        assert_eq!(json["version"], version);
    }
}

#[tokio::test]
async fn test_unsupported_version_error_structure() {
    // Request version 0.9.0 which is before all defined ranges (1.0.0..)
    let (status, json) = make_request(Some("0.9.0")).await;

    // Verify status code
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Verify error type
    assert_eq!(json["type"], "unsupported_version");

    // Verify error message contains expected text
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("No routes match version")
    );
    assert!(json["error"].as_str().unwrap().contains("0.9.0"));

    // Verify supported array is present and non-empty
    assert!(json["supported"].is_array());
    let supported = json["supported"].as_array().unwrap();
    assert!(!supported.is_empty());

    // Verify header field
    assert_eq!(json["header"], "X-API-Version");

    // Verify version field is NOT present for unsupported_version
    // (only invalid_version has the version field)
    assert!(json.get("version").is_none());
}
