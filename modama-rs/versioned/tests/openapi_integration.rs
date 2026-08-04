#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Integration tests for OpenAPI generation with versioned routes
//!
//! This file focuses on testing:
//! - OpenAPI spec generation from versioned routes
//! - Multi-method routing with OpenAPI
//! - Schema collection from handlers
//! - Mixed OpenAPI and plain routes

use axum::{
    Json,
    body::Body,
    extract::Path,
    http::{Method, Request, StatusCode},
    response::IntoResponse,
    routing::get,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower::ServiceExt;
use utoipa::ToSchema;
use versioned::{VersionedRouterBuilder, openapi_from_handler, versions};

// ============================================================================
// Test Types
// ============================================================================

#[derive(Serialize, Deserialize, ToSchema, Debug, PartialEq)]
#[schema(title = "UserV1")]
struct UserV1 {
    id: u32,
    name: String,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, PartialEq)]
#[schema(title = "UserV2")]
struct UserV2 {
    id: u32,
    name: String,
    email: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[schema(title = "CreateUserV1")]
struct CreateUserV1 {
    name: String,
}

// ============================================================================
// Test Handlers with utoipa path metadata
// ============================================================================

#[utoipa::path(
    get,
    path = "/users/{id}",
    params(("id" = u32, Path, description = "User ID")),
    responses((status = 200, description = "User found", body = UserV1)),
    tags = ["users"]
)]
async fn get_user_v1(Path(id): Path<u32>) -> impl IntoResponse {
    Json(UserV1 {
        id,
        name: format!("User {}", id),
    })
}

#[utoipa::path(
    get,
    path = "/users/{id}",
    params(("id" = u32, Path, description = "User ID")),
    responses((status = 200, description = "User found with email", body = UserV2)),
    tags = ["users"]
)]
async fn get_user_v2(Path(id): Path<u32>) -> impl IntoResponse {
    Json(UserV2 {
        id,
        name: format!("User {}", id),
        email: format!("user{}@example.com", id),
    })
}

#[utoipa::path(
    post,
    path = "/users",
    request_body = CreateUserV1,
    responses((status = 201, description = "User created", body = UserV1)),
    tags = ["users"]
)]
async fn create_user_v1(Json(req): Json<CreateUserV1>) -> impl IntoResponse {
    Json(UserV1 {
        id: 1,
        name: req.name,
    })
}

// Handler with MULTIPLE methods using method() syntax
#[utoipa::path(
    method(get, post),
    path = "/items/{id}",
    params(("id" = u32, Path, description = "Item ID")),
    responses(
        (status = 200, description = "Item retrieved", body = UserV1),
        (status = 201, description = "Item created", body = UserV1)
    ),
    tags = ["items"]
)]
async fn get_or_create_item_v1(Path(id): Path<u32>) -> impl IntoResponse {
    Json(UserV1 {
        id,
        name: format!("Item {}", id),
    })
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_openapi_generation() {
    let (metadata, handler) = openapi_from_handler!(get_user_v1);

    let builder: VersionedRouterBuilder<()> =
        VersionedRouterBuilder::new().route_with_openapi(versions!("1.0.0.."), handler, metadata);

    let openapi = builder.generate_openapi(Version::new(1, 0, 0)).unwrap();
    let json = serde_json::to_string_pretty(&openapi).unwrap();
    let spec: Value = serde_json::from_str(&json).unwrap();

    // Verify path exists
    let paths = spec.get("paths").and_then(|p| p.as_object()).unwrap();
    assert!(paths.contains_key("/users/{id}"));

    // Verify GET method exists
    let user_path = paths
        .get("/users/{id}")
        .and_then(|p| p.as_object())
        .unwrap();
    assert!(user_path.contains_key("get"));

    // Verify UserV1 schema is in components
    let components = spec.get("components").and_then(|c| c.as_object()).unwrap();
    let schemas = components
        .get("schemas")
        .and_then(|s| s.as_object())
        .unwrap();
    assert!(schemas.contains_key("UserV1"));
}

#[tokio::test]
async fn test_multi_method_routing() {
    // Test that multi-method handlers work correctly
    let (metadata, handler) = openapi_from_handler!(get_or_create_item_v1);

    let builder: VersionedRouterBuilder<()> =
        VersionedRouterBuilder::new().route_with_openapi(versions!("1.0.0.."), handler, metadata);

    // Build the router using the cached builder pattern
    let cached = builder.build(()).unwrap();
    let router = cached.router_for_version(&Version::new(1, 0, 0)).await;

    // Test GET request
    let get_request = Request::builder()
        .method(Method::GET)
        .uri("/items/123")
        .body(Body::empty())
        .unwrap();

    let get_response = router.clone().oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);

    // Test POST request
    let post_request = Request::builder()
        .method(Method::POST)
        .uri("/items/123")
        .body(Body::empty())
        .unwrap();

    let post_response = router.oneshot(post_request).await.unwrap();
    assert_eq!(post_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_multi_method_openapi_generation() {
    let (metadata, handler) = openapi_from_handler!(get_or_create_item_v1);

    let builder: VersionedRouterBuilder<()> =
        VersionedRouterBuilder::new().route_with_openapi(versions!("1.0.0.."), handler, metadata);

    let openapi = builder.generate_openapi(Version::new(1, 0, 0)).unwrap();
    let json = serde_json::to_string_pretty(&openapi).unwrap();
    let spec: Value = serde_json::from_str(&json).unwrap();

    let paths = spec.get("paths").and_then(|p| p.as_object()).unwrap();
    let items_path = paths
        .get("/items/{id}")
        .and_then(|p| p.as_object())
        .unwrap();

    // Both methods should be present
    assert!(items_path.contains_key("get"));
    assert!(items_path.contains_key("post"));
}

#[tokio::test]
async fn test_magic_api_no_repetition() {
    let (meta1, handler1) = openapi_from_handler!(get_user_v1);
    let (meta2, handler2) = openapi_from_handler!(create_user_v1);
    let (meta3, handler3) = openapi_from_handler!(get_user_v2);

    let builder: VersionedRouterBuilder<()> = VersionedRouterBuilder::new()
        .route_with_openapi(versions!("1.0.0..2.0.0"), handler1, meta1)
        .route_with_openapi(versions!("1.0.0.."), handler2, meta2)
        .route_with_openapi(versions!("2.0.0.."), handler3, meta3);

    let openapi = builder.generate_openapi(Version::new(2, 0, 0)).unwrap();
    let json = serde_json::to_string_pretty(&openapi).unwrap();
    let spec: Value = serde_json::from_str(&json).unwrap();

    let paths = spec.get("paths").and_then(|p| p.as_object()).unwrap();

    // Both paths should exist
    assert!(paths.contains_key("/users/{id}"));
    assert!(paths.contains_key("/users"));

    // UserV2 schema should be present (from get_user_v2)
    let components = spec.get("components").and_then(|c| c.as_object()).unwrap();
    let schemas = components
        .get("schemas")
        .and_then(|s| s.as_object())
        .unwrap();
    assert!(schemas.contains_key("UserV2"));
}

#[tokio::test]
async fn test_mixed_openapi_and_plain_routes() {
    let (metadata, handler) = openapi_from_handler!(get_user_v1);

    let builder: VersionedRouterBuilder<()> = VersionedRouterBuilder::new()
        .route_with_openapi(versions!("1.0.0.."), handler, metadata)
        .route_get("/health", versions!("1.0.0.."), get(|| async { "OK" }));

    let openapi = builder.generate_openapi(Version::new(1, 0, 0)).unwrap();
    let json = serde_json::to_string_pretty(&openapi).unwrap();
    let spec: Value = serde_json::from_str(&json).unwrap();

    let paths = spec.get("paths").and_then(|p| p.as_object()).unwrap();

    // Both routes should be included
    assert!(paths.contains_key("/users/{id}"));
    assert!(paths.contains_key("/health"));
}

#[tokio::test]
async fn test_method_not_allowed_with_allow_header() {
    // Test that calling an invalid method on a multi-method route returns 405
    // and includes the correct Allow header listing supported methods
    let (metadata, handler) = openapi_from_handler!(get_or_create_item_v1);

    let builder: VersionedRouterBuilder<()> =
        VersionedRouterBuilder::new().route_with_openapi(versions!("1.0.0.."), handler, metadata);

    let cached = builder.build(()).unwrap();
    let router = cached.router_for_version(&Version::new(1, 0, 0)).await;

    // PUT is not supported on /items/{id} (only GET and POST)
    let put_request = Request::builder()
        .method(Method::PUT)
        .uri("/items/123")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(put_request).await.unwrap();

    // Should get 405 Method Not Allowed
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

    // Should have Allow header with the supported methods (GET, POST)
    let allow_header = response
        .headers()
        .get("Allow")
        .expect("Allow header should be present")
        .to_str()
        .expect("Allow header should be valid UTF-8");

    // The Allow header should contain both GET and POST
    assert!(
        allow_header.contains("GET") && allow_header.contains("POST"),
        "Allow header should contain GET and POST, got: {}",
        allow_header
    );

    println!("Allow header value: {}", allow_header);
}

#[tokio::test]
async fn test_version_extensions_in_openapi() {
    let (metadata, handler) = openapi_from_handler!(get_user_v1);

    let builder: VersionedRouterBuilder<()> = VersionedRouterBuilder::new().route_with_openapi(
        versions!("1.0.0..2.0.0"),
        handler,
        metadata,
    );

    let openapi = builder.generate_openapi(Version::new(1, 0, 0)).unwrap();
    let json = serde_json::to_string_pretty(&openapi).unwrap();
    let spec: Value = serde_json::from_str(&json).unwrap();

    let paths = spec.get("paths").and_then(|p| p.as_object()).unwrap();
    let user_path = paths
        .get("/users/{id}")
        .and_then(|p| p.as_object())
        .unwrap();
    let get_op = user_path.get("get").and_then(|g| g.as_object()).unwrap();

    // Check x-introduced-in-version
    let introduced = get_op
        .get("x-introduced-in-version")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(introduced, "1.0.0");

    // Check x-removed-in-version (bounded range)
    let removed = get_op
        .get("x-removed-in-version")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(removed, "2.0.0");
}

#[tokio::test]
async fn test_info_version_in_openapi() {
    let (metadata, handler) = openapi_from_handler!(get_user_v1);

    let builder: VersionedRouterBuilder<()> =
        VersionedRouterBuilder::new().route_with_openapi(versions!("1.0.0.."), handler, metadata);

    let openapi = builder.generate_openapi(Version::new(2, 5, 3)).unwrap();
    let json = serde_json::to_string_pretty(&openapi).unwrap();
    let spec: Value = serde_json::from_str(&json).unwrap();

    let info = spec.get("info").and_then(|i| i.as_object()).unwrap();
    let version = info.get("version").and_then(|v| v.as_str()).unwrap();
    assert_eq!(version, "2.5.3");
}

#[tokio::test]
async fn test_version_extensions_unbounded_range() {
    let (metadata, handler) = openapi_from_handler!(get_user_v1);

    let builder: VersionedRouterBuilder<()> =
        VersionedRouterBuilder::new().route_with_openapi(versions!("1.0.0.."), handler, metadata);

    let openapi = builder.generate_openapi(Version::new(1, 0, 0)).unwrap();
    let json = serde_json::to_string_pretty(&openapi).unwrap();
    let spec: Value = serde_json::from_str(&json).unwrap();

    let paths = spec.get("paths").and_then(|p| p.as_object()).unwrap();
    let user_path = paths
        .get("/users/{id}")
        .and_then(|p| p.as_object())
        .unwrap();
    let get_op = user_path.get("get").and_then(|g| g.as_object()).unwrap();

    // Check x-introduced-in-version exists
    let introduced = get_op
        .get("x-introduced-in-version")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(introduced, "1.0.0");

    // Check x-removed-in-version does NOT exist for unbounded range
    assert!(
        get_op.get("x-removed-in-version").is_none(),
        "x-removed-in-version should not exist for unbounded range"
    );
}

#[tokio::test]
async fn test_schema_exclusion_for_excluded_versions() {
    // Test that routes outside the version range are NOT included in the schema
    // Route get_user_v1 is only available for 1.0.0..2.0.0
    // When generating for 2.0.0, it should be excluded (2.0.0 is the end, exclusive)
    let (metadata, handler) = openapi_from_handler!(get_user_v1);

    let builder: VersionedRouterBuilder<()> = VersionedRouterBuilder::new().route_with_openapi(
        versions!("1.0.0..2.0.0"),
        handler,
        metadata,
    );

    // Generate for version 2.0.0 - the route should NOT be included
    let openapi = builder.generate_openapi(Version::new(2, 0, 0)).unwrap();
    let json = serde_json::to_string_pretty(&openapi).unwrap();
    let spec: Value = serde_json::from_str(&json).unwrap();

    let paths = spec.get("paths").and_then(|p| p.as_object()).unwrap();

    // The /users/{id} path should NOT exist because 2.0.0 is the exclusive upper bound
    assert!(
        !paths.contains_key("/users/{id}"),
        "Route with range 1.0.0..2.0.0 should not be included when generating for 2.0.0"
    );
}

#[tokio::test]
async fn test_schema_inclusion_for_in_range_versions() {
    // Test that routes within the version range ARE included
    // Route get_user_v1 is available for 1.0.0..2.0.0
    // When generating for 1.5.0, it should be included
    let (metadata, handler) = openapi_from_handler!(get_user_v1);

    let builder: VersionedRouterBuilder<()> = VersionedRouterBuilder::new().route_with_openapi(
        versions!("1.0.0..2.0.0"),
        handler,
        metadata,
    );

    // Generate for version 1.5.0 - the route SHOULD be included
    let openapi = builder.generate_openapi(Version::new(1, 5, 0)).unwrap();
    let json = serde_json::to_string_pretty(&openapi).unwrap();
    let spec: Value = serde_json::from_str(&json).unwrap();

    let paths = spec.get("paths").and_then(|p| p.as_object()).unwrap();

    // The /users/{id} path SHOULD exist because 1.5.0 is within 1.0.0..2.0.0
    assert!(
        paths.contains_key("/users/{id}"),
        "Route with range 1.0.0..2.0.0 should be included when generating for 1.5.0"
    );

    // Verify the extension values are correct
    let user_path = paths
        .get("/users/{id}")
        .and_then(|p| p.as_object())
        .unwrap();
    let get_op = user_path.get("get").and_then(|g| g.as_object()).unwrap();

    let introduced = get_op
        .get("x-introduced-in-version")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(introduced, "1.0.0");

    let removed = get_op
        .get("x-removed-in-version")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(removed, "2.0.0");
}

#[tokio::test]
async fn test_multiple_versions_schema_filtering() {
    // Test that when generating for a specific version, only routes in range are included
    let (meta1, handler1) = openapi_from_handler!(get_user_v1);
    let (meta2, handler2) = openapi_from_handler!(get_user_v2);

    let builder: VersionedRouterBuilder<()> = VersionedRouterBuilder::new()
        .route_with_openapi(versions!("1.0.0..2.0.0"), handler1, meta1) // v1 only
        .route_with_openapi(versions!("2.0.0.."), handler2, meta2); // v2+ only

    // Generate for 1.5.0 - only v1 route should be present
    let openapi_v15 = builder.generate_openapi(Version::new(1, 5, 0)).unwrap();
    let spec_v15: Value = serde_json::to_string_pretty(&openapi_v15)
        .map(|j| serde_json::from_str(&j).unwrap())
        .unwrap();

    let paths_v15 = spec_v15.get("paths").and_then(|p| p.as_object()).unwrap();
    assert!(paths_v15.contains_key("/users/{id}"));

    // The operation should have v1 extensions
    let user_path = paths_v15
        .get("/users/{id}")
        .and_then(|p| p.as_object())
        .unwrap();
    let get_op = user_path.get("get").and_then(|g| g.as_object()).unwrap();
    let introduced = get_op
        .get("x-introduced-in-version")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(introduced, "1.0.0");
    let removed = get_op
        .get("x-removed-in-version")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(removed, "2.0.0");

    // Generate for 2.5.0 - only v2 route should be present
    let openapi_v25 = builder.generate_openapi(Version::new(2, 5, 0)).unwrap();
    let spec_v25: Value = serde_json::to_string_pretty(&openapi_v25)
        .map(|j| serde_json::from_str(&j).unwrap())
        .unwrap();

    let paths_v25 = spec_v25.get("paths").and_then(|p| p.as_object()).unwrap();
    assert!(paths_v25.contains_key("/users/{id}"));

    // The operation should have v2 extensions (unbounded)
    let user_path_v2 = paths_v25
        .get("/users/{id}")
        .and_then(|p| p.as_object())
        .unwrap();
    let get_op_v2 = user_path_v2.get("get").and_then(|g| g.as_object()).unwrap();
    let introduced_v2 = get_op_v2
        .get("x-introduced-in-version")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(introduced_v2, "2.0.0");
    assert!(
        get_op_v2.get("x-removed-in-version").is_none(),
        "v2 route should not have x-removed-in-version"
    );
}

#[tokio::test]
async fn test_request_body_schema_reference() {
    // Verify that request body schemas are correctly referenced in OpenAPI
    let (metadata, handler) = openapi_from_handler!(create_user_v1);

    let builder: VersionedRouterBuilder<()> =
        VersionedRouterBuilder::new().route_with_openapi(versions!("1.0.0.."), handler, metadata);

    let openapi = builder.generate_openapi(Version::new(1, 0, 0)).unwrap();
    let spec: Value =
        serde_json::from_str(&serde_json::to_string_pretty(&openapi).unwrap()).unwrap();

    let paths = spec.get("paths").and_then(|p| p.as_object()).unwrap();
    let users_path = paths.get("/users").and_then(|p| p.as_object()).unwrap();
    let post_op = users_path.get("post").and_then(|p| p.as_object()).unwrap();

    // Verify request body exists
    let request_body = post_op
        .get("requestBody")
        .expect("requestBody should exist");
    let request_body_obj = request_body
        .as_object()
        .expect("requestBody should be an object");

    // Verify content type
    let content = request_body_obj
        .get("content")
        .expect("content should exist");
    let application_json = content
        .get("application/json")
        .expect("application/json should exist");

    // Verify schema reference
    let schema = application_json.get("schema").expect("schema should exist");
    let schema_ref = schema.get("$ref").expect("schema should have $ref");
    let schema_ref_str = schema_ref.as_str().expect("$ref should be a string");

    // Should reference CreateUserV1
    assert_eq!(
        schema_ref_str, "#/components/schemas/CreateUserV1",
        "Request body should reference CreateUserV1 schema"
    );

    // Verify CreateUserV1 schema exists in components
    let components = spec.get("components").and_then(|c| c.as_object()).unwrap();
    let schemas = components
        .get("schemas")
        .and_then(|s| s.as_object())
        .unwrap();
    assert!(
        schemas.contains_key("CreateUserV1"),
        "CreateUserV1 schema should exist in components"
    );
}

#[tokio::test]
async fn test_error_response_schema_reference() {
    // Verify that VersioningError is referenced in 400 responses
    let (metadata, handler) = openapi_from_handler!(get_user_v1);

    let builder: VersionedRouterBuilder<()> =
        VersionedRouterBuilder::new().route_with_openapi(versions!("1.0.0.."), handler, metadata);

    let openapi = builder.generate_openapi(Version::new(1, 0, 0)).unwrap();
    let spec: Value =
        serde_json::from_str(&serde_json::to_string_pretty(&openapi).unwrap()).unwrap();

    let paths = spec.get("paths").and_then(|p| p.as_object()).unwrap();
    let user_path = paths
        .get("/users/{id}")
        .and_then(|p| p.as_object())
        .unwrap();
    let get_op = user_path.get("get").and_then(|p| p.as_object()).unwrap();

    // Verify responses exist
    let responses = get_op.get("responses").expect("responses should exist");
    let responses_obj = responses
        .as_object()
        .expect("responses should be an object");

    // Verify 400 response exists (for versioning errors)
    let response_400 = responses_obj.get("400").expect("400 response should exist");
    let response_400_obj = response_400.as_object().expect("400 should be an object");

    // Verify content exists
    let content = response_400_obj
        .get("content")
        .expect("content should exist");
    let application_json = content
        .get("application/json")
        .expect("application/json should exist");

    // Verify schema reference
    let schema = application_json.get("schema").expect("schema should exist");
    let schema_ref = schema.get("$ref").expect("schema should have $ref");
    let schema_ref_str = schema_ref.as_str().expect("$ref should be a string");

    // Should reference VersioningError
    assert_eq!(
        schema_ref_str, "#/components/schemas/VersioningError",
        "400 response should reference VersioningError schema"
    );

    // Verify VersioningError schema exists in components
    let components = spec.get("components").and_then(|c| c.as_object()).unwrap();
    let schemas = components
        .get("schemas")
        .and_then(|s| s.as_object())
        .unwrap();
    assert!(
        schemas.contains_key("VersioningError"),
        "VersioningError schema should exist in components"
    );
}
mod support;
use support::TestRoutes;
