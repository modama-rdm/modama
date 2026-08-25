use eframe::egui::{self, Align, Layout, RichText};

use crate::{
    components::buttons::{danger_button, secondary_button},
    styles,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConfirmationDialogAction {
    #[default]
    None,
    Cancel,
    Confirm,
}

/// Show a modal confirmation for a destructive action.
pub fn destructive_confirmation_dialog(
    ctx: &egui::Context,
    visuals: &egui::Visuals,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    message: &str,
    confirm_label: &str,
    focus_cancel: bool,
) -> ConfirmationDialogAction {
    let response = egui::Modal::new(egui::Id::new(id))
        .frame(styles::card_frame(visuals))
        .show(ctx, |ui| {
            ui.set_width(styles::components::dialog::width::EGUI_POINTS);
            ui.add(
                egui::Label::new(
                    RichText::new(title).font(styles::typography::section_heading::font_id()),
                )
                .wrap(),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(message)
                        .font(styles::typography::body::font_id())
                        .color(styles::color::on_surface_variant::VALUE.resolve(ui.visuals())),
                )
                .wrap(),
            );
            ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let confirm_clicked = ui.add(danger_button(confirm_label, ui)).clicked();
                let cancel = ui.add(secondary_button("Cancel", ui));
                if focus_cancel {
                    cancel.request_focus();
                }
                if cancel.clicked() {
                    ConfirmationDialogAction::Cancel
                } else if confirm_clicked {
                    ConfirmationDialogAction::Confirm
                } else {
                    ConfirmationDialogAction::None
                }
            })
            .inner
        });

    if response.inner == ConfirmationDialogAction::None && response.should_close() {
        ConfirmationDialogAction::Cancel
    } else {
        response.inner
    }
}
