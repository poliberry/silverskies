//! The Silver Skies look, taken from the web design: near-black (or soft
//! white) surfaces, square corners and hairline borders, Google Sans for UI
//! text, Google Sans Flex for big numbers, Google Sans Code for the spaced
//! uppercase labels, and an accent that follows the sky.

use std::sync::Arc;

use egui::{
    epaint::Shadow, style::ScrollStyle, Color32, Context, CornerRadius, FontData, FontDefinitions,
    FontFamily, FontId, Margin, RichText, Stroke, TextStyle, Theme, Ui, Vec2, Visuals,
};
use ss_core::theme::{brightness, resolve_palette, with_alpha, Palette};
use ss_core::{Config, ThemePref};

/// Everything is square-cornered in this design.
pub const RADIUS: u8 = 0;
pub const RADIUS_SMALL: u8 = 0;
/// Gap between bento tiles.
pub const GAP: f32 = 14.0;

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

/// `--accent2`: the accent mixed towards white (dark) or black (light).
pub fn accent2(p: &Palette) -> Color32 {
    let t = if is_dark(p) { 0.3 } else { -0.2 };
    let mix = |v: u8| {
        if t >= 0.0 {
            (v as f32 + (255.0 - v as f32) * t).round() as u8
        } else {
            (v as f32 * (1.0 + t)).round() as u8
        }
    };
    Color32::from_rgb(mix(p.a0[0]), mix(p.a0[1]), mix(p.a0[2]))
}

/// `--accent-glow`: a faint accent wash for active/selected fills.
pub fn glow(p: &Palette) -> Color32 {
    alpha(p.a0, if is_dark(p) { 38 } else { 30 })
}

/// The web design's dark tokens.
pub fn web_dark() -> Palette {
    Palette {
        a0: [138, 100, 255],
        bg: [0x0d, 0x0d, 0x0f],
        surface: [0x14, 0x14, 0x18],
        surface3: [0x1e, 0x1e, 0x26],
        border: with_alpha([255, 255, 255], 0.12),
        text: [0xf0, 0xee, 0xff],
        text2: with_alpha([0xf0, 0xee, 0xff], 0.6),
        text3: with_alpha([0xf0, 0xee, 0xff], 0.3),
        danger: [0xff, 0x6b, 0x6b],
        safe: [0x64, 0xff, 0xb4],
    }
}

/// The web design's light tokens.
pub fn web_light() -> Palette {
    Palette {
        a0: [138, 100, 255],
        bg: [0xf2, 0xf3, 0xf7],
        surface: [0xff, 0xff, 0xff],
        surface3: [0xe8, 0xea, 0xf0],
        border: with_alpha([0, 0, 0], 0.12),
        text: [0x0f, 0x0e, 0x1a],
        text2: with_alpha([0x0f, 0x0e, 0x1a], 0.65),
        text3: with_alpha([0x0f, 0x0e, 0x1a], 0.38),
        danger: [0xcc, 0x22, 0x22],
        safe: [0x1a, 0x8a, 0x50],
    }
}

/// The palette the app renders with: the web tokens tinted by the sky for
/// the default theme; builtin and custom themes apply as in the Pro app.
pub fn simple_palette(cfg: &Config, condition_accent: [u8; 3], system_dark: bool) -> Palette {
    if cfg.theme_id != "default" {
        return resolve_palette(cfg, condition_accent, system_dark);
    }
    let light = match cfg.theme {
        ThemePref::Light => true,
        ThemePref::Dark => false,
        ThemePref::System => !system_dark,
    };
    // Reuse the shared logic for darkening bright accents on light themes.
    let a0 = resolve_palette(cfg, condition_accent, system_dark).a0;
    let mut p = if light { web_light() } else { web_dark() };
    p.a0 = a0;
    p
}

/// A tile: surface fill with a hairline border, square corners.
pub fn card_frame(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(c32(p.surface))
        .stroke(Stroke::new(1.0, c32a(p.border)))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::same(20))
}

/// Paint a tile background into `rect` (for hand-laid-out bento cells).
pub fn paint_tile(ui: &Ui, rect: egui::Rect, p: &Palette) {
    ui.painter().rect_filled(rect, 0.0, c32(p.surface));
    ui.painter().rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, c32a(p.border)),
        egui::StrokeKind::Inside,
    );
}

// --- type -------------------------------------------------------------------

pub fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

/// Google Sans Flex, wide and thin: the big temperature.
pub fn temp_font(size: f32) -> FontId {
    FontId::new(size, family("flex-wide-thin"))
}

/// Google Sans Flex, wide and medium: compact temperatures.
pub fn temp_font_medium(size: f32) -> FontId {
    FontId::new(size, family("flex-wide-medium"))
}

/// Google Sans Flex light: big values in tiles, place names.
pub fn light_font(size: f32) -> FontId {
    FontId::new(size, family("flex-light"))
}

pub fn semibold_font(size: f32) -> FontId {
    FontId::new(size, family("sans-semibold"))
}

pub fn mono_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn mono_medium_font(size: f32) -> FontId {
    FontId::new(size, family("code-medium"))
}

/// The spaced uppercase label used for every caption in the design
/// (`LAST REFRESH`, `CURRENT CONDITIONS`, `HUMIDITY`, …).
pub fn caps(text: &str, size: f32, color: Color32) -> RichText {
    RichText::new(text.to_uppercase())
        .font(mono_medium_font(size))
        .extra_letter_spacing(size * 0.2)
        .color(color)
}

/// A section heading: spaced caps label followed by a hairline rule.
pub fn section(ui: &mut Ui, p: &Palette, text: &str) {
    ui.horizontal(|ui| {
        ui.label(caps(text, 12.0, c32a(p.text3)));
        let r = ui.available_rect_before_wrap();
        let y = ui.min_rect().center().y;
        ui.painter().hline(
            (r.left() + 10.0)..=r.right(),
            y,
            Stroke::new(1.0, c32a(p.border)),
        );
    });
}

pub fn visuals_for(p: &Palette) -> Visuals {
    let dark = is_dark(p);
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let a0 = c32(p.a0);
    let square = CornerRadius::ZERO;
    let border = Stroke::new(1.0, c32a(p.border));

    v.override_text_color = Some(c32(p.text));
    v.panel_fill = c32(p.bg);
    v.window_fill = c32(p.surface);
    v.extreme_bg_color = c32(p.surface3);
    v.faint_bg_color = c32(p.surface);
    v.code_bg_color = c32(p.surface3);
    v.selection.bg_fill = alpha(p.a0, 90);
    v.selection.stroke = Stroke::new(1.0, a0);
    v.hyperlink_color = accent2(p);
    v.warn_fg_color = a0;
    v.error_fg_color = c32(p.danger);
    v.window_stroke = border;
    v.window_corner_radius = square;
    v.menu_corner_radius = square;
    let shadow_a = if dark { 110 } else { 26 };
    v.window_shadow = Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 0,
        color: Color32::from_black_alpha(shadow_a),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(shadow_a),
    };

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = c32(p.surface);
    w.noninteractive.weak_bg_fill = c32(p.surface);
    w.noninteractive.bg_stroke = border;
    w.noninteractive.fg_stroke = Stroke::new(1.0, c32a(p.text2));

    // Buttons: outlined squares; hover and press wash with the accent.
    w.inactive.bg_fill = c32(p.surface);
    w.inactive.weak_bg_fill = c32(p.surface);
    w.inactive.bg_stroke = border;
    w.inactive.fg_stroke = Stroke::new(1.0, c32a(p.text2));

    w.hovered.bg_fill = glow(p);
    w.hovered.weak_bg_fill = glow(p);
    w.hovered.bg_stroke = Stroke::new(1.0, alpha(p.a0, 128));
    w.hovered.fg_stroke = Stroke::new(1.0, accent2(p));

    w.active.bg_fill = alpha(p.a0, 70);
    w.active.weak_bg_fill = alpha(p.a0, 70);
    w.active.bg_stroke = Stroke::new(1.0, a0);
    w.active.fg_stroke = Stroke::new(1.0, accent2(p));

    w.open.bg_fill = c32(p.surface3);
    w.open.weak_bg_fill = c32(p.surface3);
    w.open.bg_stroke = border;
    w.open.fg_stroke = Stroke::new(1.0, c32(p.text));

    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = square;
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
        s.spacing.window_margin = Margin::same(22);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.scroll = ScrollStyle {
            bar_width: 6.0,
            ..ScrollStyle::solid()
        };
    });
}

/// Google Sans for UI text, Google Sans Code for mono, the Flex instances
/// as named families; DejaVu last so symbols always have a glyph.
pub fn font_definitions() -> FontDefinitions {
    use ss_assets::fonts as f;
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        ("GoogleSans", f::GOOGLE_SANS),
        ("GoogleSansSemiBold", f::GOOGLE_SANS_SEMIBOLD),
        ("GoogleSansFlexLight", f::GOOGLE_SANS_FLEX_LIGHT),
        ("GoogleSansFlexWideThin", f::GOOGLE_SANS_FLEX_WIDE_THIN),
        ("GoogleSansFlexWideMedium", f::GOOGLE_SANS_FLEX_WIDE_MEDIUM),
        ("GoogleSansCode", f::GOOGLE_SANS_CODE),
        ("GoogleSansCodeMedium", f::GOOGLE_SANS_CODE_MEDIUM),
        ("DejaVuSans", f::DEJAVU_SANS),
        ("DejaVuSansMono", f::DEJAVU_SANS_MONO),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    let chain = |first: &str, fallback: &str| -> Vec<String> {
        vec![first.to_owned(), fallback.to_owned()]
    };
    fonts
        .families
        .insert(FontFamily::Proportional, chain("GoogleSans", "DejaVuSans"));
    fonts.families.insert(
        FontFamily::Monospace,
        chain("GoogleSansCode", "DejaVuSansMono"),
    );
    for (fam, first) in [
        ("sans-semibold", "GoogleSansSemiBold"),
        ("flex-light", "GoogleSansFlexLight"),
        ("flex-wide-thin", "GoogleSansFlexWideThin"),
        ("flex-wide-medium", "GoogleSansFlexWideMedium"),
        ("code-medium", "GoogleSansCodeMedium"),
    ] {
        fonts
            .families
            .insert(family(fam), chain(first, "DejaVuSans"));
    }
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visuals_follow_palette_brightness() {
        assert!(visuals_for(&web_dark()).dark_mode);
        assert!(!visuals_for(&web_light()).dark_mode);
    }

    #[test]
    fn default_theme_uses_web_tokens_with_sky_accent() {
        let mut cfg = Config {
            theme: ThemePref::Dark,
            ..Default::default()
        };
        let p = simple_palette(&cfg, [10, 200, 30], true);
        assert_eq!(p.bg, web_dark().bg);
        assert_eq!(p.a0, [10, 200, 30]);
        cfg.theme = ThemePref::Light;
        assert_eq!(simple_palette(&cfg, [10, 200, 30], true).bg, web_light().bg);
    }

    #[test]
    fn fonts_cover_ui_glyphs() {
        let mut fonts = egui::epaint::text::Fonts::new(
            egui::epaint::text::TextOptions::default(),
            font_definitions(),
        );
        for fam in [
            FontFamily::Proportional,
            FontFamily::Monospace,
            family("flex-wide-thin"),
            family("code-medium"),
        ] {
            let id = FontId::new(14.0, fam.clone());
            let missing: Vec<char> = "°↑↓←→·…✕☂✦⚠•▶❚—↻Aa09%"
                .chars()
                .filter(|c| !fonts.has_glyph(&id, *c))
                .collect();
            assert!(missing.is_empty(), "{fam:?} missing glyphs: {missing:?}");
        }
    }
}
