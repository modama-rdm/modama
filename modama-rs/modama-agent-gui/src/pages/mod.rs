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
    pub fn is_busy(&self) -> bool {
        match self {
            Self::Connecting(page) => page.is_busy(),
            Self::Overview(page) => page.is_busy(),
            Self::Login(page) => page.is_busy(),
            Self::SelectProject(page) => page.is_busy(),
            Self::SelectCreateSession(page) => page.is_busy(),
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_state_covers_initial_and_idle_pages() {
        assert!(Page::Connecting(PageConnecting::new()).is_busy());
        assert!(!Page::Login(PageLogin::new()).is_busy());
        assert!(!Page::Overview(PageOverview::new()).is_busy());
        assert!(Page::SelectProject(PageSelectProject::new()).is_busy());
        assert!(
            Page::SelectCreateSession(PageSelectCreateSession::new("project".to_string()))
                .is_busy()
        );

        let mut projects = PageSelectProject::new();
        projects.available_projects.push("project".to_string());
        assert!(!Page::SelectProject(projects).is_busy());

        let mut sessions = PageSelectCreateSession::new("project".to_string());
        sessions.available_sessions.push("session".to_string());
        assert!(!Page::SelectCreateSession(sessions).is_busy());
    }
}
