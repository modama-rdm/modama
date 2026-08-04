#![allow(dead_code)]

use axum::routing::MethodRouter;
use versioned::{VersionRange, VersionedRoute, VersionedRouterBuilder};

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
        self.route(
            range,
            route(path, utoipa::openapi::path::HttpMethod::Get, handler),
        )
    }

    fn route_post(
        self,
        path: impl Into<String>,
        range: VersionRange,
        handler: MethodRouter<S>,
    ) -> Self {
        self.route(
            range,
            route(path, utoipa::openapi::path::HttpMethod::Post, handler),
        )
    }
}

fn route<S>(
    path: impl Into<String>,
    method: utoipa::openapi::path::HttpMethod,
    handler: MethodRouter<S>,
) -> VersionedRoute<S> {
    VersionedRoute::__from_parts(
        handler,
        path.into(),
        utoipa::openapi::path::OperationBuilder::new().build(),
        vec![],
        vec![method],
        vec![],
    )
}
