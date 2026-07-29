use eframe::egui::{self, Color32, RichText, Stroke};

use crate::styles;

/// An application button that owns its focus-ring behavior.
pub struct DesignButton<'a> {
    inner: egui::Button<'a>,
}

impl<'a> DesignButton<'a> {
    pub fn new(inner: egui::Button<'a>) -> Self {
        Self { inner }
    }

    pub fn min_size(mut self, min_size: egui::Vec2) -> Self {
        self.inner = self.inner.min_size(min_size);
        self
    }
}

impl egui::Widget for DesignButton<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let response = ui.add(self.inner);
        if response.enabled() {
            styles::paint_focus_ring(ui, &response);
        }
        response
    }
}

/// Visual treatments defined by the shared data-management UI specification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    Danger,
    DangerOutline,
}

fn button_text_color(visuals: &egui::Visuals, variant: ButtonVariant) -> Color32 {
    match variant {
        ButtonVariant::Primary => styles::color::on_primary::VALUE.resolve_egui(visuals),
        ButtonVariant::Secondary => styles::color::on_surface_variant::VALUE.resolve_egui(visuals),
        ButtonVariant::Danger => Color32::WHITE,
        ButtonVariant::DangerOutline => styles::color::error::VALUE.resolve_egui(visuals),
    }
}

/// Construct a button using one of the semantic design-system variants.
pub fn button<'a>(label: &'a str, ui: &egui::Ui, variant: ButtonVariant) -> DesignButton<'a> {
    let text = RichText::new(label)
        .font(styles::typography::button::font_id())
        .color(button_text_color(ui.visuals(), variant));
    DesignButton::new(button_inner(text, ui, variant))
}

fn button_inner<'a>(
    content: impl egui::IntoAtoms<'a>,
    ui: &egui::Ui,
    variant: ButtonVariant,
) -> egui::Button<'a> {
    let visuals = ui.visuals();
    let radius = styles::radius(styles::rounded::lg::EGUI_POINTS);
    let button = egui::Button::new(content)
        .corner_radius(radius)
        .min_size(egui::Vec2::ZERO);

    match variant {
        // Primary buttons use the application widget visuals, which provide the
        // documented primary-to-primary-hover transition.
        ButtonVariant::Primary => button,
        ButtonVariant::Secondary => button
            .fill(styles::color::surface::VALUE.resolve_egui(visuals))
            .stroke(Stroke::new(
                styles::components::standard_border::width::EGUI_POINTS,
                styles::color::outline::VALUE.resolve_egui(visuals),
            )),
        ButtonVariant::Danger => button
            .fill(styles::color::error::VALUE.resolve_egui(visuals))
            .stroke(Stroke::new(
                styles::components::standard_border::width::EGUI_POINTS,
                styles::color::error::VALUE.resolve_egui(visuals),
            )),
        ButtonVariant::DangerOutline => button
            .fill(styles::color::surface::VALUE.resolve_egui(visuals))
            .stroke(Stroke::new(
                styles::components::standard_border::width::EGUI_POINTS,
                styles::color::error_variant::VALUE.resolve_egui(visuals),
            )),
    }
}

pub fn primary_button<'a>(label: &'a str, ui: &egui::Ui) -> DesignButton<'a> {
    button(label, ui, ButtonVariant::Primary)
}

/// Render a disabled button with an in-button loading indicator.
pub fn loading_button(
    ui: &mut egui::Ui,
    label: &str,
    min_size: egui::Vec2,
    variant: ButtonVariant,
) -> egui::Response {
    let spinner_size = styles::typography::button::FONT_SIZE;
    let gap = styles::spacing::field_label_gap::EGUI_POINTS;
    let spinner_id = ui.next_auto_id().with("loading-spinner");
    let spinner_atom = egui::Atom::custom(spinner_id, egui::Vec2::splat(spinner_size));
    let text = RichText::new(label)
        .font(styles::typography::button::font_id())
        .color(button_text_color(ui.visuals(), variant));
    let button = button_inner((spinner_atom, text), ui, variant)
        .gap(gap)
        .min_size(min_size);

    ui.add_enabled_ui(false, |ui| {
        let response = button.atom_ui(ui);
        if let Some(spinner_rect) = response.rect(spinner_id) {
            egui::Spinner::new()
                .size(spinner_size)
                .color(
                    button_text_color(ui.visuals(), variant)
                        .gamma_multiply(ui.visuals().disabled_alpha),
                )
                .paint_at(ui, spinner_rect);
        }
        response.response
    })
    .inner
}

/// Render a disabled primary button with an in-button loading indicator.
pub fn primary_loading_button(
    ui: &mut egui::Ui,
    label: &str,
    min_size: egui::Vec2,
) -> egui::Response {
    loading_button(ui, label, min_size, ButtonVariant::Primary)
}

pub fn secondary_button<'a>(label: &'a str, ui: &egui::Ui) -> DesignButton<'a> {
    button(label, ui, ButtonVariant::Secondary)
}

pub fn danger_button<'a>(label: &'a str, ui: &egui::Ui) -> DesignButton<'a> {
    button(label, ui, ButtonVariant::Danger)
}

pub fn danger_outline_button<'a>(label: &'a str, ui: &egui::Ui) -> DesignButton<'a> {
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
            for variant in [
                ButtonVariant::Primary,
                ButtonVariant::Secondary,
                ButtonVariant::Danger,
                ButtonVariant::DangerOutline,
            ] {
                loading_button(ui, "Loading", egui::vec2(160.0, 0.0), variant);
            }
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
