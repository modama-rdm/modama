#[cfg(feature = "autostart")]
pub mod autostart_offer;
pub mod avatar;
pub mod buttons;
pub mod copy_field;
pub mod dialogs;
pub mod dropdown;
pub mod flow_navigation;
pub mod forms;
pub mod list_view;
pub mod path_line;
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
        ui.add_space(styles::guide_inset());
        ui.add(egui::Label::new(
            RichText::new(text).font(styles::typography::heading::font_id()),
        ));
    });
}

/// The small upper-case label above a value, like the web GUI's detail and copy-field
/// labels; its accessible name keeps `text`'s case, so screen readers read it as words
pub(crate) fn detail_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let response = ui.add(egui::Label::new(
        RichText::new(text.to_uppercase())
            .font(styles::typography::detail_label::font_id())
            .extra_letter_spacing(styles::typography::detail_label::LETTER_SPACING)
            .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
    ));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    response
}
