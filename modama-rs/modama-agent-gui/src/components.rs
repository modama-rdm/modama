use eframe::egui::{self, RichText, TextEdit};

use crate::styles;

/// Render a full-width error message using the standard danger styling.
pub(crate) fn display_error(ui: &mut egui::Ui, message: &str) {
    styles::danger_frame(ui.visuals()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.colored_label(
            styles::color::danger::VALUE.resolve_egui(ui.visuals()),
            RichText::new(message).font(styles::typography::body::font_id()),
        );
    });
}

/// Render a labeled, full-width text input using the standard form styling.
pub(crate) fn labeled_text_input(
    ui: &mut egui::Ui,
    label: &str,
    text_edit: TextEdit<'_>,
) -> egui::Response {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(label)
                .font(styles::typography::label::font_id())
                .color(styles::color::fg_muted::VALUE.resolve_egui(ui.visuals())),
        );
        ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
        let response = ui.add(
            text_edit
                .font(styles::typography::body::font_id())
                .frame(styles::input_frame(
                    ui.visuals(),
                    egui::Margin::symmetric(
                        styles::spacing::input_padding_x::EGUI_POINTS as i8,
                        styles::spacing::input_padding_y::EGUI_POINTS as i8,
                    ),
                ))
                .desired_width(f32::INFINITY),
        );
        styles::paint_focus_ring(ui, &response);
        response
    })
    .inner
}
