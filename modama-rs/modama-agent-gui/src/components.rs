use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, TextEdit};

use crate::{
    buttons::{danger_button, secondary_button},
    styles,
};

const IDENTITY_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x25, 0x63, 0xeb),
    Color32::from_rgb(0x7c, 0x3a, 0xed),
    Color32::from_rgb(0x05, 0x96, 0x69),
    Color32::from_rgb(0xd9, 0x77, 0x06),
    Color32::from_rgb(0xdc, 0x26, 0x26),
    Color32::from_rgb(0x08, 0x91, 0xb2),
    Color32::from_rgb(0xbe, 0x18, 0x5d),
    Color32::from_rgb(0x65, 0xa3, 0x0d),
];

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
                    RichText::new(title).font(styles::typography::section_heading::font_id()),
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

/// Render a page heading on the shared card-content guide.
pub(crate) fn page_heading(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(styles::content_card_inset());
        ui.add(egui::Label::new(
            RichText::new(text).font(styles::typography::heading::font_id()),
        ));
    });
}

/// Render a labeled, full-width text input using the standard form styling.
pub(crate) fn labeled_text_input(
    ui: &mut egui::Ui,
    label: &str,
    text_edit: TextEdit<'_>,
) -> egui::Response {
    ui.vertical(|ui| {
        let label_response = ui.label(
            RichText::new(label)
                .font(styles::typography::label::font_id())
                .color(styles::color::on_surface_variant::VALUE.resolve_egui(ui.visuals())),
        );
        ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
        let response = ui
            .add(
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
            )
            .labelled_by(label_response.id);
        styles::paint_focus_ring(ui, &response);
        response
    })
    .inner
}

/// Render a deterministic user avatar at an explicitly selected size.
pub(crate) fn avatar(ui: &mut egui::Ui, username: &str, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(size), Sense::hover());
    let color = IDENTITY_COLORS[username_hash(username) % IDENTITY_COLORS.len()];
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials(username),
        styles::typography::button::font_id(),
        Color32::WHITE,
    );
    response.on_hover_text(username)
}

fn username_hash(username: &str) -> usize {
    username
        .chars()
        .fold(0_i32, |hash, character| {
            hash.wrapping_mul(31)
                .wrapping_add(i32::try_from(u32::from(character)).unwrap_or_default())
        })
        .unsigned_abs() as usize
}

fn initials(username: &str) -> String {
    username.chars().take(2).collect::<String>().to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_identity_is_stable() {
        assert_eq!(initials("alex"), "AL");
        assert_eq!(username_hash("alex"), username_hash("alex"));
        assert_ne!(username_hash("alex"), username_hash("sam"));
    }
}
