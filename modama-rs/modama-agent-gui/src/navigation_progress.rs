use std::time::Duration;

use eframe::egui::{self, emath::easing::cubic_in_out, Sense, WidgetInfo, WidgetType};

use crate::styles;

const SHOW_DELAY_SECONDS: f64 = 0.150;
const SWEEP_SECONDS: f64 = 1.1;
const SEGMENT_FRACTION: f32 = 1.0 / 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProgressKind {
    Busy,
    ConnectionDropped,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ProgressStatus {
    Hidden,
    Delayed { remaining_seconds: f64 },
    Visible { phase: f32, kind: ProgressKind },
}

#[derive(Default)]
pub(crate) struct NavigationProgress {
    active: Option<(ProgressKind, f64)>,
}

impl NavigationProgress {
    pub(crate) fn show(&mut self, ctx: &egui::Context, busy: bool, connection_dropped: bool) {
        let now = ctx.input(|input| input.time);
        let (phase, kind) = match self.advance(now, busy, connection_dropped) {
            ProgressStatus::Hidden => return,
            ProgressStatus::Delayed { remaining_seconds } => {
                ctx.request_repaint_after(Duration::from_secs_f64(remaining_seconds));
                return;
            }
            ProgressStatus::Visible { phase, kind } => (phase, kind),
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
                let label = match kind {
                    ProgressKind::Busy => "Loading",
                    ProgressKind::ConnectionDropped => "Connection dropped; retrying",
                };
                response.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::ProgressIndicator, true, label)
                });

                let color = match kind {
                    ProgressKind::Busy => styles::color::primary::VALUE.resolve(ui.visuals()),
                    ProgressKind::ConnectionDropped => {
                        styles::color::error::VALUE.resolve(ui.visuals())
                    }
                };
                ui.painter()
                    .rect_filled(track, 0.0, color.gamma_multiply(0.20));
                ui.painter().with_clip_rect(track).rect_filled(
                    segment_rect(track, phase),
                    0.0,
                    color,
                );
            });
    }

    fn advance(&mut self, now: f64, busy: bool, connection_dropped: bool) -> ProgressStatus {
        let kind = if connection_dropped {
            Some(ProgressKind::ConnectionDropped)
        } else if busy {
            Some(ProgressKind::Busy)
        } else {
            None
        };
        let Some(kind) = kind else {
            self.active = None;
            return ProgressStatus::Hidden;
        };

        let active_since = match self.active {
            Some((active_kind, since)) if active_kind == kind => since,
            _ => {
                self.active = Some((kind, now));
                now
            }
        };
        let elapsed = (now - active_since).max(0.0);
        if kind == ProgressKind::Busy && elapsed < SHOW_DELAY_SECONDS {
            return ProgressStatus::Delayed {
                remaining_seconds: SHOW_DELAY_SECONDS - elapsed,
            };
        }

        let animation_elapsed = if kind == ProgressKind::Busy {
            elapsed - SHOW_DELAY_SECONDS
        } else {
            elapsed
        };
        let phase = (animation_elapsed % SWEEP_SECONDS) / SWEEP_SECONDS;
        ProgressStatus::Visible {
            phase: phase as f32,
            kind,
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
            progress.advance(10.0, true, false),
            ProgressStatus::Delayed {
                remaining_seconds: SHOW_DELAY_SECONDS
            }
        );
        assert!(matches!(
            progress.advance(10.149, true, false),
            ProgressStatus::Delayed { .. }
        ));
        assert!(matches!(
            progress.advance(10.150, true, false),
            ProgressStatus::Visible { .. }
        ));
        assert_eq!(
            progress.advance(10.151, false, false),
            ProgressStatus::Hidden
        );
        assert_eq!(
            progress.advance(20.0, true, false),
            ProgressStatus::Delayed {
                remaining_seconds: SHOW_DELAY_SECONDS
            }
        );
    }

    #[test]
    fn animation_repeats_and_segment_geometry_stays_constant() {
        let mut progress = NavigationProgress::default();
        let _ = progress.advance(5.0, true, false);
        let ProgressStatus::Visible { phase, .. } =
            progress.advance(5.0 + SHOW_DELAY_SECONDS + SWEEP_SECONDS, true, false)
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

    #[test]
    fn connection_error_is_immediate_and_takes_precedence() {
        let mut progress = NavigationProgress::default();

        assert_eq!(
            progress.advance(10.0, true, true),
            ProgressStatus::Visible {
                phase: 0.0,
                kind: ProgressKind::ConnectionDropped,
            }
        );
        assert_eq!(progress.advance(10.1, false, false), ProgressStatus::Hidden);
    }
}
