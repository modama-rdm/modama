use eframe::egui;

use crate::actions::Action;

#[derive(Default)]
pub struct StateC {
    name: String,
}

impl StateC {
    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        ui.label(&self.name);
        Action::None
    }

    pub fn new(name: String) -> Self {
        Self { name }
    }
}


