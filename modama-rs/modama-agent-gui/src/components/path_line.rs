//! A long path on one line that fades out at the right edge and expands when clicked

use eframe::egui::{self, Color32, FontId, Label, RichText, Sense};

use crate::styles;

/// How wide the text fades into the background before the "[…]" marker
const FADE_WIDTH: f32 = 48.0;
const MORE_MARKER: &str = "[…]";

/// `path` in `font`, on a single line. If it doesn't fit, it fades into `background` at the
/// right edge and ends with "[…]"; a click shows the whole path, wrapped, and another click
/// collapses it again. The state lives in egui's memory under `id_salt`.
pub fn path_line(
    ui: &mut egui::Ui,
    id_salt: &str,
    path: &str,
    font: FontId,
    background: Color32,
) -> egui::Response {
    let id = ui.id().with(("path-line", id_salt));
    let expanded: bool = ui.data(|data| data.get_temp(id)).unwrap_or(false);
    let color = styles::color::on_surface::VALUE.resolve(ui.visuals());
    let width = ui.available_width();

    let galley = ui
        .painter()
        .layout_no_wrap(path.to_owned(), font.clone(), color);
    let truncated = galley.size().x > width;

    let response = if expanded {
        // egui breaks long values at a dash or other punctuation before breaking anywhere
        ui.add(
            Label::new(RichText::new(path).font(font.clone()).color(color))
                .wrap()
                .sense(Sense::click()),
        )
    } else {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, galley.size().y), Sense::click());
        let painter = ui.painter_at(rect);
        painter.galley(rect.min, galley, color);
        if truncated {
            let marker = painter.layout_no_wrap(
                MORE_MARKER.to_owned(),
                font,
                styles::color::on_surface_dim::VALUE.resolve(ui.visuals()),
            );
            let marker_left = rect.right() - marker.size().x;
            let fade = egui::Rect::from_x_y_ranges(
                (marker_left - FADE_WIDTH)..=marker_left,
                rect.y_range(),
            );
            painter.add(horizontal_fade(fade, background));
            painter.rect_filled(
                egui::Rect::from_x_y_ranges(marker_left..=rect.right(), rect.y_range()),
                0.0,
                background,
            );
            painter.galley(egui::pos2(marker_left, rect.top()), marker, color);
        }
        response
    };

    let clickable = expanded || truncated;
    let response = if clickable {
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(if expanded {
                "Show on one line"
            } else {
                "Show the whole path"
            })
    } else {
        response
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, path));
    if clickable {
        styles::paint_focus_ring(ui, &response);
    }
    if clickable && response.clicked() {
        ui.data_mut(|data| data.insert_temp(id, !expanded));
    }
    response
}

/// A rectangle going from transparent at its left to `color` at its right
fn horizontal_fade(rect: egui::Rect, color: Color32) -> egui::Shape {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(rect.right_top(), color);
    mesh.colored_vertex(rect.right_bottom(), color);
    mesh.colored_vertex(rect.left_bottom(), Color32::TRANSPARENT);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    egui::Shape::mesh(mesh)
}
