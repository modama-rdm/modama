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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dimension {
    pub value: f32,
    pub unit: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Number(pub f32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringToken(pub &'static str);

#[derive(Debug, PartialEq)]
pub struct Typography {
    pub font_family: Option<&'static str>,
    pub font_size: Option<Dimension>,
    pub font_weight: Option<u16>,
    pub line_height: Option<StringToken>,
    pub letter_spacing: Option<Dimension>,
    pub font_feature: Option<&'static str>,
    pub font_variation: Option<&'static str>,
    pub weights: &'static [u16],
}

#[derive(Debug, PartialEq, Eq)]
pub struct Component {
    pub properties: &'static [(&'static str, &'static str)],
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
        let light_egui = color_literal(name, "light", &light_value);
        let dark_egui = color_literal(name, "dark", &dark_value);
        let (Some(light_egui), Some(dark_egui)) = (light_egui, dark_egui) else {
            continue;
        };
        output.push_str(&format!(
            "    pub const {identifier}: ThemeColor = ThemeColor::new({:?}, {:?}, {light_egui}, {dark_egui});\n",
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
        "#[allow(non_upper_case_globals, unused_imports)]\npub mod {module_name} {{\n    use super::{{Dimension, Number, StringToken}};\n\n"
    ));
    let Some(Value::Mapping(values)) = value else {
        output.push_str("}\n\n");
        return Ok(());
    };

    if support_legacy_scale {
        if let Some(unit) = mapping_value(values, "unit").and_then(Value::as_str) {
            if let Some(dimension) = parse_dimension(unit) {
                output.push_str(&format!(
                    "    pub const unit: Dimension = Dimension {{ value: {:?}, unit: {:?} }};\n",
                    dimension.value, dimension.unit
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
                    "    pub const scale: &[f32] = &[{}];\n",
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
                    output.push_str(&format!(
                        "    pub const {name}: Dimension = Dimension {{ value: {:?}, unit: unit.unit }};\n",
                        numbers[index] * unit_value
                    ));
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
        output.push_str(&scalar_constant(identifier, value)?);
    }
    output.push_str("}\n\n");
    Ok(())
}

fn generate_typography(output: &mut String, value: Option<&Value>) -> Result<(), String> {
    output.push_str(
        "#[allow(non_upper_case_globals, unused_imports)]\npub mod typography {\n    use super::{Dimension, StringToken, Typography};\n\n",
    );
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
        let font_family = optional_string(properties, "fontFamily");
        let font_size = optional_dimension(properties, "fontSize");
        let font_weight = optional_number(properties, "fontWeight").map(|number| number as u16);
        let line_height = optional_string_token(properties, "lineHeight");
        let letter_spacing = optional_dimension(properties, "letterSpacing");
        let font_feature = optional_string(properties, "fontFeature");
        let font_variation = optional_string(properties, "fontVariation");
        let weights = mapping_value(properties, "weights")
            .and_then(Value::as_sequence)
            .map(|values| {
                values
                    .iter()
                    .filter_map(number_value)
                    .map(|value| value as u16)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        output.push_str(&format!(
            "    pub const {identifier}: Typography = Typography {{ font_family: {font_family}, font_size: {font_size}, font_weight: {:?}, line_height: {line_height}, letter_spacing: {letter_spacing}, font_feature: {font_feature}, font_variation: {font_variation}, weights: &[{}] }};\n",
            font_weight,
            weights.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
        ));
    }
    output.push_str("}\n\n");
    Ok(())
}

fn generate_components(output: &mut String, value: Option<&Value>) -> Result<(), String> {
    output.push_str(
        "#[allow(non_upper_case_globals, unused_imports)]\npub mod components {\n    use super::Component;\n\n",
    );
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
        let properties = properties
            .iter()
            .filter_map(|(key, value)| key.as_str().map(|key| (key, scalar_string(value))))
            .map(|(key, value)| format!("({key:?}, {value:?})"))
            .collect::<Vec<_>>()
            .join(", ");
        output.push_str(&format!(
            "    pub const {identifier}: Component = Component {{ properties: &[{properties}] }};\n"
        ));
    }
    output.push_str("}\n\n");
    Ok(())
}

fn scalar_constant(identifier: String, value: &Value) -> Result<String, String> {
    if let Some(dimension) = value.as_str().and_then(parse_dimension) {
        return Ok(format!(
            "    pub const {identifier}: Dimension = Dimension {{ value: {:?}, unit: {:?} }};\n",
            dimension.value, dimension.unit
        ));
    }
    if let Some(number) = number_value(value) {
        return Ok(format!(
            "    pub const {identifier}: Number = Number({number:?});\n"
        ));
    }
    Ok(format!(
        "    pub const {identifier}: StringToken = StringToken({:?});\n",
        scalar_string(value)
    ))
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

fn optional_string(value: &serde_yaml::Mapping, key: &str) -> String {
    mapping_value(value, key)
        .and_then(Value::as_str)
        .map(|value| format!("Some({value:?})"))
        .unwrap_or_else(|| "None".to_owned())
}

fn optional_string_token(value: &serde_yaml::Mapping, key: &str) -> String {
    mapping_value(value, key)
        .map(|value| format!("Some(StringToken({:?}))", scalar_string(value)))
        .unwrap_or_else(|| "None".to_owned())
}

fn optional_dimension(value: &serde_yaml::Mapping, key: &str) -> String {
    mapping_value(value, key)
        .and_then(Value::as_str)
        .and_then(parse_dimension)
        .map(|dimension| {
            format!(
                "Some(Dimension {{ value: {:?}, unit: {:?} }})",
                dimension.value, dimension.unit
            )
        })
        .unwrap_or_else(|| "None".to_owned())
}

fn optional_number(value: &serde_yaml::Mapping, key: &str) -> Option<f32> {
    mapping_value(value, key).and_then(number_value)
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

fn color_literal(name: &str, mode: &str, value: &str) -> Option<String> {
    parse_color(value).map(|color| {
        format!(
            "eframe::egui::Color32::from_rgba_unmultiplied_const({}, {}, {}, {})",
            color.0, color.1, color.2, color.3
        )
    }).or_else(|| {
        println!(
            "cargo:warning=unrepresentable CSS color for colors.{name} ({mode}): {value}; token omitted"
        );
        None
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
