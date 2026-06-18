use eframe::egui::{self, Color32, RichText};

pub fn action_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).strong()).fill(Color32::from_rgb(0, 90, 230))
}
