//! Generates a typed HTTP client from a committed OpenAPI document. Meant for build scripts:
//! the GUI's client for the agent's local API, and the agent's client for mgmtinterface-api.
//!
//! The generated files land in `<out_dir>/generated/` (`client.rs`, `types.rs`, ...), to be
//! pulled in with `include!`.

use std::{fs, path::Path};

use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer,
    type_mapping::{DateStrategy, TypeMapper, TypeMappingConfig},
};

/// Generate the client for the OpenAPI document at `schema_path` into `<out_dir>/generated/`,
/// and tell Cargo to rerun the build script when the document changes.
pub fn generate_api_client(schema_path: &Path, out_dir: &Path) -> Result<(), String> {
    println!("cargo:rerun-if-changed={}", schema_path.display());

    let source = fs::read_to_string(schema_path)
        .map_err(|error| format!("failed to read {}: {error}", schema_path.display()))?;
    let schema: serde_json::Value = serde_json::from_str(&source)
        .map_err(|error| format!("failed to parse {}: {error}", schema_path.display()))?;

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
    use super::strip_leading_comments_and_attrs;

    #[test]
    fn strips_inner_docs_and_attributes_up_to_the_first_code_line() {
        let source = "//! docs\n#![allow(dead_code)]\n\nuse std::fmt;\n//! kept\n";
        assert_eq!(
            strip_leading_comments_and_attrs(source),
            "use std::fmt;\n//! kept"
        );
    }
}
