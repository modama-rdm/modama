use versioned::{VersionedRouterBuilder, versioned_route, versions};

#[utoipa::path(get, path = "/greeting")]
async fn greeting_v1() -> &'static str {
    "Hello from v1"
}

#[utoipa::path(get, path = "/greeting")]
async fn greeting_v2() -> &'static str {
    "Hello from v2"
}

#[tokio::main]
async fn main() {
    let router = VersionedRouterBuilder::new()
        .route(
            versions!("1.0.0..2.0.0"),
            versioned_route!(greeting_v1),
        )
        .route(
            versions!("2.0.0.."),
            versioned_route!(greeting_v2),
        )
        .build(())
        .expect("documented routes should be valid")
        ;

    let _: axum::Router = router;
}

