pub mod actions;
pub mod app;
pub mod async_state;
pub mod components;
pub mod pages;
mod simulated_work;
pub mod state;
pub mod styles;
mod top_bar;

#[cfg(test)]
mod color_parser;

#[cfg(test)]
mod style_tests {
    use super::styles;
    use eframe::egui;

    #[test]
    fn generated_tokens_are_typed_and_available() {
        assert_eq!(
            styles::color::background::VALUE.resolve_str(&egui::Visuals::light()),
            "#f1f5fd"
        );
        assert_eq!(
            styles::color::background::VALUE.resolve_str(&egui::Visuals::dark()),
            "#0b1120"
        );
        assert_eq!(styles::typography::button::FONT_WEIGHT, 600);
        assert_eq!(styles::spacing::button_padding_x::EGUI_POINTS, 20.0);
        assert_eq!(styles::rounded::_2xl::EGUI_POINTS, 16.0);
        assert_eq!(styles::components::top_bar::height::EGUI_POINTS, 56.0);
        assert_eq!(styles::components::app_shell::padding::EGUI_POINTS, 27.0);
    }

    #[test]
    fn theme_maps_generated_tokens_to_egui() {
        let ctx = egui::Context::default();
        styles::theme::install(&ctx);

        assert_eq!(
            ctx.global_style().visuals.panel_fill,
            styles::color::background::VALUE.light_egui
        );
        assert_eq!(
            ctx.style_of(egui::Theme::Light).spacing.button_padding,
            egui::vec2(
                styles::spacing::button_padding_x::EGUI_POINTS,
                styles::spacing::button_padding_y::EGUI_POINTS,
            )
        );
        assert_eq!(
            ctx.style_of(egui::Theme::Light).visuals.interact_cursor,
            Some(egui::CursorIcon::PointingHand)
        );
        assert_eq!(
            ctx.style_of(egui::Theme::Dark).visuals.interact_cursor,
            Some(egui::CursorIcon::PointingHand)
        );
    }

    #[test]
    fn alpha_color_tokens_preserve_native_treatments() {
        assert_eq!(
            styles::color::error_container::VALUE.light_egui,
            styles::color::error::VALUE.light_egui.gamma_multiply(0.05)
        );
        assert_eq!(
            styles::color::error_container::VALUE.dark_egui,
            styles::color::error::VALUE.dark_egui.gamma_multiply(0.05)
        );
        assert_eq!(
            styles::color::error_variant::VALUE.light_egui,
            styles::color::error::VALUE.light_egui.gamma_multiply(0.30)
        );
        assert_eq!(
            styles::color::error_variant::VALUE.dark_egui,
            styles::color::error::VALUE.dark_egui.gamma_multiply(0.30)
        );
        assert_eq!(
            styles::color::shadow::VALUE.light_egui,
            egui::Color32::from_black_alpha(20)
        );
    }

    #[test]
    fn default_card_frame_uses_standard_card_padding_tokens() {
        let frame = styles::card_frame(&egui::Visuals::light());

        assert_eq!(
            frame.inner_margin,
            egui::Margin::symmetric(
                styles::spacing::card_padding_x::EGUI_POINTS as i8,
                styles::spacing::card_padding_y::EGUI_POINTS as i8,
            )
        );
    }

    #[test]
    fn content_card_frame_uses_content_card_tokens() {
        let frame = styles::content_card_frame(&egui::Visuals::light());

        assert_eq!(
            frame.inner_margin,
            egui::Margin::symmetric(
                styles::components::content_card::padding_x::EGUI_POINTS as i8,
                styles::components::content_card::padding_y::EGUI_POINTS as i8,
            )
        );
    }

    #[test]
    fn toast_frame_uses_error_tokens_and_error_toast_spacing() {
        let frame = styles::toast_frame(&egui::Visuals::light());

        assert_eq!(frame.fill, styles::color::surface::VALUE.light_egui);
        assert_eq!(
            frame.stroke.color,
            styles::color::error_variant::VALUE.light_egui
        );
        assert_eq!(
            frame.inner_margin,
            egui::Margin::symmetric(
                styles::components::error_toast::padding_x::EGUI_POINTS as i8,
                styles::components::error_toast::padding_y::EGUI_POINTS as i8,
            )
        );
        assert_eq!(frame.corner_radius, styles::rounded::xl::EGUI_POINTS.into());
        assert_eq!(frame.shadow, styles::card_shadow(&egui::Visuals::light()));
    }

    #[test]
    fn content_guide_uses_content_card_padding() {
        assert_eq!(
            styles::content_card_inset(),
            styles::components::standard_border::width::EGUI_POINTS
                + styles::components::content_card::padding_x::EGUI_POINTS
        );
    }

    #[test]
    fn content_card_top_gap_accounts_for_normal_item_spacing() {
        assert_eq!(
            styles::spacing::app_item_gap_y::EGUI_POINTS
                + styles::content_card_top_margin_adjustment(),
            styles::spacing::form_field_gap::EGUI_POINTS
        );
    }

    #[test]
    fn shared_content_guide_is_derived_from_layout_tokens() {
        assert_eq!(
            styles::components::top_bar::padding::EGUI_POINTS
                + styles::top_bar_content_guide_offset(),
            styles::content_card_outer_inset_from_window() + styles::content_card_inset()
        );
        assert_eq!(
            styles::components::top_bar::padding::EGUI_POINTS + styles::top_bar_card_edge_offset(),
            styles::content_card_outer_inset_from_window()
        );
    }

    #[test]
    fn typography_uses_the_bundled_design_fonts() {
        let ctx = egui::Context::default();
        styles::theme::install(&ctx);
        assert_eq!(
            ctx.global_style().text_styles[&egui::TextStyle::Body],
            styles::typography::body::font_id()
        );

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            for font_id in [
                styles::typography::body::font_id(),
                styles::typography::label::font_id(),
                styles::typography::button::font_id(),
                styles::typography::heading::font_id(),
                styles::typography::section_heading::font_id(),
                styles::typography::supporting_text::font_id(),
                styles::typography::mono::font_id(),
                styles::typography::mono_medium::font_id(),
            ] {
                ui.label(egui::RichText::new("A").font(font_id));
            }
        });

        ctx.fonts_mut(|fonts| {
            for font_id in [
                styles::typography::body::font_id(),
                styles::typography::label::font_id(),
                styles::typography::button::font_id(),
                styles::typography::heading::font_id(),
                styles::typography::section_heading::font_id(),
                styles::typography::supporting_text::font_id(),
                styles::typography::mono::font_id(),
                styles::typography::mono_medium::font_id(),
            ] {
                let mut font = fonts.fonts.font(&font_id.family);
                assert!(font.has_glyphs("A"));
            }
            let mut proportional = fonts.fonts.font(&egui::FontFamily::Proportional);
            assert!(proportional.has_glyphs(egui_phosphor::regular::SUN));
            assert!(proportional.has_glyphs(egui_phosphor::regular::MOON));
        });
    }
}
