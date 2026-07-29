pub mod connecting;
pub mod login;
pub mod overview;
pub mod select_create_session;
pub mod select_project;

use eframe::egui;

use crate::{actions::Action, state::ActiveSession};

pub use connecting::PageConnecting;
pub use login::PageLogin;
pub use overview::PageOverview;
pub use select_create_session::PageSelectCreateSession;
pub use select_project::PageSelectProject;

pub enum Page {
    Connecting(PageConnecting),
    Overview(PageOverview),
    Login(PageLogin),
    SelectProject(PageSelectProject),
    SelectCreateSession(PageSelectCreateSession),
}

impl Default for Page {
    fn default() -> Self {
        Self::Connecting(PageConnecting::new())
    }
}

impl Page {
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        session: &Option<ActiveSession>,
    ) -> Action {
        match self {
            Page::Connecting(p) => p.ui(ui, frame),
            Page::Overview(p) => p.ui(ui, frame, session),
            Page::Login(p) => p.ui(ui, frame),
            Page::SelectProject(p) => p.ui(ui, frame),
            Page::SelectCreateSession(p) => p.ui(ui, frame),
        }
    }

    pub fn poll(&mut self) -> Action {
        match self {
            Page::Connecting(p) => p.poll(),
            Page::Overview(p) => p.poll(),
            Page::Login(p) => p.poll(),
            Page::SelectProject(p) => p.poll(),
            Page::SelectCreateSession(p) => p.poll(),
        }
    }
}
