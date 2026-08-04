use api_versioning::{VersionedRouterBuilder, openapi_from_handler, versions};

#[utoipa::path(post, path = "/greeting")]
async fn greeting() -> &'static str {
    "hello"
}

#[tokio::main]
async fn main() {
    let router = VersionedRouterBuilder::new()
        .route(versions!("1.0.0.."), openapi_from_handler!(greeting))
        .build(())
        .expect("renamed-consumer route should be valid")
        .into_axum_router();

    let _: axum::Router = router;
}

