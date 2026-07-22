use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use eframe::egui::{
    self, text::LayoutJob, vec2, FontFamily, FontId, Label, Margin, RichText, Spacing, TextEdit,
    TextFormat,
};

use crate::{
    buttons::action_button,
    state::{ActiveSession, AgentAppState, AgentGuiState},
};

pub struct AgentGuiApp {
    state: Arc<Mutex<AgentAppState>>,
}

impl AgentGuiApp {
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(AgentAppState::new()));
        let state_clone = Arc::clone(&state);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            #[allow(clippy::unwrap_used)]
            let mut s = state_clone.lock().unwrap();
            s.gui_state = AgentGuiState::Overview {};
        });
        Self { state }
    }

    fn render_login(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        username: &str,
        password: &str,
    ) -> Option<AgentGuiState> {
        ui.add(Label::new(RichText::new("Login").size(25.0).strong()));
        let new_u = ui
            .horizontal(|ui| {
                ui.label("Username:");
                let mut username = username.to_owned();
                ui.text_edit_singleline(&mut username);
                username
            })
            .inner;
        let new_p = ui
            .horizontal(|ui| {
                ui.label("Password:");
                let mut password = password.to_owned();
                ui.add(TextEdit::singleline(&mut password).password(true));
                password
            })
            .inner;
        let login_btn = action_button("Login");
        if ui.add(login_btn).clicked() {
            // TODO: dispatch login request to background thread when clicking on the login button,
            // and wait for it to succeed/fail
            return Some(AgentGuiState::SessionSelectProject {
                selected_project: None,
            });
        }
        Some(AgentGuiState::Login {
            username: new_u,
            password: new_p,
        })
    }

    fn render_select_project(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        selected_project: &Option<String>,
    ) -> Option<AgentGuiState> {
        ui.add(Label::new(
            RichText::new("Select a project").size(25.0).strong(),
        ));
        // TODO: scrollable area for project list (as it can become long)
        let project_names = vec!["Project 1", "Project 2", "Project 3", "Project 4"];
        let mut new_state = None;
        for name in project_names {
            let checked = Some(name) == selected_project.as_ref().map(|s| s.as_ref());
            if ui.selectable_label(checked, name).clicked() {
                if checked {
                    new_state = Some(AgentGuiState::SessionSelectProject {
                        selected_project: None,
                    });
                } else {
                    new_state = Some(AgentGuiState::SessionSelectProject {
                        selected_project: Some(name.to_owned()),
                    })
                }
            }
        }

        let enabled = selected_project.is_some();

        if ui
            .add_enabled(enabled, action_button("Select Project"))
            .clicked()
        {
            // TODO: dispatch action to fetch sessions in project?
            if let Some(project_name) = selected_project {
                new_state = Some(AgentGuiState::SelectCreateSession {
                    project: project_name.clone(),
                    selected_session: None,
                    new_session_input: "".to_owned(),
                });
            }
        }

        new_state
    }

    fn render_select_create_session(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        selected_project: &str,
        selected_session: &Option<String>,
        new_session_input: &str,
    ) -> Option<AgentGuiState> {
        ui.add(Label::new(
            RichText::new(format!(
                "Select or create a session in project '{selected_project}'"
            ))
            .size(25.0)
            .strong(),
        ));

        // TODO: scrollable area for session list (as it can become long)
        let project_names = vec![
            "2026-06-16-holo-stacks",
            "2026-06-18-holo-stacks",
            "2026-06-20-holo-stacks",
            "2026-06-26-holo-stacks",
        ];
        let mut new_state = None;
        for name in project_names {
            let name_is_selected = Some(name) == selected_session.as_ref().map(|s| s.as_ref());
            if ui.selectable_label(name_is_selected, name).clicked() {
                // toggle selection:
                if name_is_selected {
                    new_state = Some(AgentGuiState::SelectCreateSession {
                        project: selected_project.to_owned(),
                        selected_session: None,
                        new_session_input: new_session_input.to_owned(),
                    });
                } else {
                    new_state = Some(AgentGuiState::SelectCreateSession {
                        project: selected_project.to_owned(),
                        selected_session: Some(name.to_owned()),
                        new_session_input: new_session_input.to_owned(),
                    })
                }
            }
        }

        let enabled = selected_session.is_some();
        if ui
            .add_enabled(enabled, action_button("Select Session"))
            .clicked()
        {
            // TODO: dispatch action to fetch sessions in project?
            if let Some(session_name) = selected_session {
                todo!("dispatch fetch, update the active session in the state etc.");
            }
        }

        new_state
    }

    fn render_overview(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        session: &Option<ActiveSession>,
    ) -> Option<AgentGuiState> {
        ui.add(Label::new(RichText::new("Overview").size(25.0).strong()));
        if let Some(session) = session {
            // FIXME: use bold instead of italics (include ttf files in the repo)
            let mut job = LayoutJob::default();
            job.append("Active session ", 0.0, TextFormat::default());
            job.append(
                &session.session,
                0.0,
                TextFormat {
                    font_id: FontId::new(14.0, FontFamily::Proportional),
                    italics: true,
                    ..Default::default()
                },
            );
            job.append(" in project ", 0.0, TextFormat::default());
            job.append(
                &session.project,
                0.0,
                TextFormat {
                    font_id: FontId::new(14.0, FontFamily::Proportional),
                    italics: true,
                    ..Default::default()
                },
            );
            ui.add(Label::new(job));
        };

        let mut new_state = None;

        if ui.add(action_button("Create Session")).clicked() {
            new_state = Some(AgentGuiState::Login {
                username: "".to_owned(),
                password: "".to_owned(),
            });
        }

        new_state
    }

    fn render_connecting(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.vertical_centered(|ui| {
            ui.spinner();
            ui.label("Connecting to modama...");
        });
    }
}

const H: f32 = 15.0;
const H_I: i8 = 15;
const V: f32 = 10.0;
const V_I: i8 = 10;

impl eframe::App for AgentGuiApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        ui.style_mut().visuals = egui::Visuals::light();
        ui.style_mut().spacing = Spacing {
            item_spacing: vec2(H, V),
            window_margin: Margin {
                left: H_I,
                right: H_I,
                top: V_I,
                bottom: V_I,
            },
            button_padding: vec2(H, V),
            ..Default::default()
        };
        let state = Arc::clone(&self.state);

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.add(
                egui::Image::new(egui::include_image!("../../../branding/modama.svg"))
                    .max_width(256.0),
            );
            egui::Frame::default().inner_margin(27.0).show(ui, |ui| {
                // as we borrow from the locked state here, we can't
                // replace it in the render_* functions, so instead
                // these functions return a new state, or None if the current state should be kept.
                let new_state = {
                    #[allow(clippy::expect_used)]
                    let state_here = &state.lock().expect("lock");
                    match &state_here.gui_state {
                        AgentGuiState::Connecting => {
                            self.render_connecting(ui, frame);
                            None
                        }
                        AgentGuiState::Overview {} => {
                            self.render_overview(ui, frame, &state_here.session)
                        }
                        AgentGuiState::Login { username, password } => {
                            self.render_login(ui, frame, username, password)
                        }
                        AgentGuiState::SessionSelectProject { selected_project } => {
                            self.render_select_project(ui, frame, selected_project)
                        }
                        AgentGuiState::SelectCreateSession {
                            project,
                            selected_session,
                            new_session_input,
                        } => self.render_select_create_session(
                            ui,
                            frame,
                            project,
                            selected_session,
                            new_session_input,
                        ),
                    }
                };

                if let Some(new_state) = new_state {
                    #[allow(clippy::expect_used)]
                    let mut state_locked = state.lock().expect("lock");
                    state_locked.gui_state = new_state;
                }
            });
        });
    }
}
