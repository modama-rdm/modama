use std::fmt::Display;

use eframe::egui::{self, Response, RichText, Stroke, WidgetInfo};

use crate::{components::forms::field_label, styles};

pub fn dropdown<'a, T, I>(
    ui: &mut egui::Ui,
    options: I,
    selected_option: &mut T,
    label_text: &str,
    id_salt: &str,
) -> Response
where
    T: Display + std::cmp::PartialEq + Clone + 'static,
    I: Iterator<Item = &'a T>,
{
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;

        let label = field_label(ui, label_text);
        ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
        let surface = styles::color::surface::VALUE.resolve(ui.visuals());
        let surface_variant = styles::color::surface_variant::VALUE.resolve(ui.visuals());
        let outline = styles::color::outline::VALUE.resolve(ui.visuals());
        let primary_tint = styles::color::primary_tint::VALUE.resolve(ui.visuals());
        ui.visuals_mut().widgets.inactive.bg_fill = surface;
        ui.visuals_mut().widgets.inactive.weak_bg_fill = surface;
        ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(
            styles::components::standard_border::width::EGUI_POINTS,
            outline,
        );
        ui.spacing_mut().button_padding = egui::vec2(
            styles::spacing::input_padding_x::EGUI_POINTS,
            styles::spacing::input_padding_y::EGUI_POINTS,
        );

        let widgets = &mut ui.visuals_mut().widgets;
        for widget in [&mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
            widget.bg_fill = surface_variant;
            widget.weak_bg_fill = surface_variant;
            widget.bg_stroke = Stroke::new(
                styles::components::standard_border::width::EGUI_POINTS,
                outline,
            );
        }
        let open_radius = styles::radius(styles::rounded::lg::EGUI_POINTS);
        ui.visuals_mut().widgets.open.corner_radius = egui::CornerRadius {
            nw: open_radius.nw,
            ne: open_radius.ne,
            sw: 0,
            se: 0,
        };
        let selected_text =
            RichText::new(format!("{}", selected_option)).font(styles::typography::body::font_id());

        let response = egui::ComboBox::from_id_salt(id_salt)
            .width(ui.available_width())
            .selected_text(selected_text)
            .popup_style(egui::style::StyleModifier::new(
                move |style: &mut egui::Style| {
                    egui::containers::menu::menu_style(style);
                    style.spacing.menu_margin.left = 0;
                    style.spacing.menu_margin.right = 0;
                    style.spacing.item_spacing.y = 0.0;
                    style.spacing.button_padding = egui::vec2(
                        styles::spacing::input_padding_x::EGUI_POINTS,
                        styles::spacing::input_padding_y::EGUI_POINTS,
                    );
                    let popup_radius = style.visuals.menu_corner_radius;
                    style.visuals.menu_corner_radius = egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: popup_radius.sw,
                        se: popup_radius.se,
                    };
                    style.visuals.selection.bg_fill = primary_tint;
                    for widget in [
                        &mut style.visuals.widgets.inactive,
                        &mut style.visuals.widgets.hovered,
                        &mut style.visuals.widgets.active,
                        &mut style.visuals.widgets.open,
                    ] {
                        widget.corner_radius = egui::CornerRadius::ZERO;
                        widget.bg_stroke = Stroke::NONE;
                    }
                    style.visuals.widgets.inactive.bg_fill = surface;
                    style.visuals.widgets.inactive.weak_bg_fill = surface;
                    for widget in [
                        &mut style.visuals.widgets.hovered,
                        &mut style.visuals.widgets.active,
                        &mut style.visuals.widgets.open,
                    ] {
                        widget.bg_fill = surface_variant;
                        widget.weak_bg_fill = surface_variant;
                    }
                },
            ))
            .show_ui(ui, |ui| {
                options.for_each(|o| {
                    option(ui, selected_option, o);
                });
            })
            .response
            .labelled_by(label.id);
        styles::paint_focus_ring(ui, &response);
        response
    })
    .inner
}

fn option<T>(ui: &mut egui::Ui, selected_option: &mut T, rendering_option: &T) -> egui::Response
where
    T: Display + std::cmp::PartialEq + Clone,
{
    let rendering_label = format!("{}", rendering_option);
    let selected = selected_option == rendering_option;
    let mut response = ui.add(
        egui::Button::selectable(selected, &rendering_label)
            .corner_radius(egui::CornerRadius::ZERO)
            .min_size(egui::vec2(ui.available_width(), 0.0)),
    );
    if response.clicked() && !selected {
        *selected_option = rendering_option.clone();
        response.mark_changed();
    }
    response.widget_info(|| {
        WidgetInfo::selected(
            egui::WidgetType::Button,
            ui.is_enabled(),
            selected,
            &rendering_label,
        )
    });
    response
}
