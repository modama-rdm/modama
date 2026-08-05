use std::error::Error;

use clap::{ArgGroup, Parser};
use eframe::egui;
use modama_agent_gui::{
    app::AgentGuiApp,
    pages::{Page, PageLogin, PageOverview, PageSelectCreateSession, PageSelectProject},
    state::{ActiveSession, AgentIdentity, Credentials},
    styles,
};

const DEFAULT_PROJECT: &str = "Holo Stacks";
const DEFAULT_SESSION: &str = "2026-06-26-holo-stacks";
const DEFAULT_USERNAME: &str = "representative.user";

#[derive(Debug, Parser)]
#[command(group(
    ArgGroup::new("page")
        .args([
            "page_connecting",
            "page_overview",
            "page_login",
            "page_project",
            "page_session",
            "page_active_session",
        ])
        .multiple(false)
))]
struct Cli {
    /// Start on the connection screen.
    #[arg(long)]
    page_connecting: bool,

    /// Start on the overview without an active session.
    #[arg(long)]
    page_overview: bool,

    /// Start on the login screen.
    #[arg(long)]
    page_login: bool,

    /// Start on the project selection screen as a logged-in user.
    #[arg(long)]
    page_project: bool,

    /// Start on the session selection screen as a logged-in user.
    #[arg(long)]
    page_session: bool,

    /// Start on the overview with a seeded active session.
    #[arg(long)]
    page_active_session: bool,

    /// Project used by session pages and seeded active sessions.
    #[arg(long, default_value = DEFAULT_PROJECT)]
    project: String,

    /// Session used by the seeded active-session overview.
    #[arg(long, default_value = DEFAULT_SESSION)]
    session: String,
}

impl Cli {
    fn configure_app(self, app: &mut AgentGuiApp) {
        if self.page_connecting {
            return;
        }

        if self.page_overview {
            seed_identity(app);
            app.page = Page::Overview(PageOverview::new());
        } else if self.page_login {
            seed_identity(app);
            app.page = Page::Login(PageLogin::new());
        } else if self.page_project {
            seed_logged_in(app);
            app.page = Page::SelectProject(PageSelectProject::new());
        } else if self.page_session {
            seed_logged_in(app);
            app.page = Page::SelectCreateSession(PageSelectCreateSession::new(self.project));
        } else if self.page_active_session {
            seed_logged_in(app);
            app.app_state.session = Some(ActiveSession {
                share_id: "stub-seeded-share".to_owned(),
                mount_path: Some(format!("/todo/{}/{}/", self.project, self.session)),
                project: self.project,
                session: self.session,
            });
            app.page = Page::Overview(PageOverview::new());
        }
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
    app.app_state.creds = Some(Credentials {
        username: DEFAULT_USERNAME.to_owned(),
        password: "representative-password".to_owned(),
    });
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([
            styles::components::app_window_width::width::EGUI_POINTS,
            styles::components::app_window_height::height::EGUI_POINTS,
        ]),
        ..Default::default()
    };

    eframe::run_native(
        "modama-agent-test-gui",
        options,
        Box::new(move |cctx| {
            egui_extras::install_image_loaders(&cctx.egui_ctx);
            styles::theme::install(&cctx.egui_ctx);
            let mut app = AgentGuiApp::new();
            cli.configure_app(&mut app);

            cctx.egui_ctx.set_theme(egui::Theme::Dark);

            Ok(Box::new(app))
        }),
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(args)
    }

    #[test]
    fn no_page_flag_keeps_the_normal_startup_state() -> Result<(), clap::Error> {
        let cli = parse(&["guitest"])?;
        let mut app = AgentGuiApp::new();

        cli.configure_app(&mut app);

        assert!(matches!(app.page, Page::Connecting(_)));
        assert!(app.app_state.identity.is_none());
        assert!(app.app_state.creds.is_none());
        assert!(app.app_state.session.is_none());
        Ok(())
    }

    #[test]
    fn project_page_is_connected_and_logged_in() -> Result<(), clap::Error> {
        let cli = parse(&["guitest", "--page-project"])?;
        let mut app = AgentGuiApp::new();

        cli.configure_app(&mut app);

        assert!(matches!(app.page, Page::SelectProject(_)));
        assert!(app.app_state.identity.is_some());
        assert!(app.app_state.creds.is_some());
        assert!(app.app_state.session.is_none());
        Ok(())
    }

    #[test]
    fn session_page_uses_the_requested_project() -> Result<(), clap::Error> {
        let cli = parse(&["guitest", "--page-session", "--project", "Test Project"])?;
        let mut app = AgentGuiApp::new();

        cli.configure_app(&mut app);

        let Page::SelectCreateSession(page) = app.page else {
            panic!("expected the session selection page");
        };
        assert_eq!(page.project, "Test Project");
        assert!(app.app_state.identity.is_some());
        assert!(app.app_state.creds.is_some());
        Ok(())
    }

    #[test]
    fn active_session_page_populates_the_full_state() -> Result<(), clap::Error> {
        let cli = parse(&[
            "guitest",
            "--page-active-session",
            "--project",
            "Test Project",
            "--session",
            "Test Session",
        ])?;
        let mut app = AgentGuiApp::new();

        cli.configure_app(&mut app);

        assert!(matches!(app.page, Page::Overview(_)));
        assert!(app.app_state.identity.is_some());
        assert!(app.app_state.creds.is_some());
        assert_eq!(
            app.app_state.session,
            Some(ActiveSession {
                share_id: "stub-seeded-share".to_owned(),
                project: "Test Project".to_owned(),
                session: "Test Session".to_owned(),
                mount_path: Some("/todo/Test Project/Test Session/".to_owned()),
            })
        );
        Ok(())
    }

    #[test]
    fn page_flags_are_mutually_exclusive() {
        let error = parse(&["guitest", "--page-login", "--page-project"]);

        assert!(error.is_err());
    }
}
