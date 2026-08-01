use eframe::egui::{self, RichText, TextEdit};

use crate::styles;

/// Render a labeled, full-width text input using the standard form styling.
pub(crate) fn labeled_text_input(
    ui: &mut egui::Ui,
    label: &str,
    text_edit: TextEdit<'_>,
) -> egui::Response {
    labeled_text_input_with_insets(
        ui,
        label,
        text_edit,
        0.0,
        styles::spacing::input_padding_x::EGUI_POINTS,
    )
}

/// Render a standard labeled text input with independent label and text insets.
pub(crate) fn labeled_text_input_with_insets(
    ui: &mut egui::Ui,
    label: &str,
    text_edit: TextEdit<'_>,
    label_inset: f32,
    input_padding_x: f32,
) -> egui::Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let label_response = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(label_inset);
                field_label(ui, label)
            })
            .inner;
        ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
        let response = ui
            .add(
                text_edit
                    .font(styles::typography::body::font_id())
                    .margin(egui::Margin::symmetric(
                        input_padding_x as i8,
                        styles::spacing::input_padding_y::EGUI_POINTS as i8,
                    ))
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(label_response.id);
        styles::paint_focus_ring(ui, &response);
        response
    })
    .inner
}

/// Render the standard visual label used above form controls.
pub(crate) fn field_label(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.label(
        RichText::new(label)
            .font(styles::typography::label::font_id())
            .color(styles::color::on_surface_variant::VALUE.resolve(ui.visuals())),
    )
}
