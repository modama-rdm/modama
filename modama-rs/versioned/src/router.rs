//! Version-aware router that builds Axum routers per-request based on API version.
//!
//! # Overview
//!
//! This module provides a versioned routing system that:
//! - Stores routes with version ranges
//! - Builds Axum routers dynamically per request based on the `X-API-Version` header
//! - Caches routers per version for performance
//! - Supports all Axum extractors (Path, Json, State, etc.)
//!
//! # Example
//!
//! ```rust,no_run
//! use versioned::{VersionedRouterBuilder, versions, openapi_from_handler};
//!
//! #[utoipa::path(get, path = "/greeting")]
//! async fn handler_v1() -> &'static str { "v1" }
//! #[utoipa::path(get, path = "/greeting")]
//! async fn handler_v2() -> &'static str { "v2" }
//!
//! let builder = VersionedRouterBuilder::new()
//!     .route(versions!("1.0.0..2.0.0"), openapi_from_handler!(handler_v1))
//!     .route(versions!("2.0.0.."), openapi_from_handler!(handler_v2));
//!
//! let router = builder.build(()).unwrap().into_axum_router();
//! ```

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::MethodRouter,
};
use semver::Version;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::ServiceExt;
use utoipa::openapi::{RefOr, schema::Schema};

use crate::{
    VersionRange,
    error_response::{RouteConfigError, VersioningError},
};

/// Metadata extracted from a utoipa path handler
#[derive(Clone)]
pub struct PathMetadata {
    pub path: String,
    pub operation: utoipa::openapi::path::Operation,
    pub tags: Vec<&'static str>,
    pub methods: Vec<utoipa::openapi::path::HttpMethod>,
    pub schemas: Vec<(String, RefOr<Schema>)>,
}

/// One normalized route registration used by runtime routing and OpenAPI.
#[derive(Clone)]
struct RegisteredRoute<S> {
    range: VersionRange,
    handler: MethodRouter<S>,
    metadata: PathMetadata,
}

struct CollectedSchema {
    schema: RefOr<Schema>,
    source: String,
}

#[allow(clippy::result_large_err)]
fn collect_schema(
    schemas: &mut HashMap<String, CollectedSchema>,
    name: String,
    schema: RefOr<Schema>,
    source: String,
) -> Result<(), RouteConfigError> {
    if let Some(existing) = schemas.get(&name) {
        if existing.schema != schema {
            return Err(RouteConfigError::SchemaConflict {
                schema_name: name,
                first_source: existing.source.clone(),
                second_source: source,
            });
        }
        return Ok(());
    }

    schemas.insert(name, CollectedSchema { schema, source });
    Ok(())
}

impl<S> RegisteredRoute<S> {
    fn new(range: VersionRange, handler: MethodRouter<S>, metadata: PathMetadata) -> Self {
        Self {
            range,
            handler,
            metadata,
        }
    }

    fn path(&self) -> &str {
        &self.metadata.path
    }

    fn methods(&self) -> &[utoipa::openapi::path::HttpMethod] {
        &self.metadata.methods
    }

    fn matches(&self, version: &Version) -> bool {
        self.range.contains(version)
    }

    fn intersecting_method(&self, other: &Self) -> Option<&utoipa::openapi::path::HttpMethod> {
        self.methods()
            .iter()
            .find(|method| other.methods().contains(method))
    }
}

/// Builder for versioned routes
#[derive(Clone)]
pub struct VersionedRouterBuilder<S> {
    routes: Vec<RegisteredRoute<S>>,
    header_name: String,
}

impl<S: Clone + Send + Sync + 'static> VersionedRouterBuilder<S> {
    /// Create a new versioned router builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            header_name: "X-API-Version".to_string(),
        }
    }

    /// Set the header name for version extraction
    #[must_use]
    pub fn header_name(mut self, name: impl Into<String>) -> Self {
        self.header_name = name.into();
        self
    }

    /// Add a route with OpenAPI metadata
    #[must_use]
    pub fn route_with_openapi(
        mut self,
        range: VersionRange,
        handler: MethodRouter<S>,
        metadata: PathMetadata,
    ) -> Self {
        self.routes
            .push(RegisteredRoute::new(range, handler, metadata));
        self
    }

    /// Add a route with OpenAPI metadata from a handler using the openapi_from_handler macro
    ///
    /// This is a convenience method that combines the macro call with route registration.
    /// Note: This method is designed for use with `VersionedRouterBuilder<()>`.
    ///
    /// Example:
    /// ```rust
    /// use versioned::{VersionedRouterBuilder, versions, openapi_from_handler};
    ///
    /// #[utoipa::path(get, path="/")]
    /// async fn my_handler() -> &'static str {
    ///     "foobar"
    /// }
    ///
    /// let builder = VersionedRouterBuilder::<()>::new()
    ///     .route(versions!("1.0.0.."), openapi_from_handler!(my_handler));
    /// ```
    #[must_use]
    pub fn route(
        mut self,
        range: VersionRange,
        handler_tuple: (PathMetadata, MethodRouter<S>),
    ) -> Self {
        let (metadata, handler) = handler_tuple;
        self.routes
            .push(RegisteredRoute::new(range, handler, metadata));
        self
    }

    fn route_set_for_version(&self, version: &Version) -> RouteSetKey {
        RouteSetKey(
            self.routes
                .iter()
                .enumerate()
                .filter_map(|(index, route)| route.matches(version).then_some(index))
                .collect(),
        )
    }

    /// Build a router for a set of matching routes with state.
    fn build_for_route_set(&self, route_set: &RouteSetKey, state: &S) -> Router<()> {
        let mut router = Router::new();
        for &index in &route_set.0 {
            let route = &self.routes[index];
            router = router.route(route.path(), route.handler.clone());
        }
        router.with_state(state.clone())
    }

    /// Generate OpenAPI specification from all routes
    #[allow(clippy::result_large_err)]
    pub fn generate_openapi(
        &self,
        latest_version: semver::Version,
    ) -> Result<utoipa::openapi::OpenApi, RouteConfigError> {
        use std::collections::HashMap;
        use utoipa::openapi::{
            InfoBuilder, OpenApiBuilder, PathsBuilder, extensions::ExtensionsBuilder,
            path::PathItemBuilder,
        };

        let mut paths_builder = PathsBuilder::new();
        let mut path_items: HashMap<String, PathItemBuilder> = HashMap::new();
        let mut schemas_map: HashMap<String, CollectedSchema> = HashMap::new();

        self.validate()?;

        // First pass: collect all routes and track the earliest introduction version per path
        let mut path_intro_versions: HashMap<String, Version> = HashMap::new();
        for route in self
            .routes
            .iter()
            .filter(|route| route.matches(&latest_version))
        {
            let range = &route.range;
            if let Some(start) = range.exact_version().or_else(|| range.start()) {
                path_intro_versions
                    .entry(route.metadata.path.clone())
                    .and_modify(|existing| {
                        if start < existing {
                            *existing = start.clone();
                        }
                    })
                    .or_insert_with(|| start.clone());
            }
        }

        // Second pass: generate path items with correct x-introduced-in-version
        for route in self
            .routes
            .iter()
            .filter(|route| route.matches(&latest_version))
        {
            let metadata = &route.metadata;
            let range = &route.range;

            // Get the earliest introduction version for this path
            let intro_version = path_intro_versions.get(&metadata.path);

            // Build version extensions for this route
            let mut ext_builder = ExtensionsBuilder::new();

            // Add x-introduced-in-version (use the earliest version for this path)
            if let Some(start) = intro_version {
                ext_builder = ext_builder.add("x-introduced-in-version", start.to_string());
            }

            // Add x-removed-in-version (only if there's an end)
            if let Some(end) = range.end() {
                ext_builder = ext_builder.add("x-removed-in-version", end.to_string());
            }

            let extensions = ext_builder.build();

            let mut path_item_builder = path_items.remove(&metadata.path).unwrap_or_default();

            for method in &metadata.methods {
                // Clone the base operation and add extensions
                let mut operation = metadata.operation.clone();
                operation.extensions = Some(extensions.clone());

                // Add 400 response for versioning errors (middleware can return this before route execution)
                use utoipa::openapi::content::ContentBuilder;
                use utoipa::openapi::response::{ResponseBuilder, ResponsesBuilder};
                use utoipa::openapi::schema::Ref;

                let versioning_error_ref = "#/components/schemas/VersioningError";
                let error_response = ResponseBuilder::new()
                    .description("Version negotiation failed")
                    .content(
                        "application/json",
                        ContentBuilder::new()
                            .schema(Some(Ref::new(versioning_error_ref)))
                            .build(),
                    )
                    .build();

                // Start building responses with the 400 response
                let mut responses_builder = ResponsesBuilder::new().response("400", error_response);

                // Add existing responses from the operation
                for (code, response) in operation.responses.responses.iter() {
                    responses_builder = responses_builder.response(code, response.clone());
                }

                operation.responses = responses_builder.build();

                path_item_builder = path_item_builder.operation(method.clone(), operation);
            }

            path_items.insert(metadata.path.clone(), path_item_builder);

            // Collect all schemas from this route
            for (name, schema) in &metadata.schemas {
                collect_schema(
                    &mut schemas_map,
                    name.clone(),
                    schema.clone(),
                    metadata.path.clone(),
                )?;
            }
        }

        use crate::error_response::VersioningError;
        collect_schema(
            &mut schemas_map,
            <VersioningError as utoipa::ToSchema>::name().into_owned(),
            <VersioningError as utoipa::PartialSchema>::schema(),
            "versioning middleware".to_owned(),
        )?;

        for (path, path_item) in path_items {
            paths_builder = paths_builder.path(path, path_item.build());
        }

        let mut openapi_builder = OpenApiBuilder::new()
            .info(
                InfoBuilder::new()
                    .title("Versioned API")
                    .version(latest_version.to_string())
                    .build(),
            )
            .paths(paths_builder.build());

        // Build components section
        let mut components_builder = utoipa::openapi::ComponentsBuilder::new();

        // Add all collected schemas from routes
        for (name, collected) in schemas_map {
            components_builder = components_builder.schema(name, collected.schema);
        }

        // Only set components if we have any schemas
        let components = Some(components_builder.build());
        openapi_builder = openapi_builder.components(components);

        Ok(openapi_builder.build())
    }

    /// Create a cached versioned router
    #[allow(clippy::result_large_err)]
    pub fn build(self, state: S) -> Result<CachedVersionedRouter<S>, RouteConfigError> {
        self.validate()?;
        Ok(CachedVersionedRouter::new(self, state))
    }

    #[allow(clippy::result_large_err)]
    fn validate(&self) -> Result<(), RouteConfigError> {
        for route in &self.routes {
            validate_path(route.path())?;
        }
        for (index, first) in self.routes.iter().enumerate() {
            for second in &self.routes[index + 1..] {
                if !first.range.overlaps(&second.range) {
                    continue;
                }
                if first.path() == second.path() {
                    if let Some(method) = first.intersecting_method(second) {
                        return Err(RouteConfigError::Conflict {
                            first_path: first.path().to_owned(),
                            second_path: second.path().to_owned(),
                            method: Some(method_name(method).to_owned()),
                            first_range: first.range.clone(),
                            second_range: second.range.clone(),
                        });
                    }
                } else if paths_conflict(first.path(), second.path()) {
                    return Err(RouteConfigError::Conflict {
                        first_path: first.path().to_owned(),
                        second_path: second.path().to_owned(),
                        method: None,
                        first_range: first.range.clone(),
                        second_range: second.range.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[allow(clippy::result_large_err)]
fn validate_path(path: &str) -> Result<(), RouteConfigError> {
    let invalid = |reason: String| RouteConfigError::InvalidPath {
        path: path.to_owned(),
        reason,
    };
    if !path.starts_with('/') {
        return Err(invalid("paths must start with '/'".to_owned()));
    }
    if path
        .split('/')
        .any(|segment| segment.starts_with(':') || segment.starts_with('*'))
    {
        return Err(invalid(
            "legacy parameters beginning with ':' or '*' are not supported; use '{param}' or '{*param}'"
                .to_owned(),
        ));
    }
    let mut router = matchit::Router::new();
    router
        .insert(path, ())
        .map_err(|error| invalid(error.to_string()))?;
    Ok(())
}

fn paths_conflict(first: &str, second: &str) -> bool {
    let mut router = matchit::Router::new();
    router.insert(first, ()).is_ok() && router.insert(second, ()).is_err()
}

fn method_name(method: &utoipa::openapi::path::HttpMethod) -> &'static str {
    use utoipa::openapi::path::HttpMethod;
    match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Options => "OPTIONS",
        HttpMethod::Head => "HEAD",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Trace => "TRACE",
    }
}

impl<S: Clone + Send + Sync + 'static> Default for VersionedRouterBuilder<S> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RouteSetKey(Vec<usize>);

impl RouteSetKey {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Cached versioned router that builds and caches one router per matching route set.
#[derive(Clone)]
pub struct CachedVersionedRouter<S> {
    builder: VersionedRouterBuilder<S>,
    state: S,
    cache: Arc<Mutex<HashMap<RouteSetKey, Router<()>>>>,
    header_name: String,
}

impl<S: Clone + Send + Sync + 'static> CachedVersionedRouter<S> {
    fn new(builder: VersionedRouterBuilder<S>, state: S) -> Self {
        let header_name = builder.header_name.clone();
        Self {
            builder,
            state,
            cache: Arc::new(Mutex::new(HashMap::new())),
            header_name,
        }
    }

    pub async fn router_for_version(&self, version: &Version) -> Router<()> {
        let route_set = self.builder.route_set_for_version(version);

        // Keep the public API's empty 404 router behavior, but do not allow
        // arbitrary unsupported versions to consume cache entries.
        if route_set.is_empty() {
            return self.builder.build_for_route_set(&route_set, &self.state);
        }

        {
            let cache = self.cache.lock().await;
            if let Some(router) = cache.get(&route_set) {
                return router.clone();
            }
        }

        // Router construction happens outside the cache lock. If concurrent
        // callers race, the first insertion becomes canonical for all callers.
        let router = self.builder.build_for_route_set(&route_set, &self.state);
        let mut cache = self.cache.lock().await;
        cache.entry(route_set).or_insert(router).clone()
    }

    pub fn into_axum_router(self) -> Router {
        Router::new().layer(axum::middleware::from_fn_with_state(
            Arc::new(self),
            version_dispatch,
        ))
    }

    /// Check if a version has any matching routes
    pub fn version_is_supported(&self, version: &Version) -> bool {
        self.builder
            .routes
            .iter()
            .any(|route| route.matches(version))
    }

    pub fn header_name(&self) -> &str {
        &self.header_name
    }
}

async fn version_dispatch<S: Clone + Send + Sync + 'static>(
    State(cached): State<Arc<CachedVersionedRouter<S>>>,
    mut req: Request,
    _next: Next,
) -> Response {
    let header_name = cached.header_name.clone();

    // Extract supported version ranges from the builder
    let supported_ranges: Vec<String> = cached
        .builder
        .routes
        .iter()
        .map(|r| r.range.to_string())
        .collect();

    let version = match extract_version(&req, &header_name) {
        Ok(Some(v)) => v,
        Ok(None) => {
            return VersioningError::MissingVersion {
                error: format!("Missing required header: {}", header_name),
                supported: supported_ranges.clone(),
                header: header_name,
            }
            .into_response();
        }
        Err(_) => {
            // Extract the invalid version string for the error
            let invalid_version = req
                .headers()
                .get(&header_name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown")
                .to_string();

            return VersioningError::InvalidVersion {
                error: format!("Invalid version format: {}", invalid_version),
                supported: supported_ranges.clone(),
                version: invalid_version,
                header: header_name,
            }
            .into_response();
        }
    };

    // Insert the parsed version into request extensions for inner middleware to access
    req.extensions_mut().insert(version.clone());

    // Check if the version is supported (has any matching routes)
    if !cached.version_is_supported(&version) {
        return VersioningError::UnsupportedVersion {
            error: format!("No routes match version {}", version),
            supported: supported_ranges,
            header: header_name,
        }
        .into_response();
    }

    let router = cached.router_for_version(&version).await;
    let req = req.map(Body::new);
    let resp = match router.oneshot(req).await {
        Ok(resp) => resp,
        Err(infallible) => match infallible {},
    };

    resp.map(Body::new)
}

fn extract_version(req: &Request, header_name: &str) -> Result<Option<Version>, ()> {
    req.headers()
        .get(header_name)
        .map(|v| {
            v.to_str()
                .map_err(|_| ())
                .and_then(|s| VersionRange::parse_version(s).map_err(|_| ()))
        })
        .transpose()
}

/// Macro to extract OpenAPI metadata from a utoipa path handler.
///
/// This macro extracts path, operation, tags, methods, and schemas from a utoipa
/// path handler and returns a tuple of `(PathMetadata, MethodRouter<S>)`.
///
/// # Example
///
/// ```rust
/// use versioned::{VersionedRouterBuilder, versions, openapi_from_handler};
///
/// #[utoipa::path(get, path="/")]
/// async fn my_handler() -> &'static str {
///     "foobar"
/// }
///
/// let builder = VersionedRouterBuilder::<()>::new()
///     .route(versions!("1.0.0.."), openapi_from_handler!(my_handler));
/// ```
#[macro_export]
macro_rules! openapi_from_handler {
    ($handler:ident) => {{
        use axum::routing::{delete, get, head, options, patch, post, put, trace};
        $crate::__paste::paste! {
            // Get the methods from the utoipa Path trait
            let methods = <[<__path_ $handler>] as utoipa::Path>::methods();

            // Collect schemas from the SchemaReferences trait
            let mut schemas = Vec::new();
            <[<__path_ $handler>] as utoipa::__dev::SchemaReferences>::schemas(&mut schemas);

            // Create the metadata
            let metadata = $crate::router::PathMetadata {
                path: <[<__path_ $handler>] as utoipa::Path>::path().to_string(),
                operation: <[<__path_ $handler>] as utoipa::Path>::operation(),
                tags: <[<__path_ $handler>] as utoipa::__dev::Tags>::tags(),
                methods: methods.to_vec(),
                schemas,
            };

            // Create a MethodRouter that handles ALL methods by merging them
            let mut method_router: Option<axum::routing::MethodRouter<_>> = None;
            for method in &methods {
                let router = match method {
                    utoipa::openapi::path::HttpMethod::Get => get($handler.clone()),
                    utoipa::openapi::path::HttpMethod::Post => post($handler.clone()),
                    utoipa::openapi::path::HttpMethod::Put => put($handler.clone()),
                    utoipa::openapi::path::HttpMethod::Delete => delete($handler.clone()),
                    utoipa::openapi::path::HttpMethod::Patch => patch($handler.clone()),
                    utoipa::openapi::path::HttpMethod::Options => options($handler.clone()),
                    utoipa::openapi::path::HttpMethod::Head => head($handler.clone()),
                    utoipa::openapi::path::HttpMethod::Trace => trace($handler.clone()),
                };
                method_router = Some(match method_router {
                    Some(existing) => existing.merge(router),
                    None => router,
                });
            }

            (metadata, method_router.unwrap_or_else(|| get($handler)))
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions;
    use axum::{
        Json,
        extract::{Extension, Path},
        http::{Request as HttpRequest, StatusCode},
        response::IntoResponse,
        routing::get,
    };
    use serde::{Deserialize, Serialize};

    fn test_metadata(path: &str, method: utoipa::openapi::path::HttpMethod) -> PathMetadata {
        PathMetadata {
            path: path.to_owned(),
            operation: utoipa::openapi::path::OperationBuilder::new().build(),
            tags: vec![],
            methods: vec![method],
            schemas: vec![],
        }
    }

    #[derive(Debug, Clone)]
    struct AppState {
        domain: String,
    }

    async fn handle_user_v1(Path(id): Path<u32>) -> impl IntoResponse {
        Json(UserV1 {
            id,
            name: format!("User {}", id),
        })
    }

    async fn handle_user_v2(
        Path(id): Path<u32>,
        State(state): State<AppState>,
    ) -> impl IntoResponse {
        Json(UserV2 {
            id,
            name: format!("User {}", id),
            email: format!("user{}@{}", id, state.domain),
        })
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct UserV1 {
        id: u32,
        name: String,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct UserV2 {
        id: u32,
        name: String,
        email: String,
    }

    #[tokio::test]
    async fn test_builder_pattern() {
        let builder = VersionedRouterBuilder::new()
            .route_with_openapi(
                versions!("1.0.0..2.0.0"),
                get(handle_user_v1),
                test_metadata("/users/{id}", utoipa::openapi::path::HttpMethod::Get),
            )
            .route_with_openapi(
                versions!("2.0.0.."),
                get(handle_user_v2),
                test_metadata("/users/{id}", utoipa::openapi::path::HttpMethod::Get),
            );

        let state = AppState {
            domain: "example.com".to_owned(),
        };

        let cached = builder.build(state).unwrap();

        let router_v1 = cached.router_for_version(&Version::new(1, 5, 0)).await;
        let request = HttpRequest::builder()
            .method("GET")
            .uri("/users/42")
            .body(Body::empty())
            .unwrap();

        let response = router_v1.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let user: UserV1 = serde_json::from_slice(&body).unwrap();
        assert_eq!(user.name, "User 42");

        let router_v2 = cached.router_for_version(&Version::new(2, 0, 0)).await;
        let request = HttpRequest::builder()
            .method("GET")
            .uri("/users/42")
            .body(Body::empty())
            .unwrap();

        let response = router_v2.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let user: UserV2 = serde_json::from_slice(&body).unwrap();
        assert_eq!(user.name, "User 42");
        assert_eq!(user.email, "user42@example.com");
    }

    #[tokio::test]
    async fn test_caching_uses_matching_route_sets() {
        let builder = VersionedRouterBuilder::new()
            .route_with_openapi(
                versions!("1.0.0..2.0.0"),
                get(handle_user_v1),
                test_metadata("/users/{id}", utoipa::openapi::path::HttpMethod::Get),
            )
            .route_with_openapi(
                versions!("2.0.0.."),
                get(handle_user_v2),
                test_metadata("/users/{id}", utoipa::openapi::path::HttpMethod::Get),
            );

        let state = AppState {
            domain: "example.com".to_owned(),
        };

        let cached = builder.build(state).unwrap();

        for version in [
            "1.0.0",
            "1.5.0",
            "1.5.0-alpha.1",
            "1.5.0+build.1",
            "1.5.0+build.2",
            "1.999.999",
            "2.0.0-alpha.1",
            "2.0.0",
            "2.5.0",
            "999.0.0",
        ] {
            let version = Version::parse(version).expect("test version should be valid");
            let _ = cached.router_for_version(&version).await;
        }

        let cache = cached.cache.lock().await;
        assert_eq!(cache.len(), 2);
    }

    #[tokio::test]
    async fn test_no_matching_version() {
        let builder = VersionedRouterBuilder::new().route_with_openapi(
            versions!("2.0.0.."),
            get(handle_user_v2),
            test_metadata("/users/{id}", utoipa::openapi::path::HttpMethod::Get),
        );

        let state = AppState {
            domain: "example.com".to_owned(),
        };

        let cached = builder.build(state).unwrap();

        let router = cached.router_for_version(&Version::new(1, 0, 0)).await;
        let request = HttpRequest::builder()
            .method("GET")
            .uri("/users/42")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let cache = cached.cache.lock().await;
        assert!(cache.is_empty());
    }

    #[tokio::test]
    async fn test_concurrent_equivalent_versions_share_cache_entry() {
        let builder = VersionedRouterBuilder::new().route_with_openapi(
            versions!("1.0.0..2.0.0"),
            get(handle_user_v1),
            test_metadata("/users/{id}", utoipa::openapi::path::HttpMethod::Get),
        );
        let cached = builder
            .build(AppState {
                domain: "example.com".to_owned(),
            })
            .unwrap();

        let v1 = Version::new(1, 1, 0);
        let v2 = Version::new(1, 2, 0);
        let v3 = Version::new(1, 3, 0);
        let v4 = Version::new(1, 4, 0);
        let _ = tokio::join!(
            cached.router_for_version(&v1),
            cached.router_for_version(&v2),
            cached.router_for_version(&v3),
            cached.router_for_version(&v4),
        );

        let cache = cached.cache.lock().await;
        assert_eq!(cache.len(), 1);
    }

    #[tokio::test]
    async fn test_cached_router_preserves_plain_requested_version_extension() {
        async fn version(Extension(version): Extension<Version>) -> String {
            version.to_string()
        }

        let cached = VersionedRouterBuilder::new()
            .route_with_openapi(
                versions!("1.0.0..2.0.0"),
                get(version),
                test_metadata("/version", utoipa::openapi::path::HttpMethod::Get),
            )
            .build(())
            .unwrap();
        let app = cached.clone().into_axum_router();

        for requested_version in ["1.1.0", "1.9.0"] {
            let request = HttpRequest::builder()
                .uri("/version")
                .header("X-API-Version", requested_version)
                .body(Body::empty())
                .expect("version request should be valid");
            let response = app
                .clone()
                .oneshot(request)
                .await
                .expect("version request should succeed");
            let body = axum::body::to_bytes(response.into_body(), 1024)
                .await
                .expect("version response body should be readable");

            assert_eq!(body.as_ref(), requested_version.as_bytes());
        }

        let cache = cached.cache.lock().await;
        assert_eq!(cache.len(), 1);
    }

    #[tokio::test]
    async fn test_custom_header_name() {
        let builder = VersionedRouterBuilder::new()
            .header_name("X-Custom-API-Version")
            .route_with_openapi(
                versions!("1.0.0..2.0.0"),
                get(handle_user_v1),
                test_metadata("/users/{id}", utoipa::openapi::path::HttpMethod::Get),
            )
            .route_with_openapi(
                versions!("2.0.0.."),
                get(handle_user_v2),
                test_metadata("/users/{id}", utoipa::openapi::path::HttpMethod::Get),
            );

        let state = AppState {
            domain: "example.com".to_owned(),
        };

        let cached = builder.build(state).unwrap();
        let axum_router = cached.into_axum_router();

        // Request with custom header - should use v1 handler
        let request = HttpRequest::builder()
            .method("GET")
            .uri("/users/42")
            .header("X-Custom-API-Version", "1.5.0")
            .body(Body::empty())
            .unwrap();

        let response = axum_router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let user: UserV1 = serde_json::from_slice(&body).unwrap();
        assert_eq!(user.name, "User 42");

        // Request with wrong header name - should return MissingVersion error
        let request = HttpRequest::builder()
            .method("GET")
            .uri("/users/42")
            .header("X-API-Version", "1.5.0")
            .body(Body::empty())
            .unwrap();

        let response = axum_router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["type"], "missing_version");
    }

    #[tokio::test]
    async fn test_unsupported_version_dispatch() {
        let builder = VersionedRouterBuilder::new().route_with_openapi(
            versions!("2.0.0.."),
            get(handle_user_v2),
            test_metadata("/users/{id}", utoipa::openapi::path::HttpMethod::Get),
        );

        let state = AppState {
            domain: "example.com".to_owned(),
        };

        let cached = builder.build(state).unwrap();
        let axum_router = cached.into_axum_router();

        // Request with version 1.0.0 - no matching route (only 2.0.0+ supported)
        // Now returns 400 with UnsupportedVersion error (fixed behavior)
        let request = HttpRequest::builder()
            .method("GET")
            .uri("/users/42")
            .header("X-API-Version", "1.0.0")
            .body(Body::empty())
            .unwrap();

        let response = axum_router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Verify the response contains the UnsupportedVersion error
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["type"], "unsupported_version");
        assert!(json["error"].as_str().unwrap().contains("1.0.0"));
    }

    #[tokio::test]
    // Smoke test for Default implementation
    async fn test_builder_default_smoke() {
        let builder = VersionedRouterBuilder::<()>::default();
        let _cached = builder.build(());
        // If this compiles and runs, Default works
    }

    // ============================================================================
    // Test for ToSchema on VersioningError (Issue #3)
    // ============================================================================

    #[tokio::test]
    async fn test_versioning_error_has_to_schema() {
        // This test verifies that VersioningError implements ToSchema
        // so it can be automatically included in OpenAPI specs.
        use crate::error_response::VersioningError;
        use utoipa::ToSchema;

        // This line will fail to compile if VersioningError doesn't implement ToSchema
        fn assert_to_schema<T: ToSchema>() {}
        assert_to_schema::<VersioningError>();
    }

    #[tokio::test]
    async fn test_versioning_error_in_openapi_components() {
        // This test verifies that VersioningError is included in the generated
        // OpenAPI specification components as the error response for version
        // negotiation failures.
        use crate::{VersionedRouterBuilder, versions};
        use semver::Version;

        // Create a simple handler inside the test module
        #[derive(utoipa::ToSchema, serde::Serialize)]
        struct SimpleResponse {
            data: String,
        }

        #[utoipa::path(get, path = "/simple", responses((status = 200, description = "OK", body = SimpleResponse)))]
        async fn simple_handler() -> Json<SimpleResponse> {
            Json(SimpleResponse {
                data: "test".to_string(),
            })
        }

        let (meta, handler) = openapi_from_handler!(simple_handler);

        let builder = VersionedRouterBuilder::<()>::new().route_with_openapi(
            versions!("1.0.0.."),
            handler,
            meta,
        );

        let openapi = builder.generate_openapi(Version::new(1, 0, 0)).unwrap();
        let json = serde_json::to_string_pretty(&openapi).unwrap();
        let spec: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Get the components section
        let components = spec.get("components").and_then(|c| c.as_object());

        // Components section should exist (it will have SimpleResponse schema)
        assert!(
            components.is_some(),
            "OpenAPI spec should have components section"
        );

        let schemas = components
            .unwrap()
            .get("schemas")
            .and_then(|s| s.as_object());

        // Schemas section should exist
        assert!(schemas.is_some(), "Components should have schemas section");

        let schema_names: Vec<&String> = schemas.unwrap().keys().collect();
        println!("Schemas in OpenAPI spec: {:?}", schema_names);

        // VersioningError should be in the components
        assert!(
            schema_names
                .iter()
                .any(|name| name.contains("VersioningError")),
            "VersioningError should be in OpenAPI components. Found: {:?}",
            schema_names
        );
    }

    #[tokio::test]
    async fn test_routes_include_400_versioning_error_response() {
        // This test verifies that all routes automatically include a 400 response
        // with VersioningError schema, since the versioning middleware can return
        // 400 errors before route execution (missing/invalid version headers).
        //
        // Expected: Every route should have a 400 response documented
        use crate::{VersionedRouterBuilder, versions};
        use semver::Version;

        // Create a simple handler
        #[derive(utoipa::ToSchema, serde::Serialize)]
        struct SimpleResponse {
            data: String,
        }

        #[utoipa::path(get, path = "/test", responses((status = 200, description = "OK", body = SimpleResponse)))]
        async fn test_handler() -> Json<SimpleResponse> {
            Json(SimpleResponse {
                data: "test".to_string(),
            })
        }

        let (meta, handler) = openapi_from_handler!(test_handler);

        let builder = VersionedRouterBuilder::<()>::new().route_with_openapi(
            versions!("1.0.0.."),
            handler,
            meta,
        );

        let openapi = builder.generate_openapi(Version::new(1, 0, 0)).unwrap();
        let json = serde_json::to_string_pretty(&openapi).unwrap();
        let spec: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Get the path item for /test
        let paths = spec.get("paths").and_then(|p| p.as_object()).unwrap();
        let path_item = paths.get("/test").and_then(|p| p.as_object()).unwrap();
        let get_operation = path_item.get("get").and_then(|g| g.as_object()).unwrap();
        let responses = get_operation
            .get("responses")
            .and_then(|r| r.as_object())
            .unwrap();

        // 400 response should exist
        assert!(
            responses.contains_key("400"),
            "Routes should include 400 response for versioning errors. \
             Available responses: {:?}",
            responses.keys().collect::<Vec<_>>()
        );

        // The 400 response should reference VersioningError schema
        let response_400 = responses.get("400").and_then(|r| r.as_object()).unwrap();
        if let Some(content) = response_400.get("content").and_then(|c| c.as_object()) {
            if let Some(json_content) = content.get("application/json").and_then(|j| j.as_object())
            {
                if let Some(schema) = json_content.get("schema").and_then(|s| s.as_object()) {
                    // Should reference VersioningError
                    if let Some(reference) = schema.get("$ref").and_then(|r| r.as_str()) {
                        assert!(
                            reference.contains("VersioningError"),
                            "400 response should reference VersioningError schema. Got: {}",
                            reference
                        );
                    } else {
                        panic!("400 response schema should have $ref field");
                    }
                } else {
                    panic!("400 response should have schema field");
                }
            } else {
                panic!("400 response should have application/json content");
            }
        } else {
            panic!("400 response should have content field");
        }
    }

    fn router_with_panicking_handler() -> Router {
        async fn handler() -> &'static str {
            panic!("intentional handler panic");
        }
        let builder = VersionedRouterBuilder::new().route_with_openapi(
            versions!("1.0.0.."),
            get(handler),
            test_metadata("/crash", utoipa::openapi::path::HttpMethod::Get),
        );
        let state = AppState {
            domain: "example.com".to_owned(),
        };
        builder.build(state).unwrap().into_axum_router()
    }

    fn crash_request() -> HttpRequest<Body> {
        HttpRequest::builder()
            .method("GET")
            .uri("/crash")
            .header("X-API-Version", "1.0.0")
            .body(Body::empty())
            .expect("crash request should be valid")
    }

    #[tokio::test]
    #[should_panic(expected = "intentional handler panic")]
    async fn test_handler_panic_propagates_without_recovery_layer() {
        let _ = router_with_panicking_handler()
            .oneshot(crash_request())
            .await;
    }

    #[tokio::test]
    async fn test_application_can_convert_handler_panic_to_500() {
        use tower_http::catch_panic::CatchPanicLayer;

        let response = router_with_panicking_handler()
            .layer(CatchPanicLayer::new())
            .oneshot(crash_request())
            .await
            .expect("catch-panic layer should return a response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
