use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::PathBuf,
};

use serde::Deserialize;
use serde_yaml::Value;

#[path = "src/color_parser.rs"]
mod color_parser;

use color_parser::parse_color;

#[derive(Debug, Deserialize, Default)]
struct DesignDocument {
    #[serde(flatten)]
    values: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy)]
struct Dimension {
    value: f32,
    unit: &'static str,
}

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let design_path = manifest_dir.join("../../mgmtinterface-new/DESIGN.md");
    println!("cargo:rerun-if-changed={}", design_path.display());

    let source = fs::read_to_string(&design_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", design_path.display()));
    let front_matter = extract_front_matter(&source)
        .unwrap_or_else(|error| panic!("failed to read DESIGN.md front matter: {error}"));
    let design = match front_matter {
        Some(front_matter) => serde_yaml::from_str(front_matter)
            .unwrap_or_else(|error| panic!("failed to parse DESIGN.md front matter: {error}")),
        None => DesignDocument::default(),
    };

    let generated = generate_styles(&design).unwrap_or_else(|error| panic!("{error}"));
    let output_path =
        PathBuf::from(env::var_os("OUT_DIR").unwrap_or_default()).join("design_styles.rs");
    fs::write(&output_path, generated)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", output_path.display()));
}

fn extract_front_matter(source: &str) -> Result<Option<&str>, String> {
    let mut lines = source.lines();
    if source.trim_start().is_empty() {
        return Ok(None);
    }
    if lines.next().map(str::trim) != Some("---") {
        return Ok(None);
    }

    let start = source
        .find('\n')
        .map(|index| index + 1)
        .unwrap_or(source.len());
    let relative_end = source[start..]
        .find("\n---")
        .ok_or_else(|| "DESIGN.md front matter has no closing delimiter (---)".to_owned())?;
    Ok(Some(&source[start..start + relative_end + 1]))
}

fn generate_styles(design: &DesignDocument) -> Result<String, String> {
    let mut output = String::from(
        r#"#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeColor {
    pub light: &'static str,
    pub dark: &'static str,
    pub light_egui: eframe::egui::Color32,
    pub dark_egui: eframe::egui::Color32,
}

impl ThemeColor {
    pub const fn new(
        light: &'static str,
        dark: &'static str,
        light_egui: eframe::egui::Color32,
        dark_egui: eframe::egui::Color32,
    ) -> Self {
        Self { light, dark, light_egui, dark_egui }
    }

    pub const fn resolve(self, visuals: &eframe::egui::Visuals) -> &'static str {
        if visuals.dark_mode { self.dark } else { self.light }
    }

    pub const fn resolve_egui(
        self,
        visuals: &eframe::egui::Visuals,
    ) -> eframe::egui::Color32 {
        if visuals.dark_mode { self.dark_egui } else { self.light_egui }
    }
}

"#,
    );

    generate_colors(&mut output, design.values.get("colors"))?;
    generate_scalar_group(&mut output, "spacing", design.values.get("spacing"), true)?;
    generate_scalar_group(&mut output, "rounded", design.values.get("rounded"), false)?;
    generate_scalar_group(&mut output, "shapes", design.values.get("shapes"), false)?;
    generate_typography(&mut output, design.values.get("typography"))?;
    generate_components(&mut output, design.values.get("components"))?;

    Ok(output)
}

fn generate_colors(output: &mut String, value: Option<&Value>) -> Result<(), String> {
    output.push_str(
        "#[allow(non_upper_case_globals)]\npub mod color {\n    use super::ThemeColor;\n\n",
    );
    let Some(Value::Mapping(colors)) = value else {
        output.push_str("}\n\n");
        return Ok(());
    };

    let (light, dark) =
        if contains_mapping_key(colors, "light") || contains_mapping_key(colors, "dark") {
            (
                mapping(colors, "light").unwrap_or(colors),
                mapping(colors, "dark").unwrap_or(colors),
            )
        } else {
            (colors, colors)
        };
    let names = light
        .keys()
        .chain(dark.keys())
        .filter_map(Value::as_str)
        .collect::<std::collections::BTreeSet<_>>();

    let mut identifiers = BTreeSet::new();
    for name in names {
        let light_value = scalar_string(mapping_value(light, name).unwrap_or(&Value::Null));
        let dark_value = scalar_string(mapping_value(dark, name).unwrap_or(&Value::Null));
        let identifier = unique_identifier(name, &mut identifiers);
        let light_egui = color_literal(&light_value)
            .ok_or_else(|| format!("colors.light.{name} is not representable by egui"))?;
        let dark_egui = color_literal(&dark_value)
            .ok_or_else(|| format!("colors.dark.{name} is not representable by egui"))?;
        output.push_str(&format!(
            "    pub mod {identifier} {{\n        use super::ThemeColor;\n        pub const VALUE: ThemeColor = ThemeColor::new({:?}, {:?}, {light_egui}, {dark_egui});\n    }}\n",
            light_value,
            dark_value,
        ));
    }
    output.push_str("}\n\n");
    Ok(())
}

fn generate_scalar_group(
    output: &mut String,
    module_name: &str,
    value: Option<&Value>,
    support_legacy_scale: bool,
) -> Result<(), String> {
    output.push_str(&format!(
        "#[allow(non_upper_case_globals, unused_imports)]\npub mod {module_name} {{\n"
    ));
    let Some(Value::Mapping(values)) = value else {
        output.push_str("}\n\n");
        return Ok(());
    };

    if support_legacy_scale {
        if let Some(unit) = mapping_value(values, "unit").and_then(Value::as_str) {
            if let Some(dimension) = parse_dimension(unit) {
                output.push_str(&format!(
                    "    pub const UNIT_EGUI_POINTS: f32 = {:?};\n",
                    dimension_to_egui_points(dimension)
                ));
            }
        }
        if let Some(Value::Sequence(scale)) = mapping_value(values, "scale") {
            let numbers = scale.iter().filter_map(number_value).collect::<Vec<_>>();
            if numbers.len() == scale.len() {
                let unit_value = mapping_value(values, "unit")
                    .and_then(Value::as_str)
                    .and_then(parse_dimension)
                    .map(|dimension| dimension.value)
                    .ok_or_else(|| "spacing unit must be a supported dimension".to_owned())?;
                output.push_str(&format!(
                    "    pub const SCALE: &[f32] = &[{}];\n",
                    numbers
                        .iter()
                        .map(|number| format!("{number:?}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));

                for (name, scale_value) in [
                    ("card_padding_x", 10.0),
                    ("card_padding_y", 11.0),
                    ("input_padding_x", 3.0),
                    ("input_padding_y", 2.5),
                    ("button_padding_x", 5.0),
                    ("button_padding_y", 2.5),
                    ("field_label_gap", 1.5),
                    ("form_field_gap", 4.0),
                    ("header_title_gap", 3.5),
                    ("header_subtitle_gap", 1.0),
                    ("header_bottom_gap", 8.0),
                    ("button_margin_top", 2.0),
                ] {
                    let Some(index) = numbers
                        .iter()
                        .position(|value| (*value - scale_value).abs() < f32::EPSILON)
                    else {
                        return Err(format!(
                            "spacing scale is missing the token value for {name}"
                        ));
                    };
                    emit_dimension_module(output, name, numbers[index] * unit_value);
                }
            }
        }
    }

    let mut identifiers = BTreeSet::new();
    for (name, value) in values {
        let Some(name) = name.as_str() else {
            continue;
        };
        if support_legacy_scale && matches!(name, "unit" | "scale") {
            continue;
        }
        let identifier = unique_identifier(name, &mut identifiers);
        emit_scalar_module(output, &identifier, value)?;
    }
    output.push_str("}\n\n");
    Ok(())
}

fn generate_typography(output: &mut String, value: Option<&Value>) -> Result<(), String> {
    output.push_str("#[allow(non_upper_case_globals, unused_imports)]\npub mod typography {\n");
    let Some(Value::Mapping(values)) = value else {
        output.push_str("}\n\n");
        return Ok(());
    };

    let mut identifiers = BTreeSet::new();
    for (name, value) in values {
        let Some(name) = name.as_str() else {
            continue;
        };
        let Value::Mapping(properties) = value else {
            return Err(format!("typography token {name} must be a mapping"));
        };
        let identifier = unique_identifier(name, &mut identifiers);
        output.push_str(&format!("    pub mod {identifier} {{\n"));
        emit_optional_string_property(output, properties, "fontFamily", "FONT_FAMILY", name)?;
        emit_optional_string_property(
            output,
            properties,
            "nativeFontFamily",
            "NATIVE_FONT_FAMILY",
            name,
        )?;
        emit_optional_dimension_property(output, properties, "fontSize", "FONT_SIZE", name)?;
        emit_optional_number_property(output, properties, "fontWeight", "FONT_WEIGHT", name)?;
        emit_optional_string_property(output, properties, "lineHeight", "LINE_HEIGHT", name)?;
        emit_optional_dimension_property(
            output,
            properties,
            "letterSpacing",
            "LETTER_SPACING",
            name,
        )?;
        emit_optional_string_property(output, properties, "fontFeature", "FONT_FEATURE", name)?;
        emit_optional_string_property(output, properties, "fontVariation", "FONT_VARIATION", name)?;
        if mapping_value(properties, "fontSize").is_some()
            && mapping_value(properties, "nativeFontFamily").is_some()
        {
            output.push_str(
                "        pub fn font_id() -> eframe::egui::FontId {\n            eframe::egui::FontId::new(FONT_SIZE, eframe::egui::FontFamily::Name(NATIVE_FONT_FAMILY.into()))\n        }\n",
            );
        }
        if let Some(weights) = mapping_value(properties, "weights") {
            let values = weights.as_sequence().ok_or_else(|| {
                format!("typography.{name}.weights must be a sequence of numeric weights")
            })?;
            let weights = values
                .iter()
                .map(|value| {
                    number_value(value)
                        .map(|value| value as u16)
                        .ok_or_else(|| format!("typography.{name}.weights contains a non-number"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            output.push_str(&format!(
                "        pub const WEIGHTS: &[u16] = &[{}];\n",
                weights
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        output.push_str("    }\n");
    }
    output.push_str("}\n\n");
    Ok(())
}

fn generate_components(output: &mut String, value: Option<&Value>) -> Result<(), String> {
    output.push_str("#[allow(non_upper_case_globals, unused_imports)]\npub mod components {\n");
    let Some(Value::Mapping(values)) = value else {
        output.push_str("}\n\n");
        return Ok(());
    };

    let mut identifiers = BTreeSet::new();
    for (name, value) in values {
        let Some(name) = name.as_str() else {
            continue;
        };
        let Value::Mapping(properties) = value else {
            return Err(format!("component {name} must be a mapping"));
        };
        let identifier = unique_identifier(name, &mut identifiers);
        output.push_str(&format!("    pub mod {identifier} {{\n"));
        let mut property_identifiers = BTreeSet::new();
        for (key, value) in properties {
            let Some(key) = key.as_str() else {
                return Err(format!("components.{name} has a non-string property name"));
            };
            emit_scalar_module(
                output,
                &unique_identifier(key, &mut property_identifiers),
                value,
            )?;
        }
        output.push_str("    }\n");
    }
    output.push_str("}\n\n");
    Ok(())
}

fn emit_dimension_module(output: &mut String, identifier: &str, points: f32) {
    output.push_str(&format!(
        "    pub mod {identifier} {{\n        pub const EGUI_POINTS: f32 = {points:?};\n    }}\n"
    ));
}

fn emit_scalar_module(output: &mut String, identifier: &str, value: &Value) -> Result<(), String> {
    if let Some(dimension) = value.as_str().and_then(parse_dimension) {
        emit_dimension_module(output, identifier, dimension_to_egui_points(dimension));
        return Ok(());
    }
    if let Some(number) = number_value(value) {
        output.push_str(&format!(
            "    pub mod {identifier} {{\n        pub const VALUE: f32 = {number:?};\n    }}\n"
        ));
        return Ok(());
    }
    if value.is_string() {
        output.push_str(&format!(
            "    pub mod {identifier} {{\n        pub const VALUE: &str = {:?};\n    }}\n",
            scalar_string(value)
        ));
        return Ok(());
    }
    Err(format!(
        "{identifier} must be a dimension, number, or string"
    ))
}

fn emit_optional_string_property(
    output: &mut String,
    properties: &serde_yaml::Mapping,
    yaml_name: &str,
    rust_name: &str,
    token_name: &str,
) -> Result<(), String> {
    let Some(value) = mapping_value(properties, yaml_name) else {
        return Ok(());
    };
    let value = value
        .as_str()
        .ok_or_else(|| format!("typography.{token_name}.{yaml_name} must be a string"))?;
    output.push_str(&format!(
        "        pub const {rust_name}: &str = {value:?};\n"
    ));
    Ok(())
}

fn emit_optional_dimension_property(
    output: &mut String,
    properties: &serde_yaml::Mapping,
    yaml_name: &str,
    rust_name: &str,
    token_name: &str,
) -> Result<(), String> {
    let Some(value) = mapping_value(properties, yaml_name) else {
        return Ok(());
    };
    let value = value.as_str().and_then(parse_dimension).ok_or_else(|| {
        format!("typography.{token_name}.{yaml_name} must be a px, rem, or em dimension")
    })?;
    output.push_str(&format!(
        "        pub const {rust_name}: f32 = {:?};\n",
        dimension_to_egui_points(value)
    ));
    Ok(())
}

fn emit_optional_number_property(
    output: &mut String,
    properties: &serde_yaml::Mapping,
    yaml_name: &str,
    rust_name: &str,
    token_name: &str,
) -> Result<(), String> {
    let Some(value) = mapping_value(properties, yaml_name) else {
        return Ok(());
    };
    let value = number_value(value)
        .ok_or_else(|| format!("typography.{token_name}.{yaml_name} must be a number"))?;
    output.push_str(&format!(
        "        pub const {rust_name}: u16 = {}u16;\n",
        value as u16
    ));
    Ok(())
}

fn dimension_to_egui_points(dimension: Dimension) -> f32 {
    match dimension.unit {
        "rem" | "em" => dimension.value * 16.0,
        _ => dimension.value,
    }
}

fn mapping<'a>(value: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Mapping> {
    mapping_value(value, key).and_then(Value::as_mapping)
}

fn mapping_value<'a>(value: &'a serde_yaml::Mapping, key: &str) -> Option<&'a Value> {
    value.get(Value::String(key.to_owned()))
}

fn contains_mapping_key(value: &serde_yaml::Mapping, key: &str) -> bool {
    mapping(value, key).is_some()
}

fn scalar_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        _ => serde_yaml::to_string(value)
            .unwrap_or_default()
            .trim()
            .to_owned(),
    }
}

fn number_value(value: &Value) -> Option<f32> {
    match value {
        Value::Number(value) => value.as_f64().map(|value| value as f32),
        Value::String(value) => value.parse::<f32>().ok(),
        _ => None,
    }
}

fn parse_dimension(value: &str) -> Option<Dimension> {
    let split_at = value.find(|character: char| {
        !character.is_ascii_digit() && character != '.' && character != '-'
    })?;
    let number = value[..split_at].parse::<f32>().ok()?;
    let unit = match &value[split_at..] {
        "px" => "px",
        "rem" => "rem",
        "em" => "em",
        _ => return None,
    };
    Some(Dimension {
        value: number,
        unit,
    })
}

fn color_literal(value: &str) -> Option<String> {
    parse_color(value).map(|color| {
        format!(
            "eframe::egui::Color32::from_rgba_unmultiplied_const({}, {}, {}, {})",
            color.0, color.1, color.2, color.3
        )
    })
}

fn unique_identifier(name: &str, used: &mut BTreeSet<String>) -> String {
    let mut identifier = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            identifier.push(character.to_ascii_lowercase());
        } else {
            identifier.push('_');
        }
    }
    if identifier.is_empty() {
        identifier.push_str("token");
    }
    if identifier.starts_with(|character: char| character.is_ascii_digit()) {
        identifier.insert(0, '_');
    }
    if matches!(
        identifier.as_str(),
        "self"
            | "Self"
            | "super"
            | "crate"
            | "mod"
            | "type"
            | "match"
            | "ref"
            | "move"
            | "const"
            | "static"
    ) {
        identifier.push('_');
    }

    if used.insert(identifier.clone()) {
        return identifier;
    }

    let suffix = stable_hash(name);
    let disambiguated = format!("{identifier}_{suffix:08x}");
    used.insert(disambiguated.clone());
    disambiguated
}

fn stable_hash(value: &str) -> u32 {
    value.bytes().fold(0x811c9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x01000193)
    })
}
