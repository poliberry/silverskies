//! The calm Silver Skies look: soft rounded cards, proportional type and an
//! accent that follows the sky (the shared condition-tinted palette).

use std::sync::Arc;

use egui::{
    epaint::Shadow, style::ScrollStyle, Color32, Context, CornerRadius, FontData, FontDefinitions,
    FontFamily, FontId, Margin, Stroke, TextStyle, Theme, Vec2, Visuals,
};
use ss_core::theme::{brightness, Palette};

pub const RADIUS: u8 = 14;
pub const RADIUS_SMALL: u8 = 9;

pub fn c32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

pub fn c32a(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

pub fn alpha(c: [u8; 3], a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
}

pub fn is_dark(p: &Palette) -> bool {
    brightness(p.bg) < 128.0
}

/// Card fill: the surface, nudged a little towards the accent.
pub fn card(p: &Palette) -> Color32 {
    let mix = |a: u8, b: u8| ((a as f32) * 0.94 + (b as f32) * 0.06).round() as u8;
    Color32::from_rgb(
        mix(p.surface[0], p.a0[0]),
        mix(p.surface[1], p.a0[1]),
        mix(p.surface[2], p.a0[2]),
    )
}

/// A rounded card frame.
pub fn card_frame(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(card(p))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(16))
}

pub fn visuals_for(p: &Palette) -> Visuals {
    let dark = is_dark(p);
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let a0 = c32(p.a0);
    let soft = CornerRadius::same(RADIUS_SMALL);

    v.override_text_color = Some(c32(p.text));
    v.panel_fill = c32(p.bg);
    v.window_fill = c32(p.surface);
    v.extreme_bg_color = c32(p.surface3);
    v.faint_bg_color = c32(p.surface);
    v.code_bg_color = c32(p.surface3);
    v.selection.bg_fill = alpha(p.a0, 90);
    v.selection.stroke = Stroke::new(1.0, a0);
    v.hyperlink_color = a0;
    v.warn_fg_color = a0;
    v.error_fg_color = c32(p.danger);
    v.window_stroke = Stroke::NONE;
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS_SMALL);
    let shadow_a = if dark { 90 } else { 28 };
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(shadow_a),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(shadow_a),
    };

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = c32(p.surface);
    w.noninteractive.weak_bg_fill = c32(p.surface);
    w.noninteractive.bg_stroke = Stroke::new(1.0, c32a(p.border));
    w.noninteractive.fg_stroke = Stroke::new(1.0, c32a(p.text2));

    w.inactive.bg_fill = c32(p.surface3);
    w.inactive.weak_bg_fill = c32(p.surface3);
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, c32(p.text));

    w.hovered.bg_fill = alpha(p.a0, 60);
    w.hovered.weak_bg_fill = alpha(p.a0, 46);
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.0, c32(p.text));

    w.active.bg_fill = alpha(p.a0, 110);
    w.active.weak_bg_fill = alpha(p.a0, 90);
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.0, c32(p.text));

    w.open.bg_fill = c32(p.surface3);
    w.open.weak_bg_fill = c32(p.surface3);
    w.open.bg_stroke = Stroke::NONE;
    w.open.fg_stroke = Stroke::new(1.0, c32(p.text));

    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = soft;
        s.expansion = 0.0;
    }
    v
}

pub fn apply_palette(ctx: &Context, p: &Palette) {
    let v = visuals_for(p);
    ctx.set_visuals_of(Theme::Dark, v.clone());
    ctx.set_visuals_of(Theme::Light, v);
}

pub fn install_fonts(ctx: &Context) {
    ctx.set_fonts(font_definitions());
    ctx.all_styles_mut(|s| {
        let prop = |size: f32| FontId::new(size, FontFamily::Proportional);
        s.text_styles.insert(TextStyle::Body, prop(14.5));
        s.text_styles.insert(TextStyle::Button, prop(14.0));
        s.text_styles.insert(TextStyle::Heading, prop(20.0));
        s.text_styles.insert(TextStyle::Small, prop(12.0));
        s.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        );
        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.window_margin = Margin::same(18);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.scroll = ScrollStyle {
            bar_width: 8.0,
            ..ScrollStyle::floating()
        };
    });
}

/// DejaVu Sans for UI text; DejaVu Sans Mono for the few numeric bits.
pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "DejaVuSans".to_owned(),
        Arc::new(FontData::from_static(ss_assets::fonts::DEJAVU_SANS)),
    );
    fonts.font_data.insert(
        "DejaVuSansMono".to_owned(),
        Arc::new(FontData::from_static(ss_assets::fonts::DEJAVU_SANS_MONO)),
    );
    if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
        f.insert(0, "DejaVuSans".to_owned());
    }
    if let Some(f) = fonts.families.get_mut(&FontFamily::Monospace) {
        f.insert(0, "DejaVuSansMono".to_owned());
        f.push("DejaVuSans".to_owned());
    }
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss_core::theme::{dark_base, light_base};

    #[test]
    fn visuals_follow_palette_brightness() {
        assert!(visuals_for(&dark_base()).dark_mode);
        assert!(!visuals_for(&light_base()).dark_mode);
    }

    #[test]
    fn fonts_cover_ui_glyphs() {
        let mut fonts = egui::epaint::text::Fonts::new(
            egui::epaint::text::TextOptions::default(),
            font_definitions(),
        );
        let id = FontId::new(14.0, FontFamily::Proportional);
        let missing: Vec<char> = "°↑↓←→·…✕☂✦⚠•▶❚—↻"
            .chars()
            .filter(|c| !fonts.has_glyph(&id, *c))
            .collect();
        assert!(missing.is_empty(), "missing glyphs: {missing:?}");
    }
}
