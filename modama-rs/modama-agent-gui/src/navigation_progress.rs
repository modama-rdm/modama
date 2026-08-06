use std::time::Duration;

use eframe::egui::{self, emath::easing::cubic_in_out, Sense, WidgetInfo, WidgetType};

use crate::styles;

const SHOW_DELAY_SECONDS: f64 = 0.150;
const SWEEP_SECONDS: f64 = 1.1;
const SEGMENT_FRACTION: f32 = 1.0 / 3.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum ProgressStatus {
    Hidden,
    Delayed { remaining_seconds: f64 },
    Visible { phase: f32 },
}

#[derive(Default)]
pub(crate) struct NavigationProgress {
    busy_since: Option<f64>,
}

impl NavigationProgress {
    pub(crate) fn show(&mut self, ctx: &egui::Context, busy: bool) {
        let now = ctx.input(|input| input.time);
        let phase = match self.advance(now, busy) {
            ProgressStatus::Hidden => return,
            ProgressStatus::Delayed { remaining_seconds } => {
                ctx.request_repaint_after(Duration::from_secs_f64(remaining_seconds));
                return;
            }
            ProgressStatus::Visible { phase } => phase,
        };

        ctx.request_repaint();
        let viewport = ctx.content_rect();
        egui::Area::new(egui::Id::new("navigation-progress"))
            .order(egui::Order::Tooltip)
            .fixed_pos(viewport.left_top())
            .interactable(false)
            .show(ctx, |ui| {
                let size = egui::vec2(
                    viewport.width(),
                    styles::components::navigation_progress::height::EGUI_POINTS,
                );
                let (track, response) = ui.allocate_exact_size(size, Sense::hover());
                response.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::ProgressIndicator, true, "Loading")
                });

                let primary = styles::color::primary::VALUE.resolve(ui.visuals());
                ui.painter()
                    .rect_filled(track, 0.0, primary.gamma_multiply(0.20));
                ui.painter().with_clip_rect(track).rect_filled(
                    segment_rect(track, phase),
                    0.0,
                    primary,
                );
            });
    }

    fn advance(&mut self, now: f64, busy: bool) -> ProgressStatus {
        if !busy {
            self.busy_since = None;
            return ProgressStatus::Hidden;
        }

        let busy_since = *self.busy_since.get_or_insert(now);
        let elapsed = (now - busy_since).max(0.0);
        if elapsed < SHOW_DELAY_SECONDS {
            return ProgressStatus::Delayed {
                remaining_seconds: SHOW_DELAY_SECONDS - elapsed,
            };
        }

        let phase = ((elapsed - SHOW_DELAY_SECONDS) % SWEEP_SECONDS) / SWEEP_SECONDS;
        ProgressStatus::Visible {
            phase: phase as f32,
        }
    }
}

fn segment_rect(track: egui::Rect, phase: f32) -> egui::Rect {
    let segment_width = track.width() * SEGMENT_FRACTION;
    let eased_phase = cubic_in_out(phase.clamp(0.0, 1.0));
    let left = egui::lerp((track.left() - segment_width)..=track.right(), eased_phase);
    egui::Rect::from_min_size(
        egui::pos2(left, track.top()),
        egui::vec2(segment_width, track.height()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_delayed_and_resets_immediately() {
        let mut progress = NavigationProgress::default();

        assert_eq!(
            progress.advance(10.0, true),
            ProgressStatus::Delayed {
                remaining_seconds: SHOW_DELAY_SECONDS
            }
        );
        assert!(matches!(
            progress.advance(10.149, true),
            ProgressStatus::Delayed { .. }
        ));
        assert!(matches!(
            progress.advance(10.150, true),
            ProgressStatus::Visible { .. }
        ));
        assert_eq!(progress.advance(10.151, false), ProgressStatus::Hidden);
        assert_eq!(
            progress.advance(20.0, true),
            ProgressStatus::Delayed {
                remaining_seconds: SHOW_DELAY_SECONDS
            }
        );
    }

    #[test]
    fn animation_repeats_and_segment_geometry_stays_constant() {
        let mut progress = NavigationProgress::default();
        let _ = progress.advance(5.0, true);
        let ProgressStatus::Visible { phase } =
            progress.advance(5.0 + SHOW_DELAY_SECONDS + SWEEP_SECONDS, true)
        else {
            panic!("the progress bar should be visible");
        };
        assert!(phase.abs() <= f32::EPSILON);

        let track = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 4.0));
        let start = segment_rect(track, 0.0);
        let end = segment_rect(track, 1.0);
        assert_eq!(start.width(), 100.0);
        assert_eq!(end.width(), 100.0);
        assert_eq!(start.right(), track.left());
        assert_eq!(end.left(), track.right());
    }
}
