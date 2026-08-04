#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Tests for version header requirements.

use axum::{
    Json,
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
};
use serde_json::json;
use tower::ServiceExt;
use versioned::VersionedRouterBuilder;
use versioned::versions;

#[tokio::test]
async fn test_missing_version_header_returns_400() {
    async fn greeting_v1() -> impl IntoResponse {
        Json(json!({"version": "v1", "message": "Hello from v1"}))
    }
    async fn greeting_v2() -> impl IntoResponse {
        Json(json!({"version": "v2", "message": "Hello from v2"}))
    }

    let builder = VersionedRouterBuilder::new()
        .route_get("/greeting", versions!("1.0.0..2.0.0"), get(greeting_v1))
        .route_get("/greeting", versions!("2.0.0..3.0.0"), get(greeting_v2));

    let cached = builder.build(()).unwrap();
    let app = cached.into_axum_router();

    // Request WITHOUT X-API-Version header - should return 400
    let req = Request::builder()
        .method("GET")
        .uri("/greeting")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // Should return 400 Bad Request (missing required header)
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_valid_version_header() {
    async fn greeting_v1() -> impl IntoResponse {
        Json(json!({"version": "v1", "message": "Hello from v1"}))
    }
    async fn greeting_v2() -> impl IntoResponse {
        Json(json!({"version": "v2", "message": "Hello from v2"}))
    }

    let builder = VersionedRouterBuilder::new()
        .route_get("/greeting", versions!("1.0.0..2.0.0"), get(greeting_v1))
        .route_get("/greeting", versions!("2.0.0..3.0.0"), get(greeting_v2));

    let cached = builder.build(()).unwrap();
    let app = cached.into_axum_router();

    // Request WITH v1.5.0 header - should return v1
    let req = Request::builder()
        .method("GET")
        .uri("/greeting")
        .header("X-API-Version", "1.5.0")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(response["version"], "v1");
}
mod support;
use axum::routing::get;
use support::TestRoutes;
