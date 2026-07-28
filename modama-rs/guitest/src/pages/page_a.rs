use eframe::egui;

use crate::actions::Action;

#[derive(Default)]
pub struct StateA {}

impl StateA {
    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        if ui.button("Next").clicked() {
            Action::GoToEnterName
        } else {
            Action::None
        }
    }
}
