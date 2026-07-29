pub mod actions;
pub mod app;
pub mod async_state;
pub mod buttons;
pub mod pages;
pub mod state;
pub mod styles;

#[cfg(test)]
mod color_parser;

#[cfg(test)]
mod style_tests {
    use super::styles;
    use eframe::egui;

    #[test]
    fn colors_follow_egui_theme() {
        assert_eq!(
            styles::color::bg.resolve(&egui::Visuals::light()),
            "#f1f5fd"
        );
        assert_eq!(styles::color::bg.resolve(&egui::Visuals::dark()), "#0b1120");
        assert_eq!(
            styles::color::bg.resolve_egui(&egui::Visuals::light()),
            styles::color::bg.light_egui
        );
        assert_eq!(
            styles::color::bg.resolve_egui(&egui::Visuals::dark()),
            styles::color::bg.dark_egui
        );
        assert_eq!(styles::color::ring.light_egui.a(), 89);
        assert_eq!(styles::color::ring.dark_egui.a(), 140);
    }

    #[test]
    fn front_matter_values_are_preserved() {
        assert_eq!(styles::spacing::unit.value, 4.0);
        assert_eq!(styles::spacing::unit.unit, "px");
        assert_eq!(styles::spacing::scale.len(), 17);
        assert_eq!(styles::shapes::radius_2xl.value, 1.0);
        assert_eq!(styles::shapes::radius_2xl.unit, "rem");
        assert_eq!(styles::typography::sans.weights, &[400, 500, 600, 700]);
        assert_eq!(
            styles::typography::heading.native_font_family,
            Some("Plus Jakarta Sans Bold")
        );
        assert_eq!(styles::typography::heading.font_weight, Some(700));
        assert_eq!(styles::typography::button.font_weight, Some(600));
        assert_eq!(
            styles::typography::mono_medium.native_font_family,
            Some("JetBrains Mono Medium")
        );
    }

    #[test]
    fn theme_maps_design_tokens_to_egui() {
        let ctx = egui::Context::default();
        styles::theme::install(&ctx);

        assert_eq!(
            ctx.global_style().visuals.panel_fill,
            styles::color::bg.light_egui
        );
        assert_eq!(
            ctx.global_style().visuals.window_fill,
            styles::color::surface.light_egui
        );
        assert_eq!(
            ctx.global_style().visuals.widgets.inactive.weak_bg_fill,
            styles::color::primary.light_egui
        );
        assert_eq!(
            ctx.global_style().visuals.strong_text_color(),
            styles::color::fg.light_egui
        );
        assert_eq!(
            ctx.style_of(egui::Theme::Light).spacing.button_padding,
            egui::vec2(
                styles::token_to_px(styles::spacing::button_padding_x),
                styles::token_to_px(styles::spacing::button_padding_y),
            )
        );

        ctx.set_theme(egui::Theme::Dark);
        assert_eq!(
            ctx.global_style().visuals.panel_fill,
            styles::color::bg.dark_egui
        );
        assert_eq!(
            ctx.global_style().visuals.window_fill,
            styles::color::surface.dark_egui
        );
        assert_eq!(
            ctx.style_of(egui::Theme::Dark).spacing.button_padding,
            egui::vec2(
                styles::token_to_px(styles::spacing::button_padding_x),
                styles::token_to_px(styles::spacing::button_padding_y),
            )
        );
    }

    #[test]
    fn typography_uses_the_bundled_design_fonts() {
        let ctx = egui::Context::default();
        styles::theme::install(&ctx);

        let style = ctx.global_style();
        assert_eq!(
            style.text_styles[&egui::TextStyle::Body],
            styles::font::from_token(styles::typography::body)
        );
        assert_eq!(
            style.text_styles[&egui::TextStyle::Button],
            styles::font::from_token(styles::typography::button)
        );
        assert_eq!(
            style.text_styles[&egui::TextStyle::Heading],
            styles::font::from_token(styles::typography::heading)
        );
        assert_eq!(
            style.text_styles[&egui::TextStyle::Monospace],
            styles::font::from_token(styles::typography::mono)
        );

        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        for font_id in [
            styles::font::from_token(styles::typography::body),
            styles::font::from_token(styles::typography::navigation),
            styles::font::from_token(styles::typography::label),
            styles::font::from_token(styles::typography::button),
            styles::font::from_token(styles::typography::heading),
            styles::font::from_token(styles::typography::login_title),
            styles::font::from_token(styles::typography::login_subtitle),
            styles::font::from_token(styles::typography::mono),
            styles::font::from_token(styles::typography::mono_medium),
        ] {
            ctx.fonts_mut(|fonts| assert!(fonts.fonts.font(&font_id.family).has_glyphs("A")));
        }
    }

    #[test]
    fn component_radii_match_design_spec() {
        assert_eq!(styles::token_to_px(styles::shapes::radius_lg), 8.0);
        assert_eq!(styles::token_to_px(styles::shapes::radius_2xl), 16.0);
        assert_eq!(
            styles::radius(styles::shapes::radius_lg),
            egui::CornerRadius::same(8)
        );
        assert_eq!(
            styles::radius(styles::shapes::radius_2xl),
            egui::CornerRadius::same(16)
        );
    }

    #[test]
    fn component_spacing_uses_design_scale() {
        assert_eq!(styles::token_to_px(styles::spacing::input_padding_x), 12.0);
        assert_eq!(styles::token_to_px(styles::spacing::input_padding_y), 10.0);
        assert_eq!(styles::token_to_px(styles::spacing::button_padding_x), 20.0);
        assert_eq!(styles::token_to_px(styles::spacing::button_padding_y), 10.0);
        assert_eq!(styles::token_to_px(styles::spacing::card_padding_x), 40.0);
        assert_eq!(styles::token_to_px(styles::spacing::card_padding_y), 44.0);
        assert_eq!(styles::token_to_px(styles::spacing::field_label_gap), 6.0);
        assert_eq!(styles::token_to_px(styles::spacing::form_field_gap), 16.0);
        assert_eq!(styles::token_to_px(styles::spacing::button_margin_top), 8.0);
    }
}
