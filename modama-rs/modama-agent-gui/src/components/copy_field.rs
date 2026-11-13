//! A labelled, read-only monospace value with a copy button, like `CopyField` in the web GUI

use eframe::egui::{self, RichText};

use crate::styles;

/// How long the check mark replaces the copy icon after copying
const COPIED_FEEDBACK_SECONDS: f64 = 2.0;

/// A copy field shows this many lines before it scrolls, so that the "Use in Python" dialog
/// fits a window of the default height
const VISIBLE_LINES: f32 = 17.0;

/// Phosphor icons at the size of the text they stand next to; the glyphs are part of the
/// proportional family only
pub fn icon_font(text: &egui::FontId) -> egui::FontId {
    egui::FontId::new(text.size, egui::FontFamily::Proportional)
}

/// Icon-only button that copies `value` and briefly shows a check mark. `what` completes
/// the accessible label and the hover text: "Copy {what}".
pub fn copy_button(ui: &mut egui::Ui, value: &str, what: &str) -> egui::Response {
    let id = ui.id().with(("copy-button", what));
    let now = ui.input(|input| input.time);
    let copied_at: Option<f64> = ui.data(|data| data.get_temp(id));
    let copied = copied_at.is_some_and(|at| now - at < COPIED_FEEDBACK_SECONDS);

    let (icon, color) = if copied {
        (
            egui_phosphor::regular::CHECK_CIRCLE,
            styles::color::tertiary::VALUE,
        )
    } else {
        (
            egui_phosphor::regular::COPY,
            styles::color::on_surface_dim::VALUE,
        )
    };
    let label = format!("Copy {what}");
    let response = ui
        .add(
            egui::Button::new(
                RichText::new(icon)
                    .font(icon_font(&styles::typography::body::font_id()))
                    .color(color.resolve(ui.visuals())),
            )
            .frame(false),
        )
        .on_hover_text(&label);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &label)
    });
    styles::paint_focus_ring(ui, &response);

    if response.clicked() {
        ui.ctx().copy_text(value.to_owned());
        ui.data_mut(|data| data.insert_temp(id, now));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(COPIED_FEEDBACK_SECONDS));
    }
    response
}

/// `label` above a bordered box with `value` in monospace and a copy button at its right
/// edge. Lines don't wrap (code stays code): a value wider than the box scrolls sideways,
/// and one with more lines than [`VISIBLE_LINES`] scrolls vertically.
pub fn copy_field(ui: &mut egui::Ui, label: &str, value: &str) {
    super::detail_label(ui, label);
    ui.add_space(styles::spacing::detail_label_gap::EGUI_POINTS - ui.spacing().item_spacing.y);
    let padding = styles::spacing::input_padding_x::EGUI_POINTS;
    egui::Frame::new()
        .fill(styles::color::surface_variant::VALUE.resolve(ui.visuals()))
        .stroke(egui::Stroke::new(
            styles::components::standard_border::width::EGUI_POINTS,
            styles::color::outline::VALUE.resolve(ui.visuals()),
        ))
        .corner_radius(styles::radius(styles::rounded::lg::EGUI_POINTS))
        .inner_margin(egui::Margin::same(padding as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // the copy button first, anchored at the right padding, as far from the edge as
            // the text is on the left; the text takes the rest
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.scope(|ui| {
                    ui.spacing_mut().button_padding = egui::Vec2::ZERO;
                    copy_button(ui, value, &label.to_lowercase());
                });
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    let font = styles::typography::mono_supporting::font_id();
                    let line_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
                    egui::ScrollArea::both()
                        .id_salt(("copy-field", label))
                        .auto_shrink([false, true])
                        .max_height(VISIBLE_LINES * line_height)
                        // a dialog offers less height than that while egui sizes it
                        .min_scrolled_height(VISIBLE_LINES * line_height)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    // a trailing newline would show as an empty line
                                    RichText::new(value.trim_end_matches('\n'))
                                        .font(font)
                                        .color(
                                            styles::color::on_surface::VALUE.resolve(ui.visuals()),
                                        ),
                                )
                                .extend(),
                            );
                        });
                });
            });
        });
}
