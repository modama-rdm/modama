use eframe::egui::{self, RichText};

use crate::styles;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowStage {
    Overview,
    Project,
    Session,
}

impl FlowStage {
    const ALL: [Self; 3] = [Self::Overview, Self::Project, Self::Session];

    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Ingest",
            Self::Project => "Project",
            Self::Session => "Session",
        }
    }
}

/// Show the compact project/session flow and return a completed stage selected
/// by the user. The current and future stages are intentionally not links.
///
/// Completed stages are disabled while the current page has work in flight so
/// navigation cannot discard the page that owns the task.
pub(crate) fn flow_navigation(
    ui: &mut egui::Ui,
    current: FlowStage,
    navigation_enabled: bool,
) -> Option<FlowStage> {
    let current_index = FlowStage::ALL
        .into_iter()
        .position(|candidate| candidate == current)
        .unwrap_or_default();

    ui.horizontal(|ui| {
        ui.add_space(styles::guide_inset());
        ui.spacing_mut().item_spacing.x = styles::spacing::flow_navigation_gap::EGUI_POINTS;

        FlowStage::ALL
            .into_iter()
            .enumerate()
            .fold(None, |selected, (index, stage)| {
                if index > 0 {
                    ui.label(
                        RichText::new("›")
                            .font(styles::typography::supporting_text::font_id())
                            .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
                    );
                }

                let text_color = if index < current_index {
                    styles::color::primary::VALUE.resolve(ui.visuals())
                } else if stage == current {
                    styles::color::on_surface::VALUE.resolve(ui.visuals())
                } else {
                    styles::color::on_surface_dim::VALUE.resolve(ui.visuals())
                };
                let mut text = RichText::new(stage.label())
                    .font(styles::typography::supporting_text::font_id())
                    .color(text_color);
                if stage == current {
                    text = text.strong();
                }

                if index < current_index {
                    let response =
                        ui.add_enabled(navigation_enabled, egui::Button::new(text).frame(false));
                    styles::paint_focus_ring(ui, &response);
                    if response.clicked() {
                        Some(stage)
                    } else {
                        selected
                    }
                } else {
                    ui.label(text);
                    selected
                }
            })
    })
    .inner
}
