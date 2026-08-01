use crate::top_bar::avatar::avatar;
use std::time::Duration;

use eframe::egui::{self, Align, Color32, FontFamily, FontId, Layout, RichText, Sense, Stroke};

use crate::{
    actions::Action,
    async_state::AsyncState,
    components::buttons::{loading_button, primary_button, secondary_button, ButtonVariant, DesignButton},
    components::avatar,
    state::AgentIdentity,
    styles,
};

pub(crate) struct TopBar {
    logout_task: AsyncState<()>,
}

impl Default for TopBar {
    fn default() -> Self {
        Self {
            logout_task: AsyncState::Idle,
        }
    }
}

impl TopBar {
    pub(crate) fn show_inside(
        &mut self,
        parent: &mut egui::Ui,
        username: Option<&str>,
        agent_identity: &Option<AgentIdentity>,
        page_busy: bool,
        show_login_action: bool,
    ) -> Action {
        let panel = egui::Panel::top("agent-top-bar")
            .exact_size(styles::components::top_bar::height::EGUI_POINTS)
            .frame(
                egui::Frame::new()
                    .fill(styles::color::surface::VALUE.resolve(parent.visuals()))
                    .inner_margin(egui::Margin::symmetric(
                        styles::components::top_bar::padding::EGUI_POINTS as i8,
                        0,
                    )),
            );

        panel
            .show_inside(parent, |ui| {
                let action =
                    self.content(ui, username, agent_identity, page_busy, show_login_action);
                let border = styles::color::outline::VALUE.resolve(ui.visuals());
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.max_rect().bottom()
                        - styles::components::standard_border::width::EGUI_POINTS,
                    Stroke::new(
                        styles::components::standard_border::width::EGUI_POINTS,
                        border,
                    ),
                );
                action
            })
            .inner
    }

    fn content(
        &mut self,
        ui: &mut egui::Ui,
        username: Option<&str>,
        agent_identity: &Option<AgentIdentity>,
        page_busy: bool,
        show_login_action: bool,
    ) -> Action {
        let mut action = Action::None;
        let is_logging_out = self.logout_task.is_pending();
        let navigation_busy = page_busy || is_logging_out;
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add_space(styles::top_bar_content_guide_offset());
            let logo_response = ui
                .scope(|ui| {
                    compact_button_spacing(ui);
                    let logo = egui::Button::image(
                        egui::Image::new(egui::include_image!("../assets/modama-tint.svg"))
                            .tint(styles::color::on_surface::VALUE.resolve(ui.visuals()))
                            .alt_text("go to Overview")
                            .max_height(styles::components::top_bar_logo::height::EGUI_POINTS),
                    )
                    .frame(false)
                    .corner_radius(styles::radius(styles::rounded::lg::EGUI_POINTS));
                    ui.add_enabled(!navigation_busy, DesignButton::new(logo))
                })
                .inner
                .on_hover_text("Go to Overview");
            if logo_response.clicked() {
                action = Action::GoToOverview;
            }

            ui.add_space(styles::components::top_bar_brand_gap::size::EGUI_POINTS);
            divider(
                ui,
                styles::components::top_bar_brand_divider::height::EGUI_POINTS,
            );
            ui.add_space(styles::components::top_bar_brand_gap::size::EGUI_POINTS);
            ui.label(
                RichText::new(
                    agent_identity
                        .as_ref()
                        .map(|i| i.format())
                        .unwrap_or_else(|| "Ingest Agent".to_owned()),
                )
                .font(styles::typography::supporting_text::font_id())
                .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(styles::top_bar_card_edge_offset());
                if let Some(username) = username {
                    let response = if is_logging_out {
                        loading_button(
                            ui,
                            "Logging out…",
                            egui::Vec2::ZERO,
                            ButtonVariant::Secondary,
                        )
                    } else {
                        ui.add_enabled(!navigation_busy, secondary_button("Log out", ui))
                    };
                    if response.clicked() {
                        self.logout_task.spawn(
                            || crate::simulated_work::sleep(Duration::from_millis(500)),
                            ui.ctx().clone(),
                        );
                    }

                    ui.add_space(styles::components::top_bar_control_gap::size::EGUI_POINTS);
                    divider(
                        ui,
                        styles::components::top_bar_control_divider::height::EGUI_POINTS,
                    );
                    ui.add_space(styles::components::top_bar_control_gap::size::EGUI_POINTS);
                    ui.label(
                        RichText::new(username)
                            .font(styles::typography::navigation::font_id())
                            .color(
                                styles::color::on_surface_variant::VALUE.resolve(ui.visuals()),
                            ),
                    );
                    avatar(ui, username, styles::components::avatar::size::EGUI_POINTS);
                    ui.add_space(styles::components::top_bar_control_gap::size::EGUI_POINTS);
                    divider(
                        ui,
                        styles::components::top_bar_control_divider::height::EGUI_POINTS,
                    );
                    ui.add_space(styles::components::top_bar_control_gap::size::EGUI_POINTS);
                } else if show_login_action {
                    let response = ui.add_enabled(!navigation_busy, primary_button("Log in", ui));
                    if response.clicked() {
                        action = Action::GoToLogin;
                    }
                    ui.add_space(styles::components::top_bar_control_gap::size::EGUI_POINTS);
                }

                theme_button(ui);
            });
        });
        action
    }

    pub(crate) fn poll(&mut self) -> Action {
        if self.logout_task.poll().take_ready().is_some() {
            Action::LogoutSuccess
        } else {
            Action::None
        }
    }
}

fn divider(ui: &mut egui::Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(
            styles::components::standard_border::width::EGUI_POINTS,
            height,
        ),
        Sense::hover(),
    );
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(
            styles::components::standard_border::width::EGUI_POINTS,
            styles::color::outline::VALUE.resolve(ui.visuals()),
        ),
    );
}

fn theme_button(ui: &mut egui::Ui) {
    let dark = ui.visuals().dark_mode;
    let (icon, tooltip) = if dark {
        (egui_phosphor::regular::SUN, "Switch to light theme")
    } else {
        (egui_phosphor::regular::MOON, "Switch to dark theme")
    };
    let response = ui
        .scope(|ui| {
            compact_button_spacing(ui);
            ui.add(DesignButton::new(
                egui::Button::new(
                    RichText::new(icon)
                        .font(FontId::new(
                            styles::components::top_bar_icon::size::EGUI_POINTS,
                            FontFamily::Proportional,
                        ))
                        .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
                )
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::NONE)
                .corner_radius(styles::radius(styles::rounded::lg::EGUI_POINTS))
                .min_size(egui::Vec2::splat(
                    styles::components::top_bar_control::size::EGUI_POINTS,
                )),
            ))
        })
        .inner
        .on_hover_text(tooltip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tooltip));
    if response.clicked() {
        ui.ctx().set_theme(if dark {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        });
    }
}

fn compact_button_spacing(ui: &mut egui::Ui) {
    ui.spacing_mut().button_padding = egui::Vec2::ZERO;
    ui.spacing_mut().interact_size =
        egui::Vec2::splat(styles::components::top_bar_control::size::EGUI_POINTS);
}
