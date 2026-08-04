//! Example: Using utoipa builders for comprehensive OpenAPI info customization
//!
//! This demonstrates using utoipa's InfoBuilder, ContactBuilder, and LicenseBuilder
//! for type-safe, comprehensive customization.
//!
//! Run with: `cargo run -p versioned --example customize-openapi-info-builders`

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

    // Rebuild the info section with full customization
    use utoipa::openapi::{ContactBuilder, InfoBuilder, LicenseBuilder};

    let custom_info = InfoBuilder::new()
        .title("Professional API")
        .version("1.0.0")
        .description(Some(
            "A production-ready API with versioned endpoints.\n\
             Supports multiple client versions with backward compatibility.",
        ))
        .terms_of_service(Some("https://api.example.com/terms".to_string()))
        .contact(Some(
            ContactBuilder::new()
                .name(Some("API Support Team"))
                .email(Some("support@example.com"))
                .url(Some("https://example.com/support"))
                .build(),
        ))
        .license(Some(
            LicenseBuilder::new()
                .name("MIT")
                .url(Some("https://opensource.org/licenses/MIT"))
                .build(),
        ))
        .build();

    // Now apply the custom info to the actual OpenAPI spec
    let mut openapi = builder.generate_openapi(semver::Version::new(1, 0, 0))?;
    openapi.info = custom_info;

    let json = serde_json::to_string_pretty(&openapi)?;
    println!("Generated OpenAPI info (JSON):\n{}", json);

    Ok(())
}
