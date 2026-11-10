use crate::top_bar::avatar::avatar;
use eframe::egui::{self, Align, Color32, FontFamily, FontId, Layout, RichText, Sense, Stroke};
use egui_taffy::{
    TuiBuilderLogic,
    taffy::{
        self,
        prelude::{AlignItems, FlexDirection, FlexWrap, JustifyContent, Style, length, percent},
    },
    tui,
};

use crate::{
    actions::{Action, LoginDestination},
    components::avatar,
    components::buttons::{
        ButtonVariant, DesignButton, loading_button, primary_button, secondary_button,
    },
    state::AgentIdentity,
    styles,
};

#[derive(Default)]
pub(crate) struct TopBar {}

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
            .show(parent, |ui| {
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
        let is_logging_out = false;
        let navigation_busy = page_busy || is_logging_out;
        let layout_state = (username.is_some(), show_login_action);
        tui(ui, ui.id().with(("top-bar-content", layout_state)))
            .reserve_available_space()
            .style(content_layout_style(username.is_some()))
            .show(|tui| {
                let brand_action = tui
                    .id("top-bar-brand")
                    .ui(|ui| show_brand(ui, agent_identity.as_ref(), !navigation_busy));

                let controls_action = tui.id(controls_id(username, show_login_action)).ui(|ui| {
                    self.show_controls(
                        ui,
                        username,
                        navigation_busy,
                        is_logging_out,
                        show_login_action,
                    )
                });
                controls_action.or(brand_action).unwrap_or(Action::None)
            })
    }

    fn show_controls(
        &mut self,
        ui: &mut egui::Ui,
        username: Option<&str>,
        navigation_busy: bool,
        is_logging_out: bool,
        show_login_action: bool,
    ) -> Option<Action> {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            theme_button(ui);

            if username.is_some() || show_login_action {
                control_gap(ui);
            }

            if let Some(username) = username {
                self.show_authenticated_controls(ui, username, navigation_busy, is_logging_out)
            } else if show_login_action {
                ui.add_enabled(!navigation_busy, primary_button("Log in", ui))
                    .clicked()
                    .then_some(Action::GoToLogin {
                        destination: LoginDestination::Overview,
                    })
            } else {
                None
            }
        })
        .inner
    }

    fn show_authenticated_controls(
        &mut self,
        ui: &mut egui::Ui,
        username: &str,
        navigation_busy: bool,
        is_logging_out: bool,
    ) -> Option<Action> {
        divider(
            ui,
            styles::components::top_bar_control_divider::height::EGUI_POINTS,
        );
        control_gap(ui);
        avatar(ui, username, styles::components::avatar::size::EGUI_POINTS);
        ui.label(
            RichText::new(username)
                .font(styles::typography::navigation::font_id())
                .color(styles::color::on_surface_variant::VALUE.resolve(ui.visuals())),
        );
        control_gap(ui);
        divider(
            ui,
            styles::components::top_bar_control_divider::height::EGUI_POINTS,
        );
        control_gap(ui);

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
            return Some(Action::LogoutRequested);
        }
        None
    }

    pub(crate) fn poll(&mut self) -> Action {
        Action::None
    }
}

fn content_layout_style(authenticated: bool) -> Style {
    // Taffy snaps the authenticated compound row to whole points. Account for that
    // one-point expansion so its trailing edge stays on the existing content guide.
    let trailing_inset = styles::top_bar_outer_guide_offset()
        - if authenticated {
            styles::components::standard_border::width::EGUI_POINTS
        } else {
            0.0
        };

    Style {
        display: taffy::Display::Flex,
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::NoWrap,
        align_items: Some(AlignItems::Center),
        justify_content: Some(JustifyContent::SpaceBetween),
        size: taffy::Size {
            width: percent(1.0_f32),
            height: percent(1.0_f32),
        },
        padding: taffy::Rect {
            left: length(styles::top_bar_inner_guide_offset()),
            right: length(trailing_inset),
            top: length(0.0_f32),
            bottom: length(0.0_f32),
        },
        ..Default::default()
    }
}

fn controls_id(username: Option<&str>, show_login_action: bool) -> &'static str {
    if username.is_some() {
        "top-bar-controls-authenticated"
    } else if show_login_action {
        "top-bar-controls-login-action"
    } else {
        "top-bar-controls-theme-only"
    }
}

fn show_brand(
    ui: &mut egui::Ui,
    agent_identity: Option<&AgentIdentity>,
    navigation_enabled: bool,
) -> Option<Action> {
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        let logo_response = ui
            .scope(|ui| {
                compact_button_spacing(ui);
                let logo = egui::Button::image(
                    egui::Image::new(egui::include_image!("../assets/modama-tint.svg"))
                        .tint(styles::color::on_surface::VALUE.resolve(ui.visuals()))
                        .alt_text("go to Ingest")
                        .max_height(styles::components::top_bar_logo::height::EGUI_POINTS),
                )
                .frame(false)
                .corner_radius(styles::radius(styles::rounded::lg::EGUI_POINTS));
                ui.add_enabled(navigation_enabled, DesignButton::new(logo))
            })
            .inner
            .on_hover_text("Go to Overview");

        ui.add_space(styles::components::top_bar_brand_gap::size::EGUI_POINTS);
        divider(
            ui,
            styles::components::top_bar_brand_divider::height::EGUI_POINTS,
        );
        ui.add_space(styles::components::top_bar_brand_gap::size::EGUI_POINTS);
        ui.label(
            RichText::new(
                agent_identity
                    .map(AgentIdentity::format)
                    .unwrap_or_else(|| "Ingest Agent".to_owned()),
            )
            .font(styles::typography::supporting_text::font_id())
            .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
        );

        logo_response.clicked().then_some(Action::GoToOverview)
    })
    .inner
}

fn control_gap(ui: &mut egui::Ui) {
    ui.add_space(styles::components::top_bar_control_gap::size::EGUI_POINTS);
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
