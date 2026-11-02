//! Deterministic scenes shared by the user-guide generator and visual tests.

use std::sync::{Arc, Barrier};
use time::{Date, Month};

use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

use crate::{
    app::AgentGuiApp,
    pages::{
        Page, PageConnecting, PageLogin, PageOverview, PageSelectCreateSession, PageSelectProject,
        select_project::representative_projects,
    },
    service::ConnectedAgent,
    state::{ActiveSession, AgentIdentity, ForegroundSession, SignedIn},
    styles,
};

pub const VIEWPORT: egui::Vec2 = egui::vec2(
    styles::components::app_window::width::EGUI_POINTS,
    styles::components::app_window::height::EGUI_POINTS,
);

const PROJECT: &str = "Holo Stacks";
const SESSION: &str = "2026-06-26-holo-stacks";
const USERNAME: &str = "representative.user";
const SESSIONS: [&str; 4] = [
    "2026-06-16-holo-stacks",
    "2026-06-18-holo-stacks",
    "2026-06-20-holo-stacks",
    SESSION,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideScene {
    Connecting,
    OverviewLoggedOut,
    Login,
    ProjectSelection,
    SessionSelection,
    CreateSession,
    ActiveSession,
}

impl GuideScene {
    pub const ALL: [Self; 7] = [
        Self::Connecting,
        Self::OverviewLoggedOut,
        Self::Login,
        Self::ProjectSelection,
        Self::SessionSelection,
        Self::CreateSession,
        Self::ActiveSession,
    ];

    pub const fn filename(self) -> &'static str {
        match self {
            Self::Connecting => "connecting.png",
            Self::OverviewLoggedOut => "overview-logged-out.png",
            Self::Login => "login.png",
            Self::ProjectSelection => "project-selection.png",
            Self::SessionSelection => "session-selection.png",
            Self::CreateSession => "create-session.png",
            Self::ActiveSession => "active-session.png",
        }
    }

    pub fn snapshot_name(self) -> &'static str {
        self.filename()
            .strip_suffix(".png")
            .unwrap_or(self.filename())
    }
}

fn seed_identity(app: &mut AgentGuiApp) {
    app.app_state.identity = Some(AgentIdentity::AgentId {
        agent_name: "k2".to_owned(),
        resource_name: "Holo".to_owned(),
    });
}

fn seed_logged_in(app: &mut AgentGuiApp) {
    seed_identity(app);
    app.app_state.signed_in = Some(SignedIn {
        username: USERNAME.to_owned(),
    });
}

fn build_app(scene: GuideScene, release: Option<Arc<Barrier>>, ctx: &egui::Context) -> AgentGuiApp {
    let mut app = AgentGuiApp::new();

    match scene {
        GuideScene::Connecting => {
            let task_release = release.unwrap_or_else(|| Arc::new(Barrier::new(1)));
            let mut page = PageConnecting::new();
            page.connect_task.spawn(
                move || {
                    task_release.wait();
                    ConnectedAgent {
                        identity: AgentIdentity::Anonymous {
                            hostname: "guide-host".to_owned(),
                        },
                        foreground: ForegroundSession::None,
                    }
                },
                ctx.clone(),
            );
            app.page = Page::Connecting(page);
        }
        GuideScene::OverviewLoggedOut => {
            seed_identity(&mut app);
            app.page = Page::Overview(PageOverview::new());
        }
        GuideScene::Login => {
            seed_identity(&mut app);
            app.page = Page::Login(PageLogin::new());
        }
        GuideScene::ProjectSelection => {
            seed_logged_in(&mut app);
            let mut page = PageSelectProject::new();
            page.available_projects = representative_projects();
            page.selected_project = Some(PROJECT.to_owned());
            app.page = Page::SelectProject(page);
        }
        GuideScene::SessionSelection => {
            seed_logged_in(&mut app);
            let mut page = PageSelectCreateSession::new(PROJECT.to_owned());
            page.available_sessions = SESSIONS
                .iter()
                .map(|session| (*session).to_owned())
                .collect();
            page.selected_session = Some(SESSION.to_owned());
            app.page = Page::SelectCreateSession(page);
        }
        GuideScene::CreateSession => {
            seed_logged_in(&mut app);
            let mut page = PageSelectCreateSession::new(PROJECT.to_owned());
            page.available_sessions = SESSIONS
                .iter()
                .map(|session| (*session).to_owned())
                .collect();
            page.new_session_input = "2026-06-27-alignment-run".to_owned();
            page.session_date =
                Date::from_calendar_date(2026, Month::August, 6).unwrap_or(page.session_date);
            app.page = Page::SelectCreateSession(page);
        }
        GuideScene::ActiveSession => {
            seed_logged_in(&mut app);
            app.app_state.foreground = ForegroundSession::Mounted(ActiveSession {
                share_id: "guide-share".to_owned(),
                project: PROJECT.to_owned(),
                session: SESSION.to_owned(),
                mount_path: Some(format!("/mnt/modama/{PROJECT}/{SESSION}")),
                created_by: Some(USERNAME.to_owned()),
            });
            app.page = Page::Overview(PageOverview::new());
        }
    }

    app
}

/// Render one light-theme guide scene at the canonical viewport size.
pub fn render(scene: GuideScene) -> Result<image::RgbaImage, String> {
    let release = (scene == GuideScene::Connecting).then(|| Arc::new(Barrier::new(2)));
    let task_release = release.clone();
    let mut harness = Harness::new_eframe(move |cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        styles::theme::install(&cc.egui_ctx);
        build_app(scene, task_release, &cc.egui_ctx)
    });
    harness.set_size(VIEWPORT).set_pixels_per_point(1.0);
    harness.ctx.set_theme(egui::Theme::Light);
    harness.run_steps(3);
    if scene == GuideScene::CreateSession {
        harness.get_by_label("Create a new session").click();
        harness.run_steps(2);
        harness.remove_cursor();
        harness.run_steps(1);
    }

    let image = harness.render();
    if let Some(release) = release {
        release.wait();
    }
    image
}
