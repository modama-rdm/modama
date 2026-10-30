use eframe::egui::{self, Align2, Id, Layout, RichText};

use crate::{
    components::buttons::{primary_button, secondary_button},
    styles,
};

const AUTOSTART_OFFER_ID: &str = "autostart-offer";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutostartOfferAction {
    Enable,
    Dismiss,
}

/// Non-modal offer to start the session helper at logon (Windows only; see `autostart`)
#[derive(Debug, Default)]
pub struct AutostartOffer {
    /// Why enabling failed, if it did
    pub error: Option<String>,
}

impl AutostartOffer {
    pub fn show(&self, ctx: &egui::Context) -> Option<AutostartOfferAction> {
        egui::Area::new(Id::new(AUTOSTART_OFFER_ID))
            .order(egui::Order::Foreground)
            .default_width(styles::components::error_toast::width::EGUI_POINTS)
            .fixed_pos(
                ctx.viewport_rect().right_bottom()
                    + egui::vec2(
                        -styles::toast_right_inset_from_window(),
                        -styles::toast_bottom_inset_from_window(),
                    ),
            )
            .pivot(Align2::RIGHT_BOTTOM)
            .show(ctx, |ui| {
                // the toast's geometry, with a neutral border: this is an offer, not an error
                styles::toast_frame(ui.visuals())
                    .stroke(egui::Stroke::new(
                        styles::components::standard_border::width::EGUI_POINTS,
                        styles::color::outline::VALUE.resolve(ui.visuals()),
                    ))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Connect ingest drives automatically?")
                                .font(styles::typography::label::font_id()),
                        );
                        ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
                        ui.label(
                            RichText::new(
                                "Modama can run a small background program when you sign in to \
                                 Windows. It connects your ingest drives and disconnects them \
                                 after a session, even when this window is closed.",
                            )
                            .font(styles::typography::body::font_id())
                            .color(styles::color::on_surface_variant::VALUE.resolve(ui.visuals())),
                        );
                        if let Some(error) = &self.error {
                            ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
                            ui.label(
                                RichText::new(error)
                                    .font(styles::typography::body::font_id())
                                    .color(styles::color::error::VALUE.resolve(ui.visuals())),
                            );
                        }
                        ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            let enable_clicked = ui.add(primary_button("Turn on", ui)).clicked();
                            let dismiss_clicked =
                                ui.add(secondary_button("No thanks", ui)).clicked();
                            if dismiss_clicked {
                                Some(AutostartOfferAction::Dismiss)
                            } else if enable_clicked {
                                Some(AutostartOfferAction::Enable)
                            } else {
                                None
                            }
                        })
                        .inner
                    })
                    .inner
            })
            .inner
    }
}
