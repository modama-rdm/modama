#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rgba(pub(crate) u8, pub(crate) u8, pub(crate) u8, pub(crate) u8);

pub(crate) fn parse_color(value: &str) -> Option<Rgba> {
    let color: csscolorparser::Color = value.parse().ok()?;
    let [red, green, blue, alpha] = color.to_rgba8();
    Some(Rgba(red, green, blue, alpha))
}

#[cfg(test)]
mod tests {
    use super::{Rgba, parse_color};

    #[test]
    fn parses_css_formats_supported_by_csscolorparser() {
        assert_eq!(parse_color("#abc"), Some(Rgba(170, 187, 204, 255)));
        assert_eq!(parse_color("#abcd"), Some(Rgba(170, 187, 204, 221)));
        assert_eq!(parse_color("#112233"), Some(Rgba(17, 34, 51, 255)));
        assert_eq!(parse_color("#11223380"), Some(Rgba(17, 34, 51, 128)));
        for value in [
            "red",
            "cornflowerblue",
            "transparent",
            "rgb(17 34 51)",
            "rgb(17, 34, 51)",
            "rgba(17, 34, 51, 0.5)",
            "hsl(210 50% 20%)",
            "hsla(210, 50%, 20%, 0.5)",
            "hwb(210 10% 20%)",
            "oklch(60% 0.2 250)",
            "oklab(60% 0.1 0.1)",
            "lch(60% 40 250)",
            "lab(60% 20 20)",
        ] {
            assert!(parse_color(value).is_some(), "{value}");
        }
    }

    #[test]
    fn rejects_color_mix_until_the_crate_supports_it() {
        assert_eq!(parse_color("color-mix(in srgb, red 50%, blue)"), None);
    }
}
