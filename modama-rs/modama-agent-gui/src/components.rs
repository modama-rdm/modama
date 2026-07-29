use eframe::egui::{self, Align, Layout, RichText, TextEdit};

use crate::{
    buttons::{danger_button, secondary_button},
    styles,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ConfirmationDialogAction {
    #[default]
    None,
    Cancel,
    Confirm,
}

/// Show a modal confirmation for a destructive action.
pub(crate) fn destructive_confirmation_dialog(
    ctx: &egui::Context,
    visuals: &egui::Visuals,
    id: impl std::hash::Hash,
    title: &str,
    message: &str,
    confirm_label: &str,
    focus_cancel: bool,
) -> ConfirmationDialogAction {
    let mut action = ConfirmationDialogAction::None;
    let response = egui::Modal::new(egui::Id::new(id))
        .frame(styles::card_frame(visuals))
        .show(ctx, |ui| {
            ui.set_width(styles::components::login_card::width::EGUI_POINTS);
            ui.add(
                egui::Label::new(
                    RichText::new(title).font(styles::typography::login_title::font_id()),
                )
                .wrap(),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(message)
                        .font(styles::typography::body::font_id())
                        .color(styles::color::on_surface_variant::VALUE.resolve_egui(ui.visuals())),
                )
                .wrap(),
            );
            ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(danger_button(confirm_label, ui)).clicked() {
                    action = ConfirmationDialogAction::Confirm;
                }
                let cancel = ui.add(secondary_button("Cancel", ui));
                if focus_cancel {
                    cancel.request_focus();
                }
                if cancel.clicked() {
                    action = ConfirmationDialogAction::Cancel;
                }
            });
        });

    if action == ConfirmationDialogAction::None && response.should_close() {
        ConfirmationDialogAction::Cancel
    } else {
        action
    }
}

/// Render a full-width error message using the standard danger styling.
pub(crate) fn display_error(ui: &mut egui::Ui, message: &str) {
    styles::danger_frame(ui.visuals()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.colored_label(
            styles::color::error::VALUE.resolve_egui(ui.visuals()),
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
                .color(styles::color::on_surface_variant::VALUE.resolve_egui(ui.visuals())),
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
