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

    match variant {
        // Primary buttons use the application widget visuals, which provide the
        // documented primary-to-primary-hover transition.
        ButtonVariant::Primary => egui::Button::new(
            RichText::new(label)
                .font(styles::font::button())
                .color(styles::color::primary_fg.resolve_egui(visuals)),
        )
        .corner_radius(radius)
        .min_size(egui::Vec2::ZERO),
        ButtonVariant::Secondary => egui::Button::new(
            RichText::new(label)
                .font(styles::font::button())
                .color(styles::color::fg_muted.resolve_egui(visuals)),
        )
        .fill(styles::color::surface.resolve_egui(visuals))
        .stroke(Stroke::new(
            1.0_f32,
            styles::color::border.resolve_egui(visuals),
        ))
        .corner_radius(radius)
        .min_size(egui::Vec2::ZERO),
        ButtonVariant::Danger => egui::Button::new(
            RichText::new(label)
                .font(styles::font::button())
                .color(Color32::WHITE),
        )
        .fill(styles::color::danger.resolve_egui(visuals))
        .stroke(Stroke::new(
            1.0_f32,
            styles::color::danger.resolve_egui(visuals),
        ))
        .corner_radius(radius)
        .min_size(egui::Vec2::ZERO),
        ButtonVariant::DangerOutline => egui::Button::new(
            RichText::new(label)
                .font(styles::font::button())
                .color(styles::color::danger.resolve_egui(visuals)),
        )
        .fill(styles::color::surface.resolve_egui(visuals))
        .stroke(Stroke::new(
            1.0_f32,
            styles::color::danger
                .resolve_egui(visuals)
                .gamma_multiply(0.30),
        ))
        .corner_radius(radius)
        .min_size(egui::Vec2::ZERO),
    }
}

/// Backwards-compatible name for the primary action button.
pub fn action_button<'a>(label: &'a str, ui: &egui::Ui) -> egui::Button<'a> {
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
