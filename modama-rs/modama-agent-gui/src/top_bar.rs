use std::time::Duration;

use eframe::egui::{self, Align, Color32, FontFamily, FontId, Layout, RichText, Sense, Stroke};

use crate::{
    actions::Action,
    async_state::AsyncState,
    buttons::{primary_button, secondary_button, DesignButton},
    state::AgentIdentity,
    styles,
};

const IDENTITY_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x25, 0x63, 0xeb),
    Color32::from_rgb(0x7c, 0x3a, 0xed),
    Color32::from_rgb(0x05, 0x96, 0x69),
    Color32::from_rgb(0xd9, 0x77, 0x06),
    Color32::from_rgb(0xdc, 0x26, 0x26),
    Color32::from_rgb(0x08, 0x91, 0xb2),
    Color32::from_rgb(0xbe, 0x18, 0x5d),
    Color32::from_rgb(0x65, 0xa3, 0x0d),
];

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
                    .fill(styles::color::surface::VALUE.resolve_egui(parent.visuals()))
                    .inner_margin(egui::Margin::symmetric(
                        styles::components::top_bar::padding::EGUI_POINTS as i8,
                        0,
                    )),
            );

        panel
            .show_inside(parent, |ui| {
                let action =
                    self.content(ui, username, agent_identity, page_busy, show_login_action);
                let border = styles::color::outline::VALUE.resolve_egui(ui.visuals());
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
            let logo_response = ui
                .scope(|ui| {
                    compact_button_spacing(ui);
                    let logo = egui::Button::image(
                        egui::Image::new(egui::include_image!("../assets/modama-tint.svg"))
                            .tint(styles::color::on_surface::VALUE.resolve_egui(ui.visuals()))
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
                .font(styles::typography::login_subtitle::font_id())
                .color(styles::color::on_surface_dim::VALUE.resolve_egui(ui.visuals())),
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(username) = username {
                    let response = ui.add_enabled(
                        !navigation_busy,
                        secondary_button(
                            if is_logging_out {
                                "Logging out…"
                            } else {
                                "Log out"
                            },
                            ui,
                        ),
                    );
                    if response.clicked() {
                        self.logout_task.spawn(
                            || std::thread::sleep(Duration::from_millis(500)),
                            ui.ctx().clone(),
                        );
                    }
                    if is_logging_out {
                        ui.spinner();
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
                                styles::color::on_surface_variant::VALUE.resolve_egui(ui.visuals()),
                            ),
                    );
                    avatar(ui, username);
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
            styles::color::outline::VALUE.resolve_egui(ui.visuals()),
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
                        .color(styles::color::on_surface_dim::VALUE.resolve_egui(ui.visuals())),
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

fn avatar(ui: &mut egui::Ui, username: &str) {
    let avatar_size = styles::components::avatar::size::EGUI_POINTS;
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(avatar_size), Sense::hover());
    let color = IDENTITY_COLORS[username_hash(username) % IDENTITY_COLORS.len()];
    ui.painter()
        .circle_filled(rect.center(), avatar_size / 2.0, color);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials(username),
        styles::typography::button::font_id(),
        Color32::WHITE,
    );
    response.on_hover_text(username);
}

fn username_hash(username: &str) -> usize {
    username
        .chars()
        .fold(0_i32, |hash, character| {
            hash.wrapping_mul(31)
                .wrapping_add(i32::try_from(u32::from(character)).unwrap_or_default())
        })
        .unsigned_abs() as usize
}

fn initials(username: &str) -> String {
    username.chars().take(2).collect::<String>().to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_identity_is_stable() {
        assert_eq!(initials("alex"), "AL");
        assert_eq!(username_hash("alex"), username_hash("alex"));
        assert_ne!(username_hash("alex"), username_hash("sam"));
    }
}
