use eframe::egui::{self, Label, RichText};
use std::time::Duration;

use crate::{
    actions::Action, async_state::AsyncState, buttons::primary_button, state::ActiveSession, styles,
};

#[derive(Debug, Clone)]
pub struct FetchSessionsRequest {
    pub project: String,
}

impl FetchSessionsRequest {
    pub fn execute(&self) -> Vec<String> {
        std::thread::sleep(Duration::from_millis(500));
        vec![
            format!("{}-holo-stacks", "2026-06-16"),
            format!("{}-holo-stacks", "2026-06-18"),
            format!("{}-holo-stacks", "2026-06-20"),
            format!("{}-holo-stacks", "2026-06-26"),
        ]
    }
}

#[derive(Debug, Clone)]
pub struct CreateIngestRequest {
    pub project: String,
    pub session: String,
}

impl CreateIngestRequest {
    pub fn execute(&self) -> ActiveSession {
        std::thread::sleep(Duration::from_millis(500));

        ActiveSession {
            project: self.project.clone(),
            session: self.session.clone(),
            mount_path: Some(format!("/todo/{}/{}/", self.project, self.session)),
        }
    }
}

pub struct PageSelectCreateSession {
    pub project: String,
    pub selected_session: Option<String>,
    pub new_session_input: String,
    pub sessions_task: AsyncState<Vec<String>>,
    pub create_ingest_task: AsyncState<ActiveSession>,
    pub available_sessions: Vec<String>,
}

impl PageSelectCreateSession {
    pub fn new(project: String) -> Self {
        Self {
            project,
            selected_session: None,
            new_session_input: String::new(),
            sessions_task: AsyncState::Idle,
            available_sessions: Vec::new(),
            create_ingest_task: AsyncState::Idle,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        if matches!(self.sessions_task, AsyncState::Idle) && self.available_sessions.is_empty() {
            let req = FetchSessionsRequest {
                project: self.project.clone(),
            };
            let ctx = ui.ctx().clone();
            self.sessions_task.spawn(move || req.execute(), ctx);
        }

        ui.add(Label::new(
            RichText::new(format!(
                "Select or create a session in project '{}'",
                self.project
            ))
            .font(styles::typography::heading::font_id()),
        ));

        if self.sessions_task.is_pending() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Fetching sessions...").font(styles::typography::body::font_id()),
                );
            });
        } else {
            for name in &self.available_sessions {
                let is_selected = Some(name.as_str()) == self.selected_session.as_deref();
                if ui
                    .selectable_label(
                        is_selected,
                        RichText::new(name).font(styles::typography::mono::font_id()),
                    )
                    .clicked()
                {
                    if is_selected {
                        self.selected_session = None;
                    } else {
                        self.selected_session = Some(name.clone());
                    }
                }
            }

            let enabled = self.selected_session.is_some();
            if ui
                .add_enabled(enabled, primary_button("Select Session", ui))
                .clicked()
            {
                if let Some(session_name) = &self.selected_session {
                    let task = CreateIngestRequest {
                        project: self.project.clone(),
                        session: session_name.clone(),
                    };
                    self.create_ingest_task
                        .spawn(move || task.execute(), ui.ctx().clone());
                }
            }
        }

        Action::None
    }

    pub fn poll(&mut self) -> Action {
        if let Some(sessions) = self.sessions_task.poll().take_ready() {
            self.available_sessions = sessions;
        }
        if let Some(active_ingest) = self.create_ingest_task.poll().take_ready() {
            return Action::SelectSession {
                session: active_ingest,
            };
        }
        Action::None
    }

    pub fn is_busy(&self) -> bool {
        self.sessions_task.is_pending()
            || self.create_ingest_task.is_pending()
            || (matches!(self.sessions_task, AsyncState::Idle)
                && self.available_sessions.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_sessions_request() {
        let req = FetchSessionsRequest {
            project: "test".to_string(),
        };
        let res = req.execute();
        assert_eq!(res.len(), 4);
    }
}
