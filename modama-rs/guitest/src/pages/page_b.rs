use std::sync::mpsc::Sender;

use eframe::egui;

use crate::{actions::Action, async_state::{AsyncRequest, AsyncResponse, AsyncState}, background::{BackgroundRequest, BackgroundResponse}};

pub struct StateB {
    new_name: String,
    name_checking: AsyncState<BackgroundResponse>,
    to_bg: Sender<AsyncRequest<BackgroundRequest>>,
}

impl StateB {
    pub fn new(to_bg: Sender<AsyncRequest<BackgroundRequest>>) -> Self {
        Self {
            new_name: "".to_owned(),
            name_checking: AsyncState::Idle,
            to_bg,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        let busy = matches!(self.name_checking, AsyncState::Pending { .. });
        ui.add_enabled_ui(!busy, |ui| {
            ui.text_edit_singleline(&mut self.new_name);
            if ui.button("Next").clicked() {
                self.name_checking.send_request(
                    BackgroundRequest::CheckName {
                        name: self.new_name.clone(),
                    },
                    &self.to_bg,
                );
                Action::None
            } else {
                Action::None
            }
        })
        .inner
    }

    pub fn on_background_message(&mut self, msg: &AsyncResponse<BackgroundResponse>) -> Action {
        self.name_checking.handle(msg);
        match &self.name_checking {
            AsyncState::Idle => Action::None,
            AsyncState::Pending { request_id } => Action::None,
            AsyncState::Ready { payload } => Action::ConfirmName { name: payload }
            AsyncState::Error => Action::None,
        }
    }
}


