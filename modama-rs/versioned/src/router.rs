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
//! use versioned::{VersionedRouterBuilder, versioned_route, versions};
//!
//! #[utoipa::path(get, path = "/greeting")]
//! async fn handler_v1() -> &'static str { "v1" }
//! #[utoipa::path(get, path = "/greeting")]
//! async fn handler_v2() -> &'static str { "v2" }
//!
//! let builder = VersionedRouterBuilder::new()
//!     .route(versions!("1.0.0..2.0.0"), versioned_route!(handler_v1))
//!     .route(versions!("2.0.0.."), versioned_route!(handler_v2));
//!
//! let router = builder.build(())?;
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

/// A versioned route with private OpenAPI metadata.
///
/// Values are created by [`versioned_route!`](crate::versioned_route), rather
/// than by manually constructing metadata.
#[derive(Clone)]
pub struct VersionedRoute<S> {
    handler: MethodRouter<S>,
    metadata: RouteMetadata,
}

#[derive(Clone)]
struct RouteMetadata {
    path: String,
    operation: utoipa::openapi::path::Operation,
    #[allow(dead_code)]
    tags: Vec<&'static str>,
    methods: Vec<utoipa::openapi::path::HttpMethod>,
    schemas: Vec<(String, RefOr<Schema>)>,
}

impl<S> VersionedRoute<S> {
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn __from_parts(
        handler: MethodRouter<S>,
        path: String,
        operation: utoipa::openapi::path::Operation,
        tags: Vec<&'static str>,
        methods: Vec<utoipa::openapi::path::HttpMethod>,
        schemas: Vec<(String, RefOr<Schema>)>,
    ) -> Self {
        Self {
            handler,
            metadata: RouteMetadata {
                path,
                operation,
                tags,
                methods,
                schemas,
            },
        }
    }
}

/// One normalized route registration used by runtime routing and OpenAPI.
#[derive(Clone)]
struct RegisteredRoute<S> {
    range: VersionRange,
    route: VersionedRoute<S>,
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
    fn new(range: VersionRange, route: VersionedRoute<S>) -> Self {
        Self { range, route }
    }

    fn path(&self) -> &str {
        &self.route.metadata.path
    }

    fn methods(&self) -> &[utoipa::openapi::path::HttpMethod] {
        &self.route.metadata.methods
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
    pub fn version_header_name(mut self, name: impl Into<String>) -> Self {
        self.header_name = name.into();
        self
    }

    /// Add a route with metadata extracted by [`versioned_route!`].
    #[must_use]
    pub fn route(mut self, range: VersionRange, route: VersionedRoute<S>) -> Self {
        self.routes.push(RegisteredRoute::new(range, route));
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
            router = router.route(route.path(), route.route.handler.clone());
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
                    .entry(route.route.metadata.path.clone())
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
            let metadata = &route.route.metadata;
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

    /// Build the versioned Axum router.
    #[allow(clippy::result_large_err)]
    pub fn build(self, state: S) -> Result<Router<()>, RouteConfigError> {
        self.validate()?;
        let dispatch = Arc::new(VersionedRouterState::new(self, state));
        Ok(Router::new().layer(axum::middleware::from_fn_with_state(
            dispatch,
            version_dispatch,
        )))
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

/// Private dispatch state used by the returned Axum router.
#[derive(Clone)]
struct VersionedRouterState<S> {
    builder: VersionedRouterBuilder<S>,
    state: S,
    cache: Arc<Mutex<HashMap<RouteSetKey, Router<()>>>>,
    header_name: String,
}

impl<S: Clone + Send + Sync + 'static> VersionedRouterState<S> {
    fn new(builder: VersionedRouterBuilder<S>, state: S) -> Self {
        let header_name = builder.header_name.clone();
        Self {
            builder,
            state,
            cache: Arc::new(Mutex::new(HashMap::new())),
            header_name,
        }
    }

    async fn router_for_version(&self, version: &Version) -> Router<()> {
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

    fn version_is_supported(&self, version: &Version) -> bool {
        self.builder
            .routes
            .iter()
            .any(|route| route.matches(version))
    }
}

async fn version_dispatch<S: Clone + Send + Sync + 'static>(
    State(cached): State<Arc<VersionedRouterState<S>>>,
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

/// Macro to construct a versioned route from a utoipa path handler.
///
/// This macro extracts path, operation, tags, methods, and schemas from a utoipa
/// path handler and returns an opaque [`VersionedRoute`].
///
/// # Example
///
/// ```rust
/// use versioned::{VersionedRouterBuilder, versioned_route, versions};
///
/// #[utoipa::path(get, path="/")]
/// async fn my_handler() -> &'static str {
///     "foobar"
/// }
///
/// let builder = VersionedRouterBuilder::<()>::new()
///     .route(versions!("1.0.0.."), versioned_route!(my_handler));
/// ```
#[macro_export]
macro_rules! versioned_route {
    ($handler:ident) => {{
        use axum::routing::{delete, get, head, options, patch, post, put, trace};
        $crate::__paste::paste! {
            // Get the methods from the utoipa Path trait
            let methods = <[<__path_ $handler>] as utoipa::Path>::methods();

            // Collect schemas from the SchemaReferences trait
            let mut schemas = Vec::new();
            <[<__path_ $handler>] as utoipa::__dev::SchemaReferences>::schemas(&mut schemas);

            // Create the metadata
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

            $crate::router::VersionedRoute::__from_parts(
                method_router.unwrap_or_else(|| get($handler)),
                <[<__path_ $handler>] as utoipa::Path>::path().to_string(),
                <[<__path_ $handler>] as utoipa::Path>::operation(),
                <[<__path_ $handler>] as utoipa::__dev::Tags>::tags(),
                methods.to_vec(),
                schemas,
            )
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions;
    use axum::routing::get;

    async fn handler() -> &'static str {
        "ok"
    }

    fn builder() -> VersionedRouterBuilder<()> {
        VersionedRouterBuilder::new().route(
            versions!("1.0.0..2.0.0"),
            VersionedRoute::__from_parts(
                get(handler),
                "/health".to_owned(),
                utoipa::openapi::path::OperationBuilder::new().build(),
                vec![],
                vec![utoipa::openapi::path::HttpMethod::Get],
                vec![],
            ),
        )
    }

    #[tokio::test]
    async fn equivalent_versions_share_a_cache_entry() {
        let state = VersionedRouterState::new(builder(), ());
        let _ = state.router_for_version(&Version::new(1, 1, 0)).await;
        let _ = state.router_for_version(&Version::new(1, 9, 0)).await;
        assert_eq!(state.cache.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn unsupported_versions_do_not_create_cache_entries() {
        let state = VersionedRouterState::new(builder(), ());
        let _ = state.router_for_version(&Version::new(9, 0, 0)).await;
        assert!(state.cache.lock().await.is_empty());
    }
}
