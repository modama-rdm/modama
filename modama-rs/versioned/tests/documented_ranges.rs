#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;
use versioned::{VersionedRouterBuilder, versioned_route, versions};

#[utoipa::path(get, path = "/greeting")]
async fn greeting_v1() -> &'static str {
    "Hello from v1"
}

#[utoipa::path(get, path = "/greeting")]
async fn greeting_v2() -> &'static str {
    "Hello from v2"
}

async fn request(version: &str) -> (StatusCode, String) {
    let app = VersionedRouterBuilder::new()
        .route(versions!("1.0.0..2.0.0"), versioned_route!(greeting_v1))
        .route(versions!("2.0.0.."), versioned_route!(greeting_v2))
        .build(())
        .expect("documented routes should be valid");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/greeting")
                .header("X-API-Version", version)
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("request should succeed");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("response body should be readable");

    (
        status,
        String::from_utf8(body.to_vec()).expect("response should be UTF-8"),
    )
}

#[tokio::test]
async fn quick_start_ranges_match_the_documentation() {
    assert_eq!(
        request("0.9.0").await.0,
        StatusCode::BAD_REQUEST,
        "versions below the first documented range are unsupported"
    );
    assert_eq!(
        request("1.5.0").await,
        (StatusCode::OK, "Hello from v1".to_string())
    );
    assert_eq!(
        request("2.0.0").await,
        (StatusCode::OK, "Hello from v2".to_string())
    );
    assert_eq!(
        request("3.0.0").await,
        (StatusCode::OK, "Hello from v2".to_string()),
        "the documented v2 range is open-ended"
    );
}
