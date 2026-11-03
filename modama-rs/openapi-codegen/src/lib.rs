//! Generates a typed HTTP client from a committed OpenAPI document. Meant for build scripts:
//! the GUI's client for the agent's local API, and the agent's client for mgmtinterface-api.
//!
//! The generated files land in `<out_dir>/generated/` (`client.rs`, `types.rs`, ...), to be
//! pulled in with `include!`. `info.rs` holds `API_VERSION`, the document's `info.version`, so a
//! client sends the version its types were generated for.

use std::{fs, path::Path};

use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer,
    type_mapping::{DateStrategy, TypeMapper, TypeMappingConfig},
};

/// Generate the client for the OpenAPI document at `schema_path` into `<out_dir>/generated/`,
/// and tell Cargo to rerun the build script when the document changes.
///
/// With `strip_path_prefix`, that prefix is removed from every path in the document, and the
/// client's base URL has to supply it instead. This is for an API whose mount point can move,
/// like mgmtinterface-api (`/mgmt-api` now, `/api` later). A path without the prefix is an
/// error.
pub fn generate_api_client(
    schema_path: &Path,
    out_dir: &Path,
    strip_path_prefix: Option<&str>,
) -> Result<(), String> {
    println!("cargo:rerun-if-changed={}", schema_path.display());

    let source = fs::read_to_string(schema_path)
        .map_err(|error| format!("failed to read {}: {error}", schema_path.display()))?;
    let mut schema: serde_json::Value = serde_json::from_str(&source)
        .map_err(|error| format!("failed to parse {}: {error}", schema_path.display()))?;
    if let Some(prefix) = strip_path_prefix {
        strip_paths(&mut schema, prefix)?;
    }
    let version = api_version(&schema)?;

    // Timestamps stay strings: nothing reads them yet, and this avoids a chrono dependency in
    // the generated code. The analyzer and the generator each need the mapping.
    let types = TypeMappingConfig {
        date_time: DateStrategy::String,
        ..TypeMappingConfig::default()
    };
    let mut analyzer = SchemaAnalyzer::with_type_mapper(schema, TypeMapper::new(types.clone()))
        .map_err(|error| format!("failed to initialize schema analyzer: {error}"))?;
    let mut analysis = analyzer
        .analyze()
        .map_err(|error| format!("failed to analyze OpenAPI schema: {error}"))?;
    let generator = CodeGenerator::new(GeneratorConfig {
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        types,
        ..GeneratorConfig::default()
    });
    let generated = generator
        .generate_all(&mut analysis)
        .map_err(|error| format!("failed to generate API client: {error}"))?;

    let out_dir = out_dir.join("generated");
    fs::create_dir_all(&out_dir)
        .map_err(|error| format!("failed to create {}: {error}", out_dir.display()))?;
    let info_path = out_dir.join("info.rs");
    fs::write(
        &info_path,
        format!(
            "/// The API version the client was generated for (`info.version` of the document)\n\
             pub const API_VERSION: &str = {version:?};\n"
        ),
    )
    .map_err(|error| format!("failed to write {}: {error}", info_path.display()))?;
    for file in generated.files {
        let output_path = out_dir.join(&file.path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        let content = strip_leading_comments_and_attrs(&file.content);
        fs::write(&output_path, content)
            .map_err(|error| format!("failed to write {}: {error}", output_path.display()))?;
    }
    Ok(())
}

fn api_version(schema: &serde_json::Value) -> Result<String, String> {
    schema
        .pointer("/info/version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "the OpenAPI document has no info.version string".to_owned())
}

fn strip_paths(schema: &mut serde_json::Value, prefix: &str) -> Result<(), String> {
    let paths = schema
        .get_mut("paths")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("the OpenAPI document has no paths")?;
    let stripped = std::mem::take(paths)
        .into_iter()
        .map(|(path, item)| match path.strip_prefix(prefix) {
            Some(rest) if rest.starts_with('/') => Ok((rest.to_owned(), item)),
            _ => Err(format!("path {path} doesn't start with {prefix}/")),
        })
        .collect::<Result<serde_json::Map<_, _>, _>>()?;
    *paths = stripped;
    Ok(())
}

/// `include!` can't take inner doc comments or inner attributes, so drop the leading ones
fn strip_leading_comments_and_attrs(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let first_code_line = lines
        .iter()
        .position(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with("//!") || trimmed.starts_with("#![") || trimmed.is_empty())
        })
        .unwrap_or(lines.len());
    lines[first_code_line..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::{api_version, strip_leading_comments_and_attrs, strip_paths};

    #[test]
    fn the_api_version_comes_from_info_version() {
        let schema = serde_json::json!({"info": {"title": "t", "version": "1"}});
        assert_eq!(api_version(&schema).unwrap(), "1");
        for schema in [
            serde_json::json!({"info": {"title": "t"}}),
            serde_json::json!({"info": {"version": 1}}),
            serde_json::json!({}),
        ] {
            assert!(api_version(&schema).is_err(), "{schema}");
        }
    }

    #[test]
    fn strips_the_prefix_from_every_path() {
        let mut schema = serde_json::json!({
            "paths": {"/mgmt-api/projects": {"get": {}}, "/mgmt-api/versions": {}}
        });
        strip_paths(&mut schema, "/mgmt-api").unwrap();
        assert_eq!(
            schema,
            serde_json::json!({"paths": {"/projects": {"get": {}}, "/versions": {}}})
        );
    }

    #[test]
    fn a_path_outside_the_prefix_is_an_error() {
        for path in ["/other/projects", "/mgmt-apiary"] {
            let mut schema = serde_json::json!({"paths": {path: {}}});
            assert!(strip_paths(&mut schema, "/mgmt-api").is_err(), "{path}");
        }
    }

    #[test]
    fn strips_inner_docs_and_attributes_up_to_the_first_code_line() {
        let source = "//! docs\n#![allow(dead_code)]\n\nuse std::fmt;\n//! kept\n";
        assert_eq!(
            strip_leading_comments_and_attrs(source),
            "use std::fmt;\n//! kept"
        );
    }
}
