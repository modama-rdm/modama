use eframe::egui::{self, Align, Layout, RichText, Stroke};

use crate::styles;

/// The footer below every page, with the GUI's version right-aligned on the inner right guide
/// (DESIGN.md › Layout › Guides and › Components › Footer). Release builds on Windows have no
/// console, so Windows users find the version here.
pub(crate) fn show_footer(parent: &mut egui::Ui, version: &str) {
    let panel = egui::Panel::bottom("agent-footer")
        .resizable(false)
        .exact_size(styles::components::footer::height::EGUI_POINTS)
        .frame(
            egui::Frame::new()
                .fill(styles::color::surface::VALUE.resolve(parent.visuals()))
                .inner_margin(egui::Margin::symmetric(
                    styles::inner_guide_inset() as i8,
                    0,
                )),
        );

    let response = panel.show(parent, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(version)
                    .font(styles::typography::mono_supporting::font_id())
                    .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
            )
            .on_hover_text(format!("modama-agent-gui {version}"));
        });
    });

    let rect = response.response.rect;
    let width = styles::components::standard_border::width::EGUI_POINTS;
    parent.painter().hline(
        rect.x_range(),
        rect.top() + width / 2.0,
        Stroke::new(
            width,
            styles::color::outline::VALUE.resolve(parent.visuals()),
        ),
    );
}
