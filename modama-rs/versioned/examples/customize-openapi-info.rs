//! Example: Customizing OpenAPI info after generation
//!
//! Run with: `cargo run -p versioned --example customize-openapi-info`

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use versioned::{VersionedRouterBuilder, openapi_from_handler, versions};

#[derive(Serialize, Deserialize, ToSchema)]
struct Greeting {
    message: String,
}

#[utoipa::path(
    get,
    path = "/greeting/{name}",
    params(("name" = String, Path, description = "Name to greet")),
    responses((status = 200, description = "Greeting returned", body = Greeting))
)]
async fn greeting_handler(
    axum::extract::Path(name): axum::extract::Path<String>,
) -> axum::Json<Greeting> {
    axum::Json(Greeting {
        message: format!("Hello, {}!", name),
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build the versioned router
    let builder = VersionedRouterBuilder::<()>::new().route(
        versions!("1.0.0.."),
        openapi_from_handler!(greeting_handler),
    );

    // Generate the base OpenAPI spec
    let mut openapi = builder.generate_openapi(semver::Version::new(1, 0, 0))?;

    openapi.info.title = "My Custom API".to_string();
    openapi.info.description = Some("A versioned API with custom info".to_string());
    openapi.info.terms_of_service = Some("https://example.com/terms".to_string());

    println!("Title: {}", openapi.info.title);
    println!("Description: {:?}", openapi.info.description);
    println!("Terms of Service: {:?}", openapi.info.terms_of_service);

    let json = serde_json::to_string_pretty(&openapi)?;
    println!("Generated OpenAPI info (JSON):\n{}", json);

    Ok(())
}
