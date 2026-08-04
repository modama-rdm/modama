//! Test to demonstrate the macro state type hardcoding bug
//!
//! This test should FAIL to compile, showing that openapi_from_handler!
//! hardcodes MethodRouter<()> and cannot work with custom state types.

use axum::extract::State;
use versioned::{VersionedRouterBuilder, openapi_from_handler, versions};

#[derive(Debug, Clone)]
#[expect(dead_code)] // Used in the test to verify state type inference
struct MyAppState {
    db_url: String,
}

// Handler that requires custom state
#[utoipa::path(get, path = "/health")]
async fn health_check(State(_state): State<MyAppState>) -> &'static str {
    "ok"
}

#[tokio::test]
async fn test_macro_with_custom_state() {
    // This should NOW compile because the macro infers the state type
    let builder = VersionedRouterBuilder::<MyAppState>::new()
        .route(versions!("1.0.0.."), openapi_from_handler!(health_check));

    let state = MyAppState {
        db_url: "postgres://localhost".to_string(),
    };

    let _router = builder.build(state).unwrap().into_axum_router();
}
