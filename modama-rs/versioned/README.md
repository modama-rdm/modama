# versioned

Header-based API versioning for Axum/Utoipa with semver support and automatic OpenAPI generation.

## Overview

The `versioned` crate provides a flexible, middleware-based approach to API versioning that:
- Accepts client version declarations via `X-API-Version` header
- Selects the most compatible handler version at runtime
- Returns 400 Bad Request for unsupported versions
- Adds version extensions to OpenAPI documentation
- Supports all Axum extractors (Path, Json, State, etc.)

## Features

- Header-based version negotiation (`X-API-Version: 1.0.0`)
- Semver-compatible version parsing and comparison
- Version ranges: `"1.0.0..2.0.0"` or `"2.0.0.."`
- Automatic OpenAPI generation from `#[utoipa::path]` handlers

## Quick Start

### Basic Versioned Router Setup

```rust
use versioned::{VersionedRouterBuilder, versions, versioned_route};
use axum::routing::get;

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
    let builder = VersionedRouterBuilder::new()
        .route(versions!("1.0.0..2.0.0"), versioned_route!(greeting_v1))
        .route(versions!("2.0.0.."), versioned_route!(greeting_v2));

    let router = builder.build(()).unwrap();
    
    // router is now a ready-to-use Axum Router with version dispatch middleware
}
```

## Version Range Syntax

| Syntax | Meaning |
|--------|---------|
| `"1.0.0"` | Exactly version 1.0.0 |
| `"1.0.0.."` | All versions ≥ 1.0.0 |
| `"1.0.0..2.0.0"` | Versions ≥ 1.0.0 and < 2.0.0 |
| `"2.0.0..3.0.0"` | Versions ≥ 2.0.0 and < 3.0.0 |

API versions must use plain `X.Y.Z` syntax. Prerelease and build modifiers are not supported.
For bounded ranges, the start must be strictly less than the end.

Use the `versions!()` macro for compile-time validation:

```rust
use versioned::versions;

let range = versions!("1.0.0..2.0.0"); // Compile-time parsing
```

## OpenAPI Generation

### Automatic Schema Collection

The `versioned_route!` macro automatically extracts:
- Path template
- HTTP method(s)
- Request/response schemas
- Tags
- Operation metadata

```rust
use axum::{extract::Path, Json};

#[derive(serde::Serialize)]
struct User { /* ... */ }

#[utoipa::path(
    get,
    path = "/users/{id}",
    params(("id" = u32, Path, description = "User ID")),
    responses(
        (status = 200, description = "User found", body = User),
        (status = 400, description = "Versioning error", body = VersioningError)
    )
)]
async fn get_user(Path(id): Path<u32>) -> Json<User> {
    // ...
}

let builder = VersionedRouterBuilder::new()
    .route(versions!("1.0.0.."), versioned_route!(get_user));

// Generate OpenAPI spec
let openapi = builder.generate_openapi(semver::Version::new(1, 0, 0))?;
```

### Exporting OpenAPI

```rust
use serde_json::to_string_pretty;

let openapi = builder.generate_openapi(semver::Version::new(1, 0, 0))?;
let json = to_string_pretty(&openapi).unwrap();
std::fs::write("openapi.json", json).unwrap();
```

### Version Extensions

Each operation includes version metadata:

```yaml
paths:
  /users/{id}:
    get:
      x-introduced-in-version: "1.0.0"
      x-removed-in-version: "2.0.0"  # If applicable
```

## Error Handling

All version negotiation errors return **400 Bad Request** with structured JSON.
The error enum variants are documented in the OpenAPI schema.

## Applying Middleware

The builder returns a normal Axum router, so you can use the usual middleware
facilities directly:

```rust
use axum::middleware;
use tower_http::trace::TraceLayer;

let app = builder.build(())?
    .layer(TraceLayer::new_for_http())
    .layer(middleware::from_fn(auth_middleware));
```

Layers added with `.layer()` wrap the router and execute before the version dispatch middleware inside.

## Examples

The crate includes examples for customizing OpenAPI metadata:

```bash
cargo run -p versioned --example customize-openapi-info
cargo run -p versioned --example customize-openapi-info-builders
```

## Design decisions

- OpenAPI spec includes only **one version** of each endpoint; it's meant to be used to generate a client for the latest version
- Macro requires handlers to use `#[utoipa::path]` for OpenAPI metadata

## Development

```bash
# Run the versioning crate tests and examples
cargo test -p version-range -p versions-macro -p versioned
cargo check -p versioned --examples
cargo fmt --all -- --check
```
