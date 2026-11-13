use eframe::egui::{self, Align, Layout, RichText};

use crate::{
    components::buttons::{ButtonVariant, button, secondary_button},
    styles,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConfirmationDialogAction {
    #[default]
    None,
    Cancel,
    Confirm,
}

/// One of the choices of a [`choice_dialog`], besides cancelling
pub struct DialogChoice<'a, T> {
    pub label: &'a str,
    pub variant: ButtonVariant,
    pub value: T,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChoiceDialogAction<T> {
    #[default]
    None,
    Cancel,
    Choose(T),
}

/// The button of a dialog that gets the keyboard focus
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogFocus {
    Cancel,
    FirstChoice,
}

/// Show a modal with a message, the `choices` and Cancel. The first choice is placed at the
/// right edge, the others to its left, and Cancel last. `focus` moves the keyboard focus;
/// pass it once, in the frame the dialog opens.
#[allow(
    clippy::too_many_arguments,
    reason = "a dialog's parts; a builder would only move them around"
)]
pub fn choice_dialog<T: Copy>(
    ctx: &egui::Context,
    visuals: &egui::Visuals,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    message: &str,
    choices: &[DialogChoice<'_, T>],
    focus: Option<DialogFocus>,
) -> ChoiceDialogAction<T> {
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
                let chosen = choices
                    .iter()
                    .enumerate()
                    .map(|(index, choice)| {
                        let response = ui.add(button(choice.label, ui, choice.variant));
                        if index == 0 && focus == Some(DialogFocus::FirstChoice) {
                            response.request_focus();
                        }
                        response.clicked().then_some(choice.value)
                    })
                    .fold(None, Option::or);
                let cancel = ui.add(secondary_button("Cancel", ui));
                if focus == Some(DialogFocus::Cancel) {
                    cancel.request_focus();
                }
                if cancel.clicked() {
                    ChoiceDialogAction::Cancel
                } else if let Some(value) = chosen {
                    ChoiceDialogAction::Choose(value)
                } else {
                    ChoiceDialogAction::None
                }
            })
            .inner
        });

    if matches!(response.inner, ChoiceDialogAction::None) && response.should_close() {
        ChoiceDialogAction::Cancel
    } else {
        response.inner
    }
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
    let choices = [DialogChoice {
        label: confirm_label,
        variant: ButtonVariant::Danger,
        value: (),
    }];
    let focus = focus_cancel.then_some(DialogFocus::Cancel);
    match choice_dialog(ctx, visuals, id, title, message, &choices, focus) {
        ChoiceDialogAction::None => ConfirmationDialogAction::None,
        ChoiceDialogAction::Cancel => ConfirmationDialogAction::Cancel,
        ChoiceDialogAction::Choose(()) => ConfirmationDialogAction::Confirm,
    }
}
