#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::routing::get;
use semver::Version;
use utoipa::openapi::{RefOr, path::HttpMethod, schema::Schema};
use versioned::{
    PathMetadata, RouteConfigError, VersionedRouterBuilder, error_response::VersioningError,
    versions,
};

fn schema(schema_type: &str) -> RefOr<Schema> {
    serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {
            "value": { "type": schema_type }
        }
    }))
    .unwrap()
}

fn metadata(path: &str, schemas: Vec<(String, RefOr<Schema>)>) -> PathMetadata {
    PathMetadata {
        path: path.to_owned(),
        operation: utoipa::openapi::path::OperationBuilder::new().build(),
        tags: vec![],
        methods: vec![HttpMethod::Get],
        schemas,
    }
}

fn route(
    builder: VersionedRouterBuilder<()>,
    path: &str,
    range: versioned::VersionRange,
    schemas: Vec<(String, RefOr<Schema>)>,
) -> VersionedRouterBuilder<()> {
    builder.route_with_openapi(range, get(|| async {}), metadata(path, schemas))
}

#[test]
fn identical_active_schemas_deduplicate() {
    let shared = schema("string");
    let builder = route(
        route(
            VersionedRouterBuilder::new(),
            "/first",
            versions!("1.0.0.."),
            vec![("Shared".to_owned(), shared.clone())],
        ),
        "/second",
        versions!("1.0.0.."),
        vec![("Shared".to_owned(), shared.clone())],
    );

    let document = builder.generate_openapi(Version::new(1, 0, 0)).unwrap();
    assert!(
        document.components.unwrap().schemas.get("Shared") == Some(&shared),
        "the shared schema should be retained"
    );
}

#[test]
fn unequal_active_schemas_report_both_routes() {
    let builder = route(
        route(
            VersionedRouterBuilder::new(),
            "/first",
            versions!("1.0.0.."),
            vec![("Shared".to_owned(), schema("string"))],
        ),
        "/second",
        versions!("1.0.0.."),
        vec![("Shared".to_owned(), schema("integer"))],
    );

    let expected = RouteConfigError::SchemaConflict {
        schema_name: "Shared".to_owned(),
        first_source: "/first".to_owned(),
        second_source: "/second".to_owned(),
    };
    match builder.generate_openapi(Version::new(1, 0, 0)) {
        Err(error) => assert_eq!(error, expected),
        Ok(_) => panic!("unequal schemas should conflict"),
    }
}

#[test]
fn unequal_schemas_in_disjoint_versions_are_valid() {
    let builder = route(
        route(
            VersionedRouterBuilder::new(),
            "/first",
            versions!("1.0.0..2.0.0"),
            vec![("Shared".to_owned(), schema("string"))],
        ),
        "/second",
        versions!("2.0.0.."),
        vec![("Shared".to_owned(), schema("integer"))],
    );

    builder
        .generate_openapi(Version::new(1, 0, 0))
        .expect("only the first schema is active");
    builder
        .generate_openapi(Version::new(2, 0, 0))
        .expect("only the second schema is active");
}

#[test]
fn unequal_duplicate_within_one_route_is_rejected() {
    let builder = route(
        VersionedRouterBuilder::new(),
        "/one",
        versions!("1.0.0.."),
        vec![
            ("Shared".to_owned(), schema("string")),
            ("Shared".to_owned(), schema("integer")),
        ],
    );

    assert!(matches!(
        builder.generate_openapi(Version::new(1, 0, 0)),
        Err(RouteConfigError::SchemaConflict {
            first_source,
            second_source,
            ..
        }) if first_source == "/one" && second_source == "/one"
    ));
}

#[test]
fn built_in_versioning_schema_uses_the_same_collision_rules() {
    let unequal = route(
        VersionedRouterBuilder::new(),
        "/unequal",
        versions!("1.0.0.."),
        vec![("VersioningError".to_owned(), schema("string"))],
    );
    assert!(matches!(
        unequal.generate_openapi(Version::new(1, 0, 0)),
        Err(RouteConfigError::SchemaConflict {
            schema_name,
            first_source,
            second_source,
        }) if schema_name == "VersioningError"
            && first_source == "/unequal"
            && second_source == "versioning middleware"
    ));

    let identical = route(
        VersionedRouterBuilder::new(),
        "/identical",
        versions!("1.0.0.."),
        vec![(
            "VersioningError".to_owned(),
            <VersioningError as utoipa::PartialSchema>::schema(),
        )],
    );
    identical
        .generate_openapi(Version::new(1, 0, 0))
        .expect("the built-in schema should deduplicate");
}
