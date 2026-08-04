#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Full integration tests for versioned routing - Step 1: Core extraction features
//!
//! This test file verifies that all standard Axum extractors work correctly
//! with the versioned routing system.

use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tower::ServiceExt;
use versioned::VersionedRouterBuilder;
use versioned::versions;

// Test state for State extraction
#[derive(Debug, Clone)]
struct AppState {
    domain: String,
    max_items: usize,
}

// Test request body for Json extraction
#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CreateUserRequest {
    name: String,
    email: String,
}

// Test response for Json extraction
#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct UserResponse {
    id: u32,
    name: String,
    email: String,
    domain: String,
}

// Test path parameters
#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct ItemResponse {
    item_id: u32,
    user_id: u32,
    name: String,
}

// ==================== V1 Handlers ====================

async fn create_user_v1(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> impl IntoResponse {
    Json(UserResponse {
        id: 1,
        name: req.name,
        email: req.email,
        domain: state.domain.clone(),
    })
}

async fn get_user_item_v1(
    State(state): State<AppState>,
    Path((user_id, item_id)): Path<(u32, u32)>,
) -> impl IntoResponse {
    Json(ItemResponse {
        item_id,
        user_id,
        name: format!(
            "Item {} for user {} from {}",
            item_id, user_id, state.domain
        ),
    })
}

async fn echo_body_v1(State(state): State<AppState>, body: Body) -> impl IntoResponse {
    let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
    let body_str = String::from_utf8_lossy(&bytes);

    Json(serde_json::json!({
        "echo": body_str,
        "domain": state.domain
    }))
}

async fn get_user_optional_v1(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> impl IntoResponse {
    // V1: returns name only, no email processing
    Json(serde_json::json!({
        "id": 1,
        "name": req.name,
        "domain": state.domain
    }))
}

// ==================== V2 Handlers ====================

async fn create_user_v2(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> impl IntoResponse {
    Json(UserResponse {
        id: 2,
        name: req.name,
        email: req.email,
        domain: state.domain.clone(),
    })
}

async fn get_user_item_v2(
    State(state): State<AppState>,
    Path((user_id, item_id)): Path<(u32, u32)>,
) -> impl IntoResponse {
    Json(ItemResponse {
        item_id,
        user_id,
        name: format!(
            "[V2] Item {} for user {} from {} (max_items={})",
            item_id, user_id, state.domain, state.max_items
        ),
    })
}

async fn echo_body_v2(State(state): State<AppState>, body: Body) -> impl IntoResponse {
    let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
    let body_str = String::from_utf8_lossy(&bytes);

    Json(serde_json::json!({
        "echo": body_str,
        "domain": state.domain,
        "max_items": state.max_items
    }))
}

async fn get_user_optional_v2(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> impl IntoResponse {
    // V2: returns full user data with email validation
    Json(serde_json::json!({
        "id": 2,
        "name": req.name,
        "email": req.email,
        "domain": state.domain,
        "max_items": state.max_items
    }))
}

// ==================== Test Setup ====================

fn create_test_builder() -> VersionedRouterBuilder<AppState> {
    VersionedRouterBuilder::new()
        // User creation - different response IDs for v1 vs v2
        .route_post("/users", versions!("1.0.0..2.0.0"), post(create_user_v1))
        .route_post("/users", versions!("2.0.0.."), post(create_user_v2))
        // User items with path parameters
        .route_get(
            "/users/{user_id}/items/{item_id}",
            versions!("1.0.0..2.0.0"),
            get(get_user_item_v1),
        )
        .route_get(
            "/users/{user_id}/items/{item_id}",
            versions!("2.0.0.."),
            get(get_user_item_v2),
        )
        // Body echo endpoint
        .route_post("/echo", versions!("1.0.0..2.0.0"), post(echo_body_v1))
        .route_post("/echo", versions!("2.0.0.."), post(echo_body_v2))
        // Optional endpoint with different methods per version
        .route_get(
            "/users/optional",
            versions!("1.0.0..2.0.0"),
            get(get_user_optional_v1),
        )
        .route_get(
            "/users/optional",
            versions!("2.0.0.."),
            get(get_user_optional_v2),
        )
}

fn create_test_state() -> AppState {
    AppState {
        domain: "example.com".to_string(),
        max_items: 100,
    }
}

// ==================== Tests: State Extraction ====================

#[tokio::test]
async fn test_state_extraction_v1() {
    let builder = create_test_builder();
    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    let request = Request::builder()
        .method("POST")
        .uri("/users")
        .header("X-API-Version", "1.0.0")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"name":"Alice","email":"alice@example.com"}"#,
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let user: UserResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(user.id, 1); // V1 specific
    assert_eq!(user.name, "Alice");
    assert_eq!(user.domain, "example.com");
}

#[tokio::test]
async fn test_state_extraction_v2() {
    let builder = create_test_builder();
    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    let request = Request::builder()
        .method("POST")
        .uri("/users")
        .header("X-API-Version", "2.0.0")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"name":"Bob","email":"bob@example.com"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let user: UserResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(user.id, 2); // V2 specific
    assert_eq!(user.name, "Bob");
    assert_eq!(user.domain, "example.com");
}

// ==================== Tests: Json Extraction ====================

#[tokio::test]
async fn test_json_extraction_v1() {
    let builder = create_test_builder();
    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    let request = Request::builder()
        .method("POST")
        .uri("/users")
        .header("X-API-Version", "1.5.0")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"name":"Charlie","email":"charlie@test.com"}"#,
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let user: UserResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(user.name, "Charlie");
    assert_eq!(user.email, "charlie@test.com");
}

#[tokio::test]
async fn test_json_extraction_v2() {
    let builder = create_test_builder();
    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    let request = Request::builder()
        .method("POST")
        .uri("/users")
        .header("X-API-Version", "2.5.0")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"name":"Diana","email":"diana@test.com"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let user: UserResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(user.name, "Diana");
    assert_eq!(user.email, "diana@test.com");
}

// ==================== Tests: Path Extraction ====================

#[tokio::test]
async fn test_path_extraction_v1() {
    let builder = create_test_builder();
    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    let request = Request::builder()
        .method("GET")
        .uri("/users/123/items/456")
        .header("X-API-Version", "1.0.0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: ItemResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(item.user_id, 123);
    assert_eq!(item.item_id, 456);
    assert_eq!(item.name, "Item 456 for user 123 from example.com");
}

#[tokio::test]
async fn test_path_extraction_v2() {
    let builder = create_test_builder();
    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    let request = Request::builder()
        .method("GET")
        .uri("/users/789/items/101112")
        .header("X-API-Version", "2.0.0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: ItemResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(item.user_id, 789);
    assert_eq!(item.item_id, 101112);
    assert!(item.name.contains("[V2]"));
    assert!(item.name.contains("max_items=100"));
}

// ==================== Tests: Body Extraction ====================

#[tokio::test]
async fn test_body_extraction_v1() {
    let builder = create_test_builder();
    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    let test_payload = r#"{"message":"hello world","count":42}"#;

    let request = Request::builder()
        .method("POST")
        .uri("/echo")
        .header("X-API-Version", "1.0.0")
        .header("content-type", "application/json")
        .body(Body::from(test_payload))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["echo"], test_payload);
    assert_eq!(result["domain"], "example.com");
    assert!(result.get("max_items").is_none()); // V1 doesn't have max_items
}

#[tokio::test]
async fn test_body_extraction_v2() {
    let builder = create_test_builder();
    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    let test_payload = r#"{"message":"hello v2","count":99}"#;

    let request = Request::builder()
        .method("POST")
        .uri("/echo")
        .header("X-API-Version", "2.0.0")
        .header("content-type", "application/json")
        .body(Body::from(test_payload))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["echo"], test_payload);
    assert_eq!(result["domain"], "example.com");
    assert_eq!(result["max_items"], 100); // V2 includes max_items
}

// ==================== Tests: Combined Extractors ====================

#[tokio::test]
async fn test_combined_state_json_path() {
    let builder = create_test_builder();
    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    // Test that State and Path work together
    let request = Request::builder()
        .method("GET")
        .uri("/users/999/items/888")
        .header("X-API-Version", "1.5.0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: ItemResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(item.user_id, 999);
    assert_eq!(item.item_id, 888);
    assert!(item.name.contains("example.com"));
}

#[tokio::test]
async fn test_combined_state_json_body() {
    let builder = create_test_builder();
    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    // Test that State and Json/Body work together
    let request = Request::builder()
        .method("POST")
        .uri("/users")
        .header("X-API-Version", "2.0.0")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"name":"Combined","email":"test@combined.com"}"#,
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let user: UserResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(user.name, "Combined");
    assert_eq!(user.email, "test@combined.com");
    assert_eq!(user.domain, "example.com");
}

// ==================== Tests: Result-based Extraction ====================

async fn get_user_with_result_v1(
    State(state): State<AppState>,
    req: Result<Json<CreateUserRequest>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    match req {
        Ok(Json(data)) => Json(serde_json::json!({
            "id": 1,
            "name": data.name,
            "domain": state.domain,
            "result_type": "success"
        }))
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("JSON extraction failed: {}", e)
            })),
        )
            .into_response(),
    }
}

async fn get_user_with_result_v2(
    State(state): State<AppState>,
    req: Result<Json<CreateUserRequest>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    match req {
        Ok(Json(data)) => Json(serde_json::json!({
            "id": 2,
            "name": data.name,
            "email": data.email,
            "domain": state.domain,
            "max_items": state.max_items,
            "result_type": "success"
        }))
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("JSON extraction failed: {}", e)
            })),
        )
            .into_response(),
    }
}

#[tokio::test]
async fn test_result_extraction_success_v1() {
    let builder = VersionedRouterBuilder::new().route_post(
        "/users/result",
        versions!("1.0.0..2.0.0"),
        post(get_user_with_result_v1),
    );

    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    let request = Request::builder()
        .method("POST")
        .uri("/users/result")
        .header("X-API-Version", "1.0.0")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"name":"ResultUser","email":"result@test.com"}"#,
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["id"], 1);
    assert_eq!(result["name"], "ResultUser");
    assert_eq!(result["result_type"], "success");
}

#[tokio::test]
async fn test_result_extraction_success_v2() {
    let builder = VersionedRouterBuilder::new().route_post(
        "/users/result",
        versions!("2.0.0.."),
        post(get_user_with_result_v2),
    );

    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    let request = Request::builder()
        .method("POST")
        .uri("/users/result")
        .header("X-API-Version", "2.0.0")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"name":"ResultUserV2","email":"resultv2@test.com"}"#,
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["id"], 2);
    assert_eq!(result["name"], "ResultUserV2");
    assert_eq!(result["email"], "resultv2@test.com");
    assert_eq!(result["result_type"], "success");
}

#[tokio::test]
async fn test_result_extraction_invalid_json() {
    let builder = VersionedRouterBuilder::new().route_post(
        "/users/result",
        versions!("1.0.0.."),
        post(get_user_with_result_v1),
    );

    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    // Send invalid JSON
    let request = Request::builder()
        .method("POST")
        .uri("/users/result")
        .header("X-API-Version", "1.0.0")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"invalid json"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    // Handler returns 400 with error JSON
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Check that error field is present
    assert!(result.get("error").is_some());
    let error_msg = result["error"].as_str().unwrap();
    assert!(error_msg.contains("JSON extraction failed"));
}

// ==================== Tests: Different HTTP Methods per Route ====================

async fn handle_users_get_v1(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "method": "GET",
        "version": "v1",
        "users": [],
        "domain": state.domain
    }))
}

async fn handle_users_post_v1(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "method": "POST",
        "version": "v1",
        "user": req.name,
        "domain": state.domain
    }))
}

async fn handle_users_get_v2(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "method": "GET",
        "version": "v2",
        "users": [],
        "domain": state.domain,
        "max_items": state.max_items
    }))
}

async fn handle_users_post_v2(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "method": "POST",
        "version": "v2",
        "user": req.name,
        "email": req.email,
        "domain": state.domain,
        "max_items": state.max_items
    }))
}

#[tokio::test]
async fn test_different_methods_same_path_v1_get() {
    let builder = VersionedRouterBuilder::new()
        .route_get(
            "/multi",
            versions!("1.0.0..2.0.0"),
            get(handle_users_get_v1),
        )
        .route_post(
            "/multi",
            versions!("1.0.0..2.0.0"),
            post(handle_users_post_v1),
        );

    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    // Test GET method
    let request = Request::builder()
        .method("GET")
        .uri("/multi")
        .header("X-API-Version", "1.0.0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["method"], "GET");
    assert_eq!(result["version"], "v1");
}

#[tokio::test]
async fn test_different_methods_same_path_v1_post() {
    let builder = VersionedRouterBuilder::new()
        .route_get(
            "/multi",
            versions!("1.0.0..2.0.0"),
            get(handle_users_get_v1),
        )
        .route_post(
            "/multi",
            versions!("1.0.0..2.0.0"),
            post(handle_users_post_v1),
        );

    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    // Test POST method
    let request = Request::builder()
        .method("POST")
        .uri("/multi")
        .header("X-API-Version", "1.0.0")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"name":"MultiUser","email":"multi@test.com"}"#,
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["method"], "POST");
    assert_eq!(result["version"], "v1");
    assert_eq!(result["user"], "MultiUser");
}

#[tokio::test]
async fn test_different_methods_same_path_v2_get() {
    let builder = VersionedRouterBuilder::new()
        .route_get("/multi", versions!("2.0.0.."), get(handle_users_get_v2))
        .route_post("/multi", versions!("2.0.0.."), post(handle_users_post_v2));

    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    // Test GET method
    let request = Request::builder()
        .method("GET")
        .uri("/multi")
        .header("X-API-Version", "2.0.0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["method"], "GET");
    assert_eq!(result["version"], "v2");
    assert_eq!(result["max_items"], 100);
}

#[tokio::test]
async fn test_different_methods_same_path_v2_post() {
    let builder = VersionedRouterBuilder::new()
        .route_get("/multi", versions!("2.0.0.."), get(handle_users_get_v2))
        .route_post("/multi", versions!("2.0.0.."), post(handle_users_post_v2));

    let state = create_test_state();
    let cached = builder.build(state).unwrap();
    let app = cached.into_axum_router();

    // Test POST method
    let request = Request::builder()
        .method("POST")
        .uri("/multi")
        .header("X-API-Version", "2.0.0")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"name":"MultiUserV2","email":"multi2@test.com"}"#,
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["method"], "POST");
    assert_eq!(result["version"], "v2");
    assert_eq!(result["user"], "MultiUserV2");
    assert_eq!(result["email"], "multi2@test.com");
    assert_eq!(result["max_items"], 100);
}
mod support;
use support::TestRoutes;
