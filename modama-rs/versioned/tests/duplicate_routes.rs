#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Tests for duplicate route detection in OpenAPI generation

use axum::{
    Json,
    body::Body,
    http::{Request, StatusCode},
};
use semver::Version;
use serde::{Deserialize, Serialize};
use tower::ServiceExt;
use utoipa::ToSchema;
use versioned::{
    PathMetadata, RouteConfigError, VersionedRouterBuilder, openapi_from_handler, versions,
};

#[derive(Serialize, Deserialize, ToSchema)]
struct Response {
    message: String,
}

#[utoipa::path(
    get,
    path = "/users",
    responses((status = 200, body = Response)),
    tags = ["users"]
)]
async fn users_v1() -> Json<Response> {
    Json(Response {
        message: "v1".to_string(),
    })
}

#[utoipa::path(
    get,
    path = "/users",
    responses((status = 200, body = Response)),
    tags = ["users"]
)]
async fn users_v1_duplicate() -> Json<Response> {
    Json(Response {
        message: "duplicate v1".to_string(),
    })
}

#[utoipa::path(post, path = "/users", responses((status = 200, body = Response)))]
async fn create_user() -> Json<Response> {
    users_v1().await
}

#[utoipa::path(method(get, post), path = "/users")]
async fn read_or_create_user() -> Json<Response> {
    users_v1().await
}

#[utoipa::path(get, path = "/users/{id}", params(("id" = String, Path)))]
async fn user_by_id() {}

#[utoipa::path(get, path = "/users/{name}", params(("name" = String, Path)))]
async fn user_by_name() {}

#[tokio::test]
async fn test_duplicate_routes_returns_error() {
    // Two handlers for the same path with overlapping version ranges
    let (meta1, handler1) = openapi_from_handler!(users_v1);
    let (meta2, handler2) = openapi_from_handler!(users_v1_duplicate);

    let builder = VersionedRouterBuilder::<()>::new()
        .route_with_openapi(versions!("1.0.0.."), handler1, meta1)
        .route_with_openapi(versions!("1.0.0.."), handler2, meta2);

    let expected = RouteConfigError::Conflict {
        first_path: "/users".to_owned(),
        second_path: "/users".to_owned(),
        method: Some("GET".to_owned()),
        first_range: versions!("1.0.0.."),
        second_range: versions!("1.0.0.."),
    };

    match builder.generate_openapi(Version::new(1, 0, 0)) {
        Err(error) => assert_eq!(error, expected.clone()),
        Ok(_) => panic!("expected OpenAPI generation to reject conflicting routes"),
    }
    match builder.build(()) {
        Err(error) => assert_eq!(error, expected),
        Ok(_) => panic!("expected build to reject conflicting routes"),
    }
}

#[tokio::test]
async fn distinct_methods_on_the_same_path_share_runtime_and_openapi_identity() {
    let builder = VersionedRouterBuilder::<()>::new()
        .route(versions!("1.0.0.."), openapi_from_handler!(users_v1))
        .route(versions!("1.0.0.."), openapi_from_handler!(create_user));

    let cached = builder.clone().build(()).expect("GET and POST may coexist");
    let runtime = cached.router_for_version(&Version::new(1, 0, 0)).await;
    for method in ["GET", "POST"] {
        let response = runtime
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/users")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    let document = builder
        .generate_openapi(Version::new(1, 0, 0))
        .expect("GET and POST should merge in OpenAPI");
    let item = document.paths.paths.get("/users").unwrap();
    assert!(item.get.is_some());
    assert!(item.post.is_some());
}

#[test]
fn multi_method_intersection_reports_the_intersecting_method() {
    let builder = VersionedRouterBuilder::<()>::new()
        .route(
            versions!("1.0.0.."),
            openapi_from_handler!(read_or_create_user),
        )
        .route(versions!("1.0.0.."), openapi_from_handler!(create_user));

    match builder.build(()) {
        Err(RouteConfigError::Conflict { method, .. }) => {
            assert_eq!(method.as_deref(), Some("POST"));
        }
        _ => panic!("POST intersection should conflict"),
    }
}

#[test]
fn equivalent_parameter_paths_conflict_regardless_of_method() {
    let builder = VersionedRouterBuilder::<()>::new()
        .route(versions!("1.0.0.."), openapi_from_handler!(user_by_id))
        .route(versions!("1.0.0.."), openapi_from_handler!(user_by_name));

    assert!(matches!(
        builder.build(()),
        Err(RouteConfigError::Conflict { method: None, .. })
    ));
}

#[test]
fn touching_ranges_do_not_conflict() {
    let builder = VersionedRouterBuilder::<()>::new()
        .route(versions!("1.0.0..2.0.0"), openapi_from_handler!(users_v1))
        .route(
            versions!("2.0.0.."),
            openapi_from_handler!(users_v1_duplicate),
        );
    builder
        .clone()
        .build(())
        .expect("half-open ranges that only touch are disjoint");

    for (version, operation_id) in [
        (Version::new(1, 0, 0), "users_v1"),
        (Version::new(2, 0, 0), "users_v1_duplicate"),
    ] {
        let document = builder.generate_openapi(version).unwrap();
        assert_eq!(
            document.paths.paths["/users"]
                .get
                .as_ref()
                .and_then(|operation| operation.operation_id.as_deref()),
            Some(operation_id)
        );
    }
}

#[test]
fn exact_versions_overlap_only_the_version_they_name() {
    let conflicting = VersionedRouterBuilder::<()>::new()
        .route(versions!("1.5.0"), openapi_from_handler!(users_v1))
        .route(
            versions!("1.0.0..2.0.0"),
            openapi_from_handler!(users_v1_duplicate),
        );
    assert!(matches!(
        conflicting.build(()),
        Err(RouteConfigError::Conflict { .. })
    ));

    VersionedRouterBuilder::<()>::new()
        .route(versions!("1.5.0"), openapi_from_handler!(users_v1))
        .route(
            versions!("1.5.1"),
            openapi_from_handler!(users_v1_duplicate),
        )
        .build(())
        .expect("different exact versions are disjoint");
}

#[test]
fn malformed_paths_are_structured_errors_for_build_and_openapi() {
    let metadata = PathMetadata {
        path: "users/:id".to_owned(),
        operation: utoipa::openapi::path::OperationBuilder::new().build(),
        tags: vec![],
        methods: vec![utoipa::openapi::path::HttpMethod::Get],
        schemas: vec![],
    };
    let builder = VersionedRouterBuilder::<()>::new().route_with_openapi(
        versions!("1.0.0.."),
        axum::routing::get(users_v1),
        metadata,
    );

    for error in [
        builder.clone().build(()).err(),
        builder.generate_openapi(Version::new(1, 0, 0)).err(),
    ] {
        assert!(matches!(
            error,
            Some(RouteConfigError::InvalidPath { ref path, .. }) if path == "users/:id"
        ));
    }
}
