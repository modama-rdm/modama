use eframe::egui::{self, RichText};

use crate::styles;

pub fn action_button<'a>(label: &'a str, ui: &egui::Ui) -> egui::Button<'a> {
    egui::Button::new(
        RichText::new(label)
            .strong()
            .color(styles::color::primary_fg.resolve_egui(ui.visuals())),
    )
    .corner_radius(styles::radius(styles::shapes::radius_lg))
    .min_size(egui::Vec2::ZERO)
}
