use eframe::egui::{self, Align2, Id, Layout, RichText};

use crate::{
    actions::RetryAction,
    components::buttons::{primary_button, secondary_button},
    styles,
};

const ERROR_TOAST_ID: &str = "agent-error-toast";

#[derive(Debug, PartialEq, Eq)]
pub enum ToastAction {
    Retry(RetryAction),
    Dismiss,
}

#[derive(Debug, Default)]
pub struct ToastState {
    error: Option<ErrorToast>,
}

#[derive(Debug)]
struct ErrorToast {
    message: String,
    retry: RetryAction,
}

impl ToastState {
    pub fn show_error(&mut self, message: String, retry: RetryAction) {
        self.error = Some(ErrorToast { message, retry });
    }

    pub fn is_visible(&self) -> bool {
        self.error.is_some()
    }

    pub fn clear(&mut self) {
        self.error = None;
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<ToastAction> {
        let error = self.error.as_ref()?;

        let action = egui::Area::new(Id::new(ERROR_TOAST_ID))
            .order(egui::Order::Foreground)
            .default_width(styles::components::error_toast::width::EGUI_POINTS)
            .fixed_pos(
                ctx.viewport_rect().right_bottom()
                    + egui::vec2(
                        -styles::outer_guide_inset(),
                        -styles::bottom_overlay_inset(),
                    ),
            )
            .pivot(Align2::RIGHT_BOTTOM)
            .show(ctx, |ui| {
                styles::toast_frame(ui.visuals())
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Error")
                                .font(styles::typography::label::font_id())
                                .color(styles::color::error::VALUE.resolve(ui.visuals())),
                        );
                        ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
                        ui.label(
                            RichText::new(&error.message)
                                .font(styles::typography::body::font_id())
                                .color(styles::color::on_surface::VALUE.resolve(ui.visuals())),
                        );
                        ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            let retry_clicked = ui.add(primary_button("Retry", ui)).clicked();
                            let dismiss_clicked = ui.add(secondary_button("Dismiss", ui)).clicked();
                            if dismiss_clicked {
                                Some(ToastAction::Dismiss)
                            } else if retry_clicked {
                                Some(ToastAction::Retry(error.retry.clone()))
                            } else {
                                None
                            }
                        })
                        .inner
                    })
                    .inner
            })
            .inner;

        if action.is_some() {
            self.clear();
        }
        action
    }
}
