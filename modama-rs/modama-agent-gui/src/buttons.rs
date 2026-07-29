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
    let radius = styles::radius(styles::shapes::radius_lg);
    let text_color = match variant {
        ButtonVariant::Primary => styles::color::primary_fg.resolve_egui(visuals),
        ButtonVariant::Secondary => styles::color::fg_muted.resolve_egui(visuals),
        ButtonVariant::Danger => Color32::WHITE,
        ButtonVariant::DangerOutline => styles::color::danger.resolve_egui(visuals),
    };
    let button = egui::Button::new(
        RichText::new(label)
            .font(styles::font::from_token(styles::typography::button))
            .color(text_color),
    )
    .corner_radius(radius)
    .min_size(egui::Vec2::ZERO);

    match variant {
        // Primary buttons use the application widget visuals, which provide the
        // documented primary-to-primary-hover transition.
        ButtonVariant::Primary => button,
        ButtonVariant::Secondary => button
            .fill(styles::color::surface.resolve_egui(visuals))
            .stroke(Stroke::new(
                1.0_f32,
                styles::color::border.resolve_egui(visuals),
            )),
        ButtonVariant::Danger => button
            .fill(styles::color::danger.resolve_egui(visuals))
            .stroke(Stroke::new(
                1.0_f32,
                styles::color::danger.resolve_egui(visuals),
            )),
        ButtonVariant::DangerOutline => button
            .fill(styles::color::surface.resolve_egui(visuals))
            .stroke(Stroke::new(
                1.0_f32,
                styles::color::danger
                    .resolve_egui(visuals)
                    .gamma_multiply(0.30),
            )),
    }
}

pub fn primary_button<'a>(label: &'a str, ui: &egui::Ui) -> egui::Button<'a> {
    button(label, ui, ButtonVariant::Primary)
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
            ui.add(secondary_button("Secondary", ui));
            ui.add(danger_button("Danger", ui));
            ui.add(danger_outline_button("Danger outline", ui));
        });
    }
}
