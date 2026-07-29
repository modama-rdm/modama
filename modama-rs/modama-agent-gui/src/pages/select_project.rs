use eframe::egui::{self, Label, RichText};
use std::time::Duration;

use crate::{actions::Action, async_state::AsyncState, buttons::primary_button, styles};

#[derive(Debug, Clone)]
pub struct FetchProjectsRequest;

impl FetchProjectsRequest {
    pub fn execute(&self) -> Vec<String> {
        std::thread::sleep(Duration::from_millis(500));
        vec![
            "Project 1".to_string(),
            "Project 2".to_string(),
            "Project 3".to_string(),
            "Project 4".to_string(),
        ]
    }
}

pub struct PageSelectProject {
    pub selected_project: Option<String>,
    pub projects_task: AsyncState<Vec<String>>,
    pub available_projects: Vec<String>,
}

impl Default for PageSelectProject {
    fn default() -> Self {
        Self::new()
    }
}

impl PageSelectProject {
    pub fn new() -> Self {
        Self {
            selected_project: None,
            projects_task: AsyncState::Idle,
            available_projects: Vec::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        if matches!(self.projects_task, AsyncState::Idle) && self.available_projects.is_empty() {
            let ctx = ui.ctx().clone();
            self.projects_task
                .spawn(move || FetchProjectsRequest.execute(), ctx);
        }

        ui.add(Label::new(
            RichText::new("Select a project")
                .font(styles::font::from_token(styles::typography::heading)),
        ));

        if self.projects_task.is_pending() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Fetching projects...")
                        .font(styles::font::from_token(styles::typography::body)),
                );
            });
        } else {
            for name in &self.available_projects {
                let checked = Some(name.as_str()) == self.selected_project.as_deref();
                if ui
                    .selectable_label(
                        checked,
                        RichText::new(name)
                            .font(styles::font::from_token(styles::typography::mono)),
                    )
                    .clicked()
                {
                    if checked {
                        self.selected_project = None;
                    } else {
                        self.selected_project = Some(name.clone());
                    }
                }
            }

            let enabled = self.selected_project.is_some();
            if ui
                .add_enabled(enabled, primary_button("Select Project", ui))
                .clicked()
            {
                if let Some(project) = &self.selected_project {
                    return Action::SelectProject {
                        project: project.clone(),
                    };
                }
            }
        }

        Action::None
    }

    pub fn poll(&mut self) -> Action {
        if let Some(projects) = self.projects_task.poll().take_ready() {
            self.available_projects = projects;
        }
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_projects_request() {
        let req = FetchProjectsRequest;
        let res = req.execute();
        assert_eq!(res.len(), 4);
        assert_eq!(res[0], "Project 1");
    }
}
