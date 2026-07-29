use eframe::egui::{self, Color32, RichText, Stroke};

use crate::styles;

/// Visual treatments defined by the shared data-management UI specification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    Danger,
    DangerOutline,
}

/// Construct a button using one of the semantic design-system variants.
pub fn button<'a>(label: &'a str, ui: &egui::Ui, variant: ButtonVariant) -> egui::Button<'a> {
    let visuals = ui.visuals();
    let radius = styles::radius(styles::shapes::radius_lg::EGUI_POINTS);
    let text_color = match variant {
        ButtonVariant::Primary => styles::color::primary_fg::VALUE.resolve_egui(visuals),
        ButtonVariant::Secondary => styles::color::fg_muted::VALUE.resolve_egui(visuals),
        ButtonVariant::Danger => Color32::WHITE,
        ButtonVariant::DangerOutline => styles::color::danger::VALUE.resolve_egui(visuals),
    };
    let button = egui::Button::new(
        RichText::new(label)
            .font(styles::typography::button::font_id())
            .color(text_color),
    )
    .corner_radius(radius)
    .min_size(egui::Vec2::ZERO);

    match variant {
        // Primary buttons use the application widget visuals, which provide the
        // documented primary-to-primary-hover transition.
        ButtonVariant::Primary => button,
        ButtonVariant::Secondary => button
            .fill(styles::color::surface::VALUE.resolve_egui(visuals))
            .stroke(Stroke::new(
                1.0_f32,
                styles::color::border::VALUE.resolve_egui(visuals),
            )),
        ButtonVariant::Danger => button
            .fill(styles::color::danger::VALUE.resolve_egui(visuals))
            .stroke(Stroke::new(
                1.0_f32,
                styles::color::danger::VALUE.resolve_egui(visuals),
            )),
        ButtonVariant::DangerOutline => button
            .fill(styles::color::surface::VALUE.resolve_egui(visuals))
            .stroke(Stroke::new(
                1.0_f32,
                styles::color::danger::VALUE
                    .resolve_egui(visuals)
                    .gamma_multiply(0.30),
            )),
    }
}

pub fn primary_button<'a>(label: &'a str, ui: &egui::Ui) -> egui::Button<'a> {
    button(label, ui, ButtonVariant::Primary)
}

/// Render a disabled primary button with an in-button loading indicator.
pub fn primary_loading_button(
    ui: &mut egui::Ui,
    label: &str,
    min_size: egui::Vec2,
) -> egui::Response {
    let response = ui.add_enabled(false, primary_button(label, ui).min_size(min_size));
    let spinner_size = styles::typography::button::FONT_SIZE;
    let label_width = ui
        .painter()
        .layout_no_wrap(
            label.to_owned(),
            styles::typography::button::font_id(),
            Color32::TRANSPARENT,
        )
        .rect
        .width();
    let gap = styles::spacing::field_label_gap::EGUI_POINTS;
    let spinner_center =
        response.rect.center() - egui::vec2(label_width / 2.0 + gap + spinner_size / 2.0, 0.0);
    let spinner_rect =
        egui::Rect::from_center_size(spinner_center, egui::vec2(spinner_size, spinner_size));
    egui::Spinner::new()
        .size(spinner_size)
        .color(
            styles::color::primary_fg::VALUE
                .resolve_egui(ui.visuals())
                .gamma_multiply(ui.visuals().disabled_alpha),
        )
        .paint_at(ui, spinner_rect);
    response
}

pub fn secondary_button<'a>(label: &'a str, ui: &egui::Ui) -> egui::Button<'a> {
    button(label, ui, ButtonVariant::Secondary)
}

pub fn danger_button<'a>(label: &'a str, ui: &egui::Ui) -> egui::Button<'a> {
    button(label, ui, ButtonVariant::Danger)
}

pub fn danger_outline_button<'a>(label: &'a str, ui: &egui::Ui) -> egui::Button<'a> {
    button(label, ui, ButtonVariant::DangerOutline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_and_public_wrapper_renders() {
        let ctx = egui::Context::default();
        styles::theme::install(&ctx);

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            for variant in [
                ButtonVariant::Primary,
                ButtonVariant::Secondary,
                ButtonVariant::Danger,
                ButtonVariant::DangerOutline,
            ] {
                ui.add(button("Button", ui, variant));
            }
            ui.add(primary_button("Primary", ui));
            primary_loading_button(ui, "Loading", egui::vec2(160.0, 0.0));
            ui.add(secondary_button("Secondary", ui));
            ui.add(danger_button("Danger", ui));
            ui.add(danger_outline_button("Danger outline", ui));
        });
    }

    #[test]
    fn primary_button_uses_design_token_padding() {
        let ctx = egui::Context::default();
        styles::theme::install(&ctx);
        let size = std::cell::Cell::new(egui::Vec2::ZERO);

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            size.set(ui.add(primary_button("Action", ui)).rect.size());
        });

        let minimum_height = styles::typography::button::FONT_SIZE
            + 2.0 * styles::spacing::button_padding_y::EGUI_POINTS
            - 2.0;
        assert!(size.get().y >= minimum_height);
        assert!(size.get().x > 2.0 * styles::spacing::button_padding_x::EGUI_POINTS);
    }
}
