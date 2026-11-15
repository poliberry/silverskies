//! Palette -> egui visuals, fonts and spacing for the Pro look: hard-edged
//! black/white blocks with electric blue and violet accents.

use std::sync::Arc;

use egui::{
    epaint::Shadow, style::ScrollStyle, Color32, Context, CornerRadius, FontData, FontDefinitions,
    FontFamily, FontId, Margin, Stroke, TextStyle, Theme, Vec2, Visuals,
};
use ss_core::theme::{brightness, resolve_palette, with_alpha, Palette};
use ss_core::{Config, ThemePref};

/// Electric blue: the Pro accent (`a0` of the default Pro palettes).
pub const PRO_BLUE: [u8; 3] = [0x3d, 0x6b, 0xff];
/// Violet: the Pro secondary — title blocks while dragging, selections,
/// the block shadow under floating cards.
pub const PRO_PURPLE: [u8; 3] = [0x8e, 0x4d, 0xff];

/// Pure black / white base with the blue accent.
pub fn pro_dark() -> Palette {
    Palette {
        a0: PRO_BLUE,
        bg: [0, 0, 0],
        surface: [0x05, 0x05, 0x09],
        surface3: [0x12, 0x14, 0x24],
        border: with_alpha([0x5a, 0x7d, 0xff], 0.38),
        // White for primary text, blue for secondary, violet for tertiary.
        text: [0xff, 0xff, 0xff],
        text2: [0x9d, 0xb6, 0xff, 0xff],
        text3: [0xa8, 0x86, 0xf0, 0xd0],
        danger: [0xff, 0x3b, 0x5c],
        safe: [0x4d, 0xff, 0xa6],
    }
}

pub fn pro_light() -> Palette {
    Palette {
        a0: [0x1f, 0x4f, 0xf0],
        bg: [0xff, 0xff, 0xff],
        surface: [0xff, 0xff, 0xff],
        surface3: [0xec, 0xec, 0xf2],
        border: with_alpha([0, 0, 0], 0.32),
        text: [0, 0, 0],
        text2: with_alpha([0, 0, 0], 0.72),
        text3: with_alpha([0, 0, 0], 0.45),
        danger: [0xd4, 0x16, 0x3c],
        safe: [0x0a, 0x8f, 0x55],
    }
}

/// The palette Pro renders with. The `"default"` theme is the fixed Pro
/// black/white/blue look (it does not tint with the sky like Silver Skies);
/// builtin and custom themes still apply as before.
pub fn pro_palette(cfg: &Config, condition_accent: [u8; 3], system_dark: bool) -> Palette {
    if cfg.theme_id != "default" {
        return resolve_palette(cfg, condition_accent, system_dark);
    }
    let light = match cfg.theme {
        ThemePref::Light => true,
        ThemePref::Dark => false,
        ThemePref::System => !system_dark,
    };
    if light {
        pro_light()
    } else {
        pro_dark()
    }
}

/// True when the palette's background is dark.
pub fn is_dark(p: &Palette) -> bool {
    brightness(p.bg) < 128.0
}

/// Foreground (black or white) that reads on a solid `fill`.
pub fn on(fill: [u8; 3]) -> Color32 {
    if brightness(fill) > 150.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

pub fn c32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

pub fn c32a(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

pub fn visuals_for(p: &Palette) -> Visuals {
    let dark = brightness(p.bg) < 128.0;
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let a0 = c32(p.a0);
    let border = Stroke::new(1.0, c32a(p.border));
    let sq = CornerRadius::ZERO;

    v.override_text_color = Some(c32(p.text));
    v.panel_fill = c32(p.bg);
    v.window_fill = c32(p.bg);
    v.extreme_bg_color = c32(p.surface);
    v.faint_bg_color = c32(p.surface);
    v.code_bg_color = c32(p.surface3);
    v.selection.bg_fill =
        Color32::from_rgba_unmultiplied(PRO_PURPLE[0], PRO_PURPLE[1], PRO_PURPLE[2], 110);
    v.selection.stroke = Stroke::new(1.5, c32(PRO_PURPLE));
    v.hyperlink_color = a0;
    v.warn_fg_color = c32(p.a0);
    v.error_fg_color = c32(p.danger);
    v.window_stroke = border;
    v.window_corner_radius = sq;
    v.menu_corner_radius = sq;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.striped = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = c32(p.bg);
    w.noninteractive.weak_bg_fill = c32(p.bg);
    w.noninteractive.bg_stroke = border;
    w.noninteractive.fg_stroke = Stroke::new(1.0, c32a(p.text2));

    // Blocky controls: every button is a hard-edged box with a visible
    // outline; hover inverts to the accent block, press goes violet.
    w.inactive.bg_fill = c32(p.surface3);
    w.inactive.weak_bg_fill = c32(p.surface3);
    w.inactive.bg_stroke = Stroke::new(1.0, c32a(p.border));
    w.inactive.fg_stroke = Stroke::new(1.0, c32(p.text));

    for s in [&mut w.hovered, &mut w.active, &mut w.open] {
        s.bg_fill = a0;
        s.weak_bg_fill = a0;
        s.fg_stroke = Stroke::new(1.5, on(p.a0));
        s.bg_stroke = Stroke::new(1.5, c32(p.text));
    }
    w.active.bg_fill = c32(PRO_PURPLE);
    w.active.weak_bg_fill = c32(PRO_PURPLE);
    w.active.fg_stroke = Stroke::new(1.5, on(PRO_PURPLE));
    w.open.bg_fill = c32(p.surface3);
    w.open.weak_bg_fill = c32(p.surface3);
    w.open.fg_stroke = Stroke::new(1.0, c32(p.text));
    w.open.bg_stroke = Stroke::new(1.5, a0);

    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = sq;
        s.expansion = 0.0;
    }
    v
}

pub fn apply_palette(ctx: &Context, p: &Palette) {
    let v = visuals_for(p);
    // Both themes get the same visuals so the system theme can never override us.
    ctx.set_visuals_of(Theme::Dark, v.clone());
    ctx.set_visuals_of(Theme::Light, v);
}

/// Monospace everywhere, tight spacing. Call once at startup.
///
/// DejaVu Sans Mono is the primary face (full box-drawing, arrows and
/// weather-symbol coverage); DejaVu Sans is the last-resort fallback for any
/// glyph the mono face lacks. Both are Bitstream-Vera licensed (see
/// `crates/ss-assets/assets/fonts/LICENSE`).
pub fn install_fonts(ctx: &Context) {
    ctx.set_fonts(font_definitions());
    apply_text_styles(ctx);
}

/// egui default fonts plus the bundled DejaVu faces.
pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "DejaVuSansMono".to_owned(),
        Arc::new(FontData::from_static(ss_assets::fonts::DEJAVU_SANS_MONO)),
    );
    fonts.font_data.insert(
        "DejaVuSans".to_owned(),
        Arc::new(FontData::from_static(ss_assets::fonts::DEJAVU_SANS)),
    );
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.insert(0, "DejaVuSansMono".to_owned());
        mono.push("DejaVuSans".to_owned());
    }
    // Let the proportional family fall back to the monospace font first, so any
    // widget that explicitly asks for Proportional still looks terminal-like.
    if let Some(mono) = fonts.families.get(&FontFamily::Monospace).cloned() {
        fonts.families.insert(FontFamily::Proportional, mono);
    }
    fonts
}

fn apply_text_styles(ctx: &Context) {
    ctx.all_styles_mut(|s| {
        let mono = |size: f32| FontId::new(size, FontFamily::Monospace);
        s.text_styles.insert(TextStyle::Body, mono(13.0));
        s.text_styles.insert(TextStyle::Button, mono(13.0));
        s.text_styles.insert(TextStyle::Heading, mono(16.0));
        s.text_styles.insert(TextStyle::Small, mono(10.5));
        s.text_styles.insert(TextStyle::Monospace, mono(13.0));
        s.spacing.item_spacing = Vec2::new(4.0, 4.0);
        s.spacing.window_margin = Margin::same(6);
        s.spacing.button_padding = Vec2::new(6.0, 2.0);
        s.spacing.interact_size.y = 18.0;
        s.spacing.scroll = ScrollStyle {
            bar_width: 6.0,
            ..ScrollStyle::solid()
        };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every glyph the UI paints must exist in the bundled fonts, otherwise
    /// egui renders a box and logs `No font has "…"` at startup.
    #[test]
    fn bundled_fonts_cover_all_ui_glyphs() {
        let mut fonts = egui::epaint::text::Fonts::new(
            egui::epaint::text::TextOptions::default(),
            font_definitions(),
        );
        let text = String::from("▸▾▴◂×●○⊕…·≋°→←↑↓│─┌┐└┘├┤┬┴┼█▓▒░");
        let mono = FontId::new(13.0, FontFamily::Monospace);
        let missing: Vec<char> = text
            .chars()
            .filter(|c| !fonts.has_glyph(&mono, *c))
            .collect();
        assert!(missing.is_empty(), "missing glyphs: {missing:?}");
    }
}
