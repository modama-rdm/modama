use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, Shadow, Stroke};

include!(concat!(env!("OUT_DIR"), "/design_styles.rs"));

pub mod theme {
    use super::*;

    /// Install the design tokens as egui's light and dark visual themes.
    ///
    /// The application starts in light mode. Call `Context::set_theme` later
    /// to switch modes without changing any component code.
    pub fn install(ctx: &egui::Context) {
        let light = visuals(false);
        let dark = visuals(true);
        ctx.set_visuals_of(egui::Theme::Light, light);
        ctx.set_visuals_of(egui::Theme::Dark, dark);
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
        .corner_radius(radius(shapes::radius_lg))
        .inner_margin(Margin::symmetric(14, 12))
}
