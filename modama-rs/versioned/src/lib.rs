//! Header-based API versioning for Axum.
//!
//! # Overview
//!
//! This crate provides versioned routing that selects handlers based on the
//! `X-API-Version` header. Multiple versions of the same endpoint can coexist,
//! and the router selects the appropriate version at runtime.
//!
//! # Example
//!
//! ```rust,no_run
//! use versioned::{VersionedRouterBuilder, versioned_route, versions};
//!
//! #[utoipa::path(get, path = "/greeting")]
//! async fn greeting_v1() -> &'static str { "v1 greeting" }
//! #[utoipa::path(get, path = "/greeting")]
//! async fn greeting_v2() -> &'static str { "v2 greeting" }
//!
//! let builder = VersionedRouterBuilder::new()
//!     .route(versions!("1.0.0..2.0.0"), versioned_route!(greeting_v1))
//!     .route(versions!("2.0.0.."), versioned_route!(greeting_v2));
//!
//! let router = builder.build(())?;
//! # Ok::<(), versioned::RouteConfigError>(())
//! ```

pub mod error_response;
pub mod router;

pub use router::{VersionedRoute, VersionedRouterBuilder};
// Macro versioned_route is automatically available at crate root via #[macro_export]
pub use version_range::VersionRange;

pub use ::semver;
pub use error_response::RouteConfigError;

#[doc(hidden)]
pub use paste as __paste;
#[doc(hidden)]
pub use versions_macro::versions as __versions;

/// Construct a [`VersionRange`] from a range literal.
#[macro_export]
macro_rules! versions {
    ($range:literal) => {
        $crate::__versions!($crate, $range)
    };
}
