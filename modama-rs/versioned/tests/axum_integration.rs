#![allow(clippy::unwrap_used, clippy::expect_used)]
use axum::{
    Json,
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use tower::ServiceExt;
use versioned::VersionedRouterBuilder;
use versioned::versions;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct User {
    id: u32,
    name: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Product {
    id: u32,
    title: String,
}

fn create_test_builder() -> VersionedRouterBuilder<()> {
    async fn users_v1_handler() -> impl IntoResponse {
        Json(vec![User {
            id: 1,
            name: "v1 User".to_string(),
        }])
    }
    async fn users_v2_handler() -> impl IntoResponse {
        Json(vec![User {
            id: 1,
            name: "v2 User".to_string(),
        }])
    }
    async fn products_v1_handler() -> impl IntoResponse {
        Json(vec![Product {
            id: 1,
            title: "v1 Product".to_string(),
        }])
    }

    VersionedRouterBuilder::new()
        // User endpoint - v1 and v2
        .route_get("/users", versions!("1.0.0..2.0.0"), get(users_v1_handler))
        .route_get("/users", versions!("2.0.0.."), get(users_v2_handler))
        // Product endpoint - v1 only (bounded range)
        .route_get(
            "/products",
            versions!("1.0.0..2.0.0"),
            get(products_v1_handler),
        )
}

#[tokio::test]
async fn test_users_v1() {
    let builder = create_test_builder();
    let app = builder.build(()).unwrap();

    let request = Request::builder()
        .method("GET")
        .uri("/users")
        .header("X-API-Version", "1.0.0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let users: Vec<User> = serde_json::from_slice(&body).unwrap();
    assert_eq!(users[0].name, "v1 User");
}

#[tokio::test]
async fn test_users_v2() {
    let builder = create_test_builder();
    let app = builder.build(()).unwrap();

    let request = Request::builder()
        .method("GET")
        .uri("/users")
        .header("X-API-Version", "2.0.0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let users: Vec<User> = serde_json::from_slice(&body).unwrap();
    assert_eq!(users[0].name, "v2 User");
}

#[tokio::test]
async fn test_products_v1_only() {
    let builder = create_test_builder();
    let app = builder.build(()).unwrap();

    // v1 should work
    let request = Request::builder()
        .method("GET")
        .uri("/products")
        .header("X-API-Version", "1.0.0")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // v2 should return 404 (no products endpoint)
    let request = Request::builder()
        .method("GET")
        .uri("/products")
        .header("X-API-Version", "2.0.0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_invalid_version_header() {
    let builder = create_test_builder();
    let app = builder.build(()).unwrap();

    let request = Request::builder()
        .method("GET")
        .uri("/users")
        .header("X-API-Version", "invalid")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_missing_version_header() {
    let builder = create_test_builder();
    let app = builder.build(()).unwrap();

    let request = Request::builder()
        .method("GET")
        .uri("/users")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
mod support;
use axum::routing::get;
use support::TestRoutes;
