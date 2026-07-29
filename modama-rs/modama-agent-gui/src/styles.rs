use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin,
    Shadow, Stroke, TextStyle,
};
use std::sync::Arc;

include!(concat!(env!("OUT_DIR"), "/design_styles.rs"));

/// Runtime font families and semantic text roles shared by every GUI screen.
pub mod font {
    use super::*;

    const SANS_REGULAR: &str = "plus-jakarta-sans-regular";
    const SANS_MEDIUM: &str = "plus-jakarta-sans-medium";
    const SANS_SEMIBOLD: &str = "plus-jakarta-sans-semibold";
    const SANS_BOLD: &str = "plus-jakarta-sans-bold";
    const MONO_REGULAR: &str = "jetbrains-mono-regular";
    const MONO_MEDIUM: &str = "jetbrains-mono-medium";

    const SANS_REGULAR_FAMILY: &str = "Plus Jakarta Sans Regular";
    const SANS_MEDIUM_FAMILY: &str = "Plus Jakarta Sans Medium";
    const SANS_SEMIBOLD_FAMILY: &str = "Plus Jakarta Sans SemiBold";
    const SANS_BOLD_FAMILY: &str = "Plus Jakarta Sans Bold";
    const MONO_REGULAR_FAMILY: &str = "JetBrains Mono Regular";
    const MONO_MEDIUM_FAMILY: &str = "JetBrains Mono Medium";

    fn family(name: &str) -> FontFamily {
        FontFamily::Name(name.into())
    }

    pub(crate) fn font_id(font_family: &str, font_weight: u16, size: f32) -> FontId {
        let native_family = match (font_family, font_weight) {
            (family, 400) if family.starts_with("Plus Jakarta Sans") => SANS_REGULAR_FAMILY,
            (family, 500) if family.starts_with("Plus Jakarta Sans") => SANS_MEDIUM_FAMILY,
            (family, 600) if family.starts_with("Plus Jakarta Sans") => SANS_SEMIBOLD_FAMILY,
            (family, 700) if family.starts_with("Plus Jakarta Sans") => SANS_BOLD_FAMILY,
            (family, 400) if family.starts_with("JetBrains Mono") => MONO_REGULAR_FAMILY,
            (family, 500) if family.starts_with("JetBrains Mono") => MONO_MEDIUM_FAMILY,
            _ => panic!("no bundled native font for {font_family} weight {font_weight}"),
        };
        FontId::new(size, family(native_family))
    }

    /// Install the static design-spec font faces and map egui's default text
    /// roles to their React UI equivalents.
    pub fn install(ctx: &egui::Context) {
        let mut fonts = FontDefinitions::default();
        for (name, bytes, family_name) in [
            (
                SANS_REGULAR,
                include_bytes!("../assets/fonts/PlusJakartaSans-Regular.ttf").as_slice(),
                SANS_REGULAR_FAMILY,
            ),
            (
                SANS_MEDIUM,
                include_bytes!("../assets/fonts/PlusJakartaSans-Medium.ttf").as_slice(),
                SANS_MEDIUM_FAMILY,
            ),
            (
                SANS_SEMIBOLD,
                include_bytes!("../assets/fonts/PlusJakartaSans-SemiBold.ttf").as_slice(),
                SANS_SEMIBOLD_FAMILY,
            ),
            (
                SANS_BOLD,
                include_bytes!("../assets/fonts/PlusJakartaSans-Bold.ttf").as_slice(),
                SANS_BOLD_FAMILY,
            ),
            (
                MONO_REGULAR,
                include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf").as_slice(),
                MONO_REGULAR_FAMILY,
            ),
            (
                MONO_MEDIUM,
                include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf").as_slice(),
                MONO_MEDIUM_FAMILY,
            ),
        ] {
            fonts
                .font_data
                .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
            fonts
                .families
                .insert(family(family_name), vec![name.to_owned()]);
        }
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        ctx.set_fonts(fonts);
        ctx.all_styles_mut(|style| {
            style
                .text_styles
                .insert(TextStyle::Body, typography::body::font_id());
            style
                .text_styles
                .insert(TextStyle::Button, typography::button::font_id());
            style
                .text_styles
                .insert(TextStyle::Heading, typography::heading::font_id());
            style
                .text_styles
                .insert(TextStyle::Monospace, typography::mono::font_id());
        });
    }
}

pub mod theme {
    use super::*;

    /// Install the design tokens as egui's light and dark visual themes.
    ///
    /// The application starts in light mode. Call `Context::set_theme` later
    /// to switch modes without changing any component code.
    pub fn install(ctx: &egui::Context) {
        font::install(ctx);
        let light = visuals(false);
        let dark = visuals(true);
        ctx.set_visuals_of(egui::Theme::Light, light);
        ctx.set_visuals_of(egui::Theme::Dark, dark);
        ctx.all_styles_mut(|style| {
            style.spacing.button_padding = egui::vec2(
                spacing::button_padding_x::EGUI_POINTS,
                spacing::button_padding_y::EGUI_POINTS,
            );
        });
        ctx.set_theme(egui::Theme::Light);
    }

    fn visuals(dark: bool) -> egui::Visuals {
        let base = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        let resolve = |color: ThemeColor| {
            if dark {
                color.dark_egui
            } else {
                color.light_egui
            }
        };
        let bg = resolve(color::background::VALUE);
        let surface = resolve(color::surface::VALUE);
        let surface_muted = resolve(color::surface_variant::VALUE);
        let border = resolve(color::outline::VALUE);
        let fg = resolve(color::on_surface::VALUE);
        let fg_muted = resolve(color::on_surface_variant::VALUE);
        let primary = resolve(color::primary::VALUE);
        let primary_hover = resolve(color::primary_variant::VALUE);

        let mut visuals = base;
        visuals.panel_fill = bg;
        visuals.window_fill = surface;
        visuals.window_corner_radius = radius(rounded::_2xl::EGUI_POINTS);
        visuals.window_stroke =
            Stroke::new(components::standard_border::width::EGUI_POINTS, border);
        let shadow = card_shadow_with_color(resolve(color::shadow::VALUE));
        visuals.window_shadow = shadow;
        visuals.popup_shadow = shadow;
        visuals.extreme_bg_color = surface;
        visuals.text_edit_bg_color = Some(surface);
        visuals.faint_bg_color = surface_muted;
        visuals.code_bg_color = surface_muted;
        visuals.override_text_color = Some(fg);
        visuals.weak_text_color = Some(fg_muted);
        visuals.error_fg_color = resolve(color::error::VALUE);
        visuals.warn_fg_color = resolve(color::secondary::VALUE);
        visuals.hyperlink_color = primary;
        visuals.button_frame = true;
        visuals.disabled_alpha = 0.6;

        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = radius(rounded::lg::EGUI_POINTS);
            widget.fg_stroke = Stroke::new(components::standard_border::width::EGUI_POINTS, fg);
        }
        visuals.widgets.inactive.weak_bg_fill = primary;
        visuals.widgets.inactive.bg_fill = primary;
        visuals.widgets.inactive.bg_stroke =
            Stroke::new(components::standard_border::width::EGUI_POINTS, primary);
        visuals.widgets.hovered.weak_bg_fill = primary_hover;
        visuals.widgets.hovered.bg_fill = primary_hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(
            components::standard_border::width::EGUI_POINTS,
            primary_hover,
        );
        visuals.widgets.active.weak_bg_fill = primary_hover;
        visuals.widgets.active.bg_fill = primary_hover;
        visuals.widgets.active.bg_stroke = Stroke::new(
            components::standard_border::width::EGUI_POINTS,
            primary_hover,
        );
        visuals.widgets.open = visuals.widgets.hovered;

        // Text edits use the inactive state while idle and active while focused.
        visuals.widgets.noninteractive.bg_fill = surface;
        visuals.widgets.noninteractive.bg_stroke =
            Stroke::new(components::standard_border::width::EGUI_POINTS, border);
        visuals.widgets.noninteractive.fg_stroke =
            Stroke::new(components::standard_border::width::EGUI_POINTS, fg);
        visuals.widgets.noninteractive.corner_radius = radius(rounded::lg::EGUI_POINTS);
        visuals
    }
}

pub fn radius(points: f32) -> CornerRadius {
    CornerRadius::same(points.clamp(0.0, 255.0) as u8)
}

/// Horizontal distance from a compact content card's outer edge to its content guide.
pub fn content_card_inset() -> f32 {
    components::standard_border::width::EGUI_POINTS + spacing::button_padding_x::EGUI_POINTS
}

/// Horizontal distance from the window edge to a content card's outer edge.
pub fn content_card_outer_inset_from_window() -> f32 {
    spacing::app_window_margin_x::EGUI_POINTS + components::app_shell::padding::EGUI_POINTS
}

/// Horizontal position of the shared content guide relative to the window.
pub fn content_guide_from_window_left() -> f32 {
    content_card_outer_inset_from_window() + content_card_inset()
}

/// Extra space needed after normal item spacing to reach the content-card top gap.
pub fn content_card_top_margin_adjustment() -> f32 {
    let adjustment = spacing::form_field_gap::EGUI_POINTS - spacing::app_item_gap_y::EGUI_POINTS;
    debug_assert!(
        adjustment >= 0.0,
        "normal item spacing exceeds the content-card top gap"
    );
    adjustment.max(0.0)
}

/// Additional leading space needed inside the padded top bar to reach the guide.
pub fn top_bar_content_guide_offset() -> f32 {
    let offset = content_guide_from_window_left() - components::top_bar::padding::EGUI_POINTS;
    debug_assert!(offset >= 0.0, "top-bar padding exceeds the content guide");
    offset
}

/// Additional trailing space needed inside the padded top bar to reach the card edge.
pub fn top_bar_card_edge_offset() -> f32 {
    let offset = content_card_outer_inset_from_window() - components::top_bar::padding::EGUI_POINTS;
    debug_assert!(offset >= 0.0, "top-bar padding exceeds the card edge");
    offset
}

pub fn card_frame(visuals: &egui::Visuals) -> Frame {
    card_frame_with_padding(
        visuals,
        Margin::symmetric(
            spacing::card_padding_x::EGUI_POINTS as i8,
            spacing::card_padding_y::EGUI_POINTS as i8,
        ),
    )
}

pub fn content_card_frame(visuals: &egui::Visuals) -> Frame {
    card_frame_with_padding(
        visuals,
        Margin::symmetric(
            spacing::button_padding_x::EGUI_POINTS as i8,
            (2.0 * spacing::button_padding_y::EGUI_POINTS) as i8,
        ),
    )
}

fn card_frame_with_padding(visuals: &egui::Visuals, padding: Margin) -> Frame {
    Frame::new()
        .fill(color::surface::VALUE.resolve_egui(visuals))
        .stroke(Stroke::new(
            components::standard_border::width::EGUI_POINTS,
            color::outline::VALUE.resolve_egui(visuals),
        ))
        .corner_radius(radius(rounded::_2xl::EGUI_POINTS))
        .inner_margin(padding)
        .shadow(card_shadow(visuals))
}

pub fn input_frame(visuals: &egui::Visuals, margin: Margin) -> Frame {
    Frame::new()
        .fill(color::surface::VALUE.resolve_egui(visuals))
        .stroke(Stroke::new(
            components::standard_border::width::EGUI_POINTS,
            color::outline::VALUE.resolve_egui(visuals),
        ))
        .corner_radius(radius(rounded::lg::EGUI_POINTS))
        .inner_margin(margin)
}

pub fn card_shadow(visuals: &egui::Visuals) -> Shadow {
    card_shadow_with_color(color::shadow::VALUE.resolve_egui(visuals))
}

fn card_shadow_with_color(color: Color32) -> Shadow {
    Shadow {
        offset: [0, components::card_shadow_offset_y::size::EGUI_POINTS as i8],
        blur: components::card_shadow_blur::size::EGUI_POINTS as u8,
        spread: components::card_shadow_spread::size::EGUI_POINTS as u8,
        color,
    }
}

pub fn paint_focus_ring(ui: &egui::Ui, response: &egui::Response) {
    if response.has_focus() {
        let width = components::focus_ring::width::EGUI_POINTS;
        ui.painter().rect_stroke(
            response.rect,
            radius(rounded::lg::EGUI_POINTS),
            Stroke::new(
                components::standard_border::width::EGUI_POINTS,
                color::primary::VALUE.resolve_egui(ui.visuals()),
            ),
            egui::StrokeKind::Inside,
        );
        ui.painter().rect_stroke(
            response.rect,
            radius(rounded::lg::EGUI_POINTS),
            Stroke::new(width, color::primary_tint::VALUE.resolve_egui(ui.visuals())),
            egui::StrokeKind::Outside,
        );
    }
}

pub fn danger_frame(visuals: &egui::Visuals) -> Frame {
    Frame::new()
        .fill(color::error_container::VALUE.resolve_egui(visuals))
        .stroke(Stroke::new(
            components::standard_border::width::EGUI_POINTS,
            color::error_variant::VALUE.resolve_egui(visuals),
        ))
        .corner_radius(radius(rounded::xl::EGUI_POINTS))
        .inner_margin(Margin::symmetric(
            components::danger_alert_padding_x::padding::EGUI_POINTS as i8,
            components::danger_alert_padding_y::padding::EGUI_POINTS as i8,
        ))
}
