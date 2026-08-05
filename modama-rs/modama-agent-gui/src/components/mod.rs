pub mod avatar;
pub mod buttons;
pub mod dialogs;
pub mod dropdown;
pub mod flow_navigation;
pub mod forms;
pub mod list_view;
pub mod toast;

use eframe::egui::{self, RichText};

use crate::styles;

/// Render a full-width error message using the standard danger styling.
pub(crate) fn display_error(ui: &mut egui::Ui, message: &str) {
    styles::danger_frame(ui.visuals()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.colored_label(
            styles::color::error::VALUE.resolve(ui.visuals()),
            RichText::new(message).font(styles::typography::body::font_id()),
        );
    });
}

/// Render a page heading on the shared card-content guide.
pub(crate) fn page_heading(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(styles::content_card_inset());
        ui.add(egui::Label::new(
            RichText::new(text).font(styles::typography::heading::font_id()),
        ));
    });
}
