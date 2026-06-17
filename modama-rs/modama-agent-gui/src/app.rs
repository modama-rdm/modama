use eframe::egui;

use crate::state::AgentGuiState;

pub struct AgentGuiApp {
    state: AgentGuiState,
}

impl AgentGuiApp {
    pub fn new() -> Self {
        Self {
            state: AgentGuiState::Connecting,
        }
    }

    fn render_connecting(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.image(egui::include_image!("../../../branding/modama.svg"));
        });
    }
}

impl eframe::App for AgentGuiApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        match &self.state {
            AgentGuiState::Connecting => self.render_connecting(ui, frame),
            AgentGuiState::Overview { session } => todo!(),
            AgentGuiState::Login => todo!(),
            AgentGuiState::SessionSelectProject => todo!(),
            AgentGuiState::SelectCreateSession => todo!(),
        }
    }
}
