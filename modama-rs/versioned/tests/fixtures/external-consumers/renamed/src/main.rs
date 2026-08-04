use api_versioning::{VersionedRouterBuilder, versioned_route, versions};

#[utoipa::path(post, path = "/greeting")]
async fn greeting() -> &'static str {
    "hello"
}

#[tokio::main]
async fn main() {
    let router = VersionedRouterBuilder::new()
        .route(versions!("1.0.0.."), versioned_route!(greeting))
        .build(())
        .expect("renamed-consumer route should be valid")
        ;

    let _: axum::Router = router;
}

