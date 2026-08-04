#![allow(dead_code)]

use axum::routing::MethodRouter;
use versioned::{PathMetadata, VersionRange, VersionedRouterBuilder};

pub trait TestRoutes<S> {
    fn route_get(
        self,
        path: impl Into<String>,
        range: VersionRange,
        handler: MethodRouter<S>,
    ) -> Self;

    fn route_post(
        self,
        path: impl Into<String>,
        range: VersionRange,
        handler: MethodRouter<S>,
    ) -> Self;
}

impl<S: Clone + Send + Sync + 'static> TestRoutes<S> for VersionedRouterBuilder<S> {
    fn route_get(
        self,
        path: impl Into<String>,
        range: VersionRange,
        handler: MethodRouter<S>,
    ) -> Self {
        self.route_with_openapi(
            range,
            handler,
            metadata(path, utoipa::openapi::path::HttpMethod::Get),
        )
    }

    fn route_post(
        self,
        path: impl Into<String>,
        range: VersionRange,
        handler: MethodRouter<S>,
    ) -> Self {
        self.route_with_openapi(
            range,
            handler,
            metadata(path, utoipa::openapi::path::HttpMethod::Post),
        )
    }
}

fn metadata(path: impl Into<String>, method: utoipa::openapi::path::HttpMethod) -> PathMetadata {
    PathMetadata {
        path: path.into(),
        operation: utoipa::openapi::path::OperationBuilder::new().build(),
        tags: vec![],
        methods: vec![method],
        schemas: vec![],
    }
}
