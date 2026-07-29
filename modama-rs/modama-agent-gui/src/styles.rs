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

    pub fn from_token(token: Typography) -> FontId {
        FontId::new(
            token.font_size.map(token_to_px).unwrap_or(14.0),
            family(
                token
                    .native_font_family
                    .expect("typography tokens used by egui require nativeFontFamily"),
            ),
        )
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
        ctx.set_fonts(fonts);
        ctx.all_styles_mut(|style| {
            style
                .text_styles
                .insert(TextStyle::Body, from_token(typography::body));
            style
                .text_styles
                .insert(TextStyle::Button, from_token(typography::button));
            style
                .text_styles
                .insert(TextStyle::Heading, from_token(typography::heading));
            style
                .text_styles
                .insert(TextStyle::Monospace, from_token(typography::mono));
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
                token_to_px(spacing::button_padding_x),
                token_to_px(spacing::button_padding_y),
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
        let bg = resolve(color::bg);
        let surface = resolve(color::surface);
        let surface_muted = resolve(color::surface_muted);
        let border = resolve(color::border);
        let fg = resolve(color::fg);
        let fg_muted = resolve(color::fg_muted);
        let primary = resolve(color::primary);
        let primary_hover = resolve(color::primary_hover);

        let mut visuals = base;
        visuals.panel_fill = bg;
        visuals.window_fill = surface;
        visuals.window_corner_radius = radius(shapes::radius_2xl);
        visuals.window_stroke = Stroke::new(1.0_f32, border);
        visuals.window_shadow = card_shadow();
        visuals.popup_shadow = card_shadow();
        visuals.extreme_bg_color = surface;
        visuals.text_edit_bg_color = Some(surface);
        visuals.faint_bg_color = surface_muted;
        visuals.code_bg_color = surface_muted;
        visuals.override_text_color = Some(fg);
        visuals.weak_text_color = Some(fg_muted);
        visuals.error_fg_color = resolve(color::danger);
        visuals.warn_fg_color = resolve(color::warning);
        visuals.hyperlink_color = primary;
        visuals.button_frame = true;
        visuals.disabled_alpha = 0.6;

        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = radius(shapes::radius_lg);
            widget.fg_stroke = Stroke::new(1.0_f32, fg);
        }
        visuals.widgets.inactive.weak_bg_fill = primary;
        visuals.widgets.inactive.bg_fill = primary;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, primary);
        visuals.widgets.hovered.weak_bg_fill = primary_hover;
        visuals.widgets.hovered.bg_fill = primary_hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, primary_hover);
        visuals.widgets.active.weak_bg_fill = primary_hover;
        visuals.widgets.active.bg_fill = primary_hover;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, primary_hover);
        visuals.widgets.open = visuals.widgets.hovered;

        // Text edits use the inactive state while idle and active while focused.
        visuals.widgets.noninteractive.bg_fill = surface;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, fg);
        visuals.widgets.noninteractive.corner_radius = radius(shapes::radius_lg);
        visuals
    }
}

pub fn radius(token: Dimension) -> CornerRadius {
    CornerRadius::same(token_to_px(token).clamp(0.0, 255.0) as u8)
}

pub fn token_to_px(token: Dimension) -> f32 {
    match token.unit {
        "rem" | "em" => token.value * 16.0,
        _ => token.value,
    }
}

pub fn spacing(scale_index: usize) -> f32 {
    spacing::scale.get(scale_index).copied().unwrap_or_default() * spacing::unit.value
}

pub fn card_frame(visuals: &egui::Visuals, margin: Margin) -> Frame {
    Frame::new()
        .fill(color::surface.resolve_egui(visuals))
        .stroke(Stroke::new(1.0_f32, color::border.resolve_egui(visuals)))
        .corner_radius(radius(shapes::radius_2xl))
        .inner_margin(margin)
        .shadow(card_shadow())
}

pub fn input_frame(visuals: &egui::Visuals, margin: Margin) -> Frame {
    Frame::new()
        .fill(color::surface.resolve_egui(visuals))
        .stroke(Stroke::new(1.0_f32, color::border.resolve_egui(visuals)))
        .corner_radius(radius(shapes::radius_lg))
        .inner_margin(margin)
}

pub fn card_shadow() -> Shadow {
    Shadow {
        offset: [0, 1],
        blur: 4,
        spread: 0,
        color: Color32::from_black_alpha(20),
    }
}

pub fn paint_focus_ring(ui: &egui::Ui, response: &egui::Response) {
    if response.has_focus() {
        let width = token_to_px(shapes::focus_ring_width);
        ui.painter().rect_stroke(
            response.rect,
            radius(shapes::radius_lg),
            Stroke::new(1.0_f32, color::primary.resolve_egui(ui.visuals())),
            egui::StrokeKind::Inside,
        );
        ui.painter().rect_stroke(
            response.rect.expand(width / 2.0),
            radius(shapes::radius_lg),
            Stroke::new(1.0_f32, color::ring.resolve_egui(ui.visuals())),
            egui::StrokeKind::Outside,
        );
    }
}

pub fn danger_frame(visuals: &egui::Visuals) -> Frame {
    let danger = color::danger.resolve_egui(visuals);
    Frame::new()
        .fill(danger.gamma_multiply(0.05))
        .stroke(Stroke::new(1.0_f32, danger.gamma_multiply(0.30)))
        .corner_radius(radius(shapes::radius_xl))
        .inner_margin(Margin::symmetric(14, 12))
}
