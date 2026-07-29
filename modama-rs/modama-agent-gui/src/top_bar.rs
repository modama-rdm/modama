use std::time::Duration;

use eframe::egui::{self, Align, Color32, FontFamily, FontId, Layout, RichText, Sense, Stroke};

use crate::{
    actions::Action,
    async_state::AsyncState,
    buttons::{primary_button, secondary_button},
    styles,
};

const HEIGHT: f32 = 56.0;
const HORIZONTAL_PADDING: i8 = 28;
const AVATAR_SIZE: f32 = 32.0;
const ICON_SIZE: f32 = 18.0;
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
        page_busy: bool,
        show_login_action: bool,
    ) -> Action {
        let panel = egui::Panel::top("agent-top-bar").exact_size(HEIGHT).frame(
            egui::Frame::new()
                .fill(styles::color::surface::VALUE.resolve_egui(parent.visuals()))
                .inner_margin(egui::Margin::symmetric(HORIZONTAL_PADDING, 0)),
        );

        panel
            .show_inside(parent, |ui| {
                let action = self.content(ui, username, page_busy, show_login_action);
                let border = styles::color::border::VALUE.resolve_egui(ui.visuals());
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.max_rect().bottom() - 1.0,
                    Stroke::new(1.0, border),
                );
                action
            })
            .inner
    }

    fn content(
        &mut self,
        ui: &mut egui::Ui,
        username: Option<&str>,
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
                            .tint(styles::color::fg::VALUE.resolve_egui(ui.visuals()))
                            .alt_text("modama — go to Overview")
                            .max_height(26.0),
                    )
                    .frame(false)
                    .corner_radius(styles::radius(styles::shapes::radius_lg::EGUI_POINTS));
                    ui.add_enabled(!navigation_busy, logo)
                })
                .inner
                .on_hover_text("Go to Overview");
            styles::paint_focus_ring(ui, &logo_response);
            if logo_response.clicked() {
                action = Action::GoToOverview;
            }

            ui.add_space(6.0);
            divider(ui, 22.0);
            ui.add_space(6.0);
            ui.label(
                RichText::new("Data Management Ingest Agent")
                    .font(styles::typography::login_subtitle::font_id())
                    .color(styles::color::fg_subtle::VALUE.resolve_egui(ui.visuals())),
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
                    styles::paint_focus_ring(ui, &response);
                    if response.clicked() {
                        self.logout_task.spawn(
                            || std::thread::sleep(Duration::from_millis(500)),
                            ui.ctx().clone(),
                        );
                    }
                    if is_logging_out {
                        ui.spinner();
                    }

                    ui.add_space(2.0);
                    divider(ui, 18.0);
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(username)
                            .font(styles::typography::navigation::font_id())
                            .color(styles::color::fg_muted::VALUE.resolve_egui(ui.visuals())),
                    );
                    avatar(ui, username);
                    ui.add_space(2.0);
                    divider(ui, 18.0);
                    ui.add_space(2.0);
                } else if show_login_action {
                    let response = ui.add_enabled(!navigation_busy, primary_button("Log in", ui));
                    styles::paint_focus_ring(ui, &response);
                    if response.clicked() {
                        action = Action::GoToLogin;
                    }
                    ui.add_space(2.0);
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
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, height), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0, styles::color::border::VALUE.resolve_egui(ui.visuals())),
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
            ui.add(
                egui::Button::new(
                    RichText::new(icon)
                        .font(FontId::new(ICON_SIZE, FontFamily::Proportional))
                        .color(styles::color::fg_subtle::VALUE.resolve_egui(ui.visuals())),
                )
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::NONE)
                .corner_radius(styles::radius(styles::shapes::radius_lg::EGUI_POINTS))
                .min_size(egui::vec2(32.0, 32.0)),
            )
        })
        .inner
        .on_hover_text(tooltip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tooltip));
    styles::paint_focus_ring(ui, &response);
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
    ui.spacing_mut().interact_size = egui::vec2(32.0, 32.0);
}

fn avatar(ui: &mut egui::Ui, username: &str) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(AVATAR_SIZE, AVATAR_SIZE), Sense::hover());
    let color = IDENTITY_COLORS[username_hash(username) % IDENTITY_COLORS.len()];
    ui.painter()
        .circle_filled(rect.center(), AVATAR_SIZE / 2.0, color);
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
