//! The Silver Skies look: Material 3 Expressive. Every colour comes from a
//! tonal scheme generated from one seed (the sky's accent, so the whole app
//! re-tints with the weather), surfaces are borderless tonal containers with
//! large rounded corners, controls are pills, and type is rounded Google
//! Sans Flex.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{
    epaint::Shadow, style::ScrollStyle, Color32, Context, CornerRadius, FontData, FontDefinitions,
    FontFamily, FontId, Margin, RichText, Stroke, TextStyle, Theme, Ui, Vec2, Visuals,
};
use ss_core::theme::{brightness, resolve_palette, Palette};
use ss_core::{Config, ThemePref};

use crate::m3::{self, corner, Scheme};

/// Corner radius of bento tiles (Material "extra large").
pub const RADIUS: u8 = corner::EXTRA_LARGE;
/// Corner radius of floating controls on the map.
pub const RADIUS_SMALL: u8 = corner::LARGE_INCREASED;
/// Gap between bento tiles.
pub const GAP: f32 = 12.0;

pub fn c32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

pub fn c32a(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

pub fn alpha(c: [u8; 3], a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
}

fn rgb(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

fn rgba(c: Color32) -> [u8; 4] {
    [c.r(), c.g(), c.b(), 255]
}

pub fn is_dark(p: &Palette) -> bool {
    brightness(p.bg) < 128.0
}

/// Palette key: (accent, background).
type SchemeKey = ([u8; 3], [u8; 3]);

thread_local! {
    /// Schemes by the palette they were mapped to, so any view holding a
    /// `Palette` can get the full set of Material roles back.
    static SCHEMES: RefCell<HashMap<SchemeKey, Scheme>> = RefCell::new(HashMap::new());
}

/// The Material scheme behind palette `p`.
pub fn scheme(p: &Palette) -> Scheme {
    let key = (p.a0, p.bg);
    SCHEMES.with(|s| {
        *s.borrow_mut()
            .entry(key)
            .or_insert_with(|| Scheme::from_seed(p.a0, is_dark(p)))
    })
}

/// Map a scheme onto the shared `Palette` fields (used by shared crates and
/// older code paths), remembering the scheme for [`scheme`].
pub fn palette_from_scheme(s: &Scheme) -> Palette {
    let p = Palette {
        a0: rgb(s.primary),
        bg: rgb(s.surface),
        surface: rgb(s.surface_container),
        surface3: rgb(s.surface_container_highest),
        border: rgba(s.outline_variant),
        text: rgb(s.on_surface),
        text2: rgba(s.on_surface_variant),
        text3: rgba(s.outline),
        danger: rgb(s.error),
        safe: if s.dark {
            [0x7d, 0xdc, 0xa8]
        } else {
            [0x1a, 0x7a, 0x48]
        },
    };
    SCHEMES.with(|m| {
        m.borrow_mut().insert((p.a0, p.bg), *s);
    });
    p
}

/// The palette the app renders with: a Material 3 scheme seeded with the
/// sky's accent (or with a builtin/custom theme's accent).
pub fn simple_palette(cfg: &Config, condition_accent: [u8; 3], system_dark: bool) -> Palette {
    let (seed, dark) = if cfg.theme_id != "default" {
        let p = resolve_palette(cfg, condition_accent, system_dark);
        (p.a0, is_dark(&p))
    } else {
        let light = match cfg.theme {
            ThemePref::Light => true,
            ThemePref::Dark => false,
            ThemePref::System => !system_dark,
        };
        (condition_accent, !light)
    };
    palette_from_scheme(&Scheme::from_seed(seed, dark))
}

/// A tile: a tonal surface container with extra-large corners, no border.
pub fn card_frame(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(scheme(p).surface_container)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(22))
}

// --- type -------------------------------------------------------------------

pub fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

/// Rounded Google Sans Flex at display size: the big temperature.
pub fn display_font(size: f32) -> FontId {
    FontId::new(size, family("round-display"))
}

/// Rounded medium: titles, labels and buttons.
pub fn medium_font(size: f32) -> FontId {
    FontId::new(size, family("round-medium"))
}

/// Rounded regular: body text (also the default proportional font).
pub fn body_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// A Material label: medium weight, sentence case.
pub fn label(text: &str, size: f32, color: Color32) -> RichText {
    RichText::new(text).font(medium_font(size)).color(color)
}

pub fn visuals_for(p: &Palette) -> Visuals {
    let s = scheme(p);
    let mut v = if s.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let none = Stroke::NONE;
    let pill = CornerRadius::same(corner::FULL);

    v.override_text_color = Some(s.on_surface);
    v.panel_fill = s.surface;
    v.window_fill = s.surface_container_high;
    v.extreme_bg_color = s.surface_container_highest;
    v.faint_bg_color = s.surface_container_low;
    v.code_bg_color = s.surface_container_highest;
    v.selection.bg_fill = m3::with_alpha(s.primary, 90);
    v.selection.stroke = Stroke::new(1.5, s.primary);
    v.hyperlink_color = s.primary;
    v.warn_fg_color = s.tertiary;
    v.error_fg_color = s.error;
    v.window_stroke = none;
    v.window_corner_radius = CornerRadius::same(corner::EXTRA_LARGE);
    v.menu_corner_radius = CornerRadius::same(corner::LARGE);
    v.text_edit_bg_color = Some(s.surface_container_highest);
    let shadow_a = if s.dark { 120 } else { 40 };
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(shadow_a),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(shadow_a),
    };
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.35 };

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = s.surface_container;
    w.noninteractive.weak_bg_fill = s.surface_container;
    w.noninteractive.bg_stroke = Stroke::new(1.0, s.outline_variant);
    w.noninteractive.fg_stroke = Stroke::new(1.0, s.on_surface_variant);

    // Tonal pill buttons; a state layer of the content colour on hover/press.
    w.inactive.bg_fill = s.surface_container_highest;
    w.inactive.weak_bg_fill = s.surface_container_highest;
    w.inactive.bg_stroke = none;
    w.inactive.fg_stroke = Stroke::new(1.5, s.on_surface);

    let layer = |base: Color32, a: f32| base.lerp_to_gamma(s.on_surface, a);
    w.hovered.bg_fill = layer(s.secondary_container, 0.08);
    w.hovered.weak_bg_fill = layer(s.secondary_container, 0.08);
    w.hovered.bg_stroke = none;
    w.hovered.fg_stroke = Stroke::new(1.5, s.on_secondary_container);

    w.active.bg_fill = layer(s.secondary_container, 0.16);
    w.active.weak_bg_fill = layer(s.secondary_container, 0.16);
    w.active.bg_stroke = none;
    w.active.fg_stroke = Stroke::new(1.5, s.on_secondary_container);

    w.open.bg_fill = s.secondary_container;
    w.open.weak_bg_fill = s.secondary_container;
    w.open.bg_stroke = none;
    w.open.fg_stroke = Stroke::new(1.5, s.on_secondary_container);

    // `bg_fill` (unlike `weak_bg_fill`, which buttons use) draws checkbox
    // boxes and slider tracks: give it real contrast against dialogs so an
    // unticked checkbox is visible.
    let well = if s.dark {
        s.surface_container_lowest
    } else {
        s.outline_variant
    };
    w.inactive.bg_fill = well;
    w.hovered.bg_fill = well.lerp_to_gamma(s.on_surface, 0.12);
    w.active.bg_fill = well.lerp_to_gamma(s.on_surface, 0.2);

    for st in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        st.corner_radius = pill;
        st.expansion = 0.0;
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
        s.text_styles.insert(TextStyle::Body, body_font(14.5));
        s.text_styles.insert(TextStyle::Button, medium_font(14.0));
        s.text_styles.insert(TextStyle::Heading, display_font(22.0));
        s.text_styles.insert(TextStyle::Small, body_font(12.0));
        s.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        );
        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.window_margin = Margin::same(24);
        s.spacing.button_padding = Vec2::new(16.0, 8.0);
        s.spacing.interact_size.y = 36.0;
        s.spacing.slider_rail_height = 8.0;
        s.spacing.scroll = ScrollStyle {
            bar_width: 6.0,
            floating: true,
            ..ScrollStyle::solid()
        };
    });
}

/// Rounded Google Sans Flex for UI text, Google Sans Code for mono; plain
/// Google Sans then DejaVu as fallbacks so symbols always have a glyph.
pub fn font_definitions() -> FontDefinitions {
    use ss_assets::fonts as f;
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        ("RoundRegular", f::GOOGLE_SANS_FLEX_ROUND_REGULAR),
        ("RoundMedium", f::GOOGLE_SANS_FLEX_ROUND_MEDIUM),
        ("RoundDisplay", f::GOOGLE_SANS_FLEX_ROUND_DISPLAY),
        ("GoogleSans", f::GOOGLE_SANS),
        ("GoogleSansCode", f::GOOGLE_SANS_CODE),
        ("DejaVuSans", f::DEJAVU_SANS),
        ("DejaVuSansMono", f::DEJAVU_SANS_MONO),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    let chain = |first: &str| -> Vec<String> {
        vec![
            first.to_owned(),
            "GoogleSans".to_owned(),
            "DejaVuSans".to_owned(),
        ]
    };
    fonts
        .families
        .insert(FontFamily::Proportional, chain("RoundRegular"));
    fonts.families.insert(
        FontFamily::Monospace,
        vec!["GoogleSansCode".to_owned(), "DejaVuSansMono".to_owned()],
    );
    for (fam, first) in [
        ("round-medium", "RoundMedium"),
        ("round-display", "RoundDisplay"),
    ] {
        fonts.families.insert(family(fam), chain(first));
    }
    fonts
}

/// The assistant chat window's look: the same Material scheme.
pub fn chat_theme(p: &Palette) -> ss_ai::chat::ChatTheme {
    let s = scheme(p);
    ss_ai::chat::ChatTheme {
        bg: s.surface,
        surface: s.surface_container_high,
        border: s.outline_variant,
        text: s.on_surface,
        muted: s.on_surface_variant,
        accent: s.primary,
        user_bubble: s.primary_container,
        danger: s.error,
        corner: corner::LARGE_INCREASED,
        caption: medium_font(12.0),
    }
}

const LOGOMARK_DARK: &[u8] = include_bytes!("../assets/brand/logomark-dark.png");
const LOGOMARK_LIGHT: &[u8] = include_bytes!("../assets/brand/logomark-light.png");

/// The Silver Skies logomark (icon + SILVER SKIES wordmark) for the current
/// theme, `height` points tall.
pub fn logomark(ui: &Ui, p: &Palette, height: f32) -> egui::Image<'static> {
    let (key, png) = if is_dark(p) {
        ("ss-logomark-dark", LOGOMARK_DARK)
    } else {
        ("ss-logomark-light", LOGOMARK_LIGHT)
    };
    let tex = ss_assets::png_texture(ui.ctx(), key, png);
    let s = tex.size_vec2();
    egui::Image::from_texture(&tex)
        .fit_to_exact_size(egui::vec2(height * s.x / s.y.max(1.0), height))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two schemes for the default purple seed (tests and previews).
    fn web_dark() -> Palette {
        palette_from_scheme(&Scheme::from_seed([138, 100, 255], true))
    }

    fn web_light() -> Palette {
        palette_from_scheme(&Scheme::from_seed([138, 100, 255], false))
    }

    #[test]
    fn logomarks_decode() {
        for png in [LOGOMARK_DARK, LOGOMARK_LIGHT] {
            let img = image::load_from_memory(png).unwrap();
            assert_eq!((img.width(), img.height()), (150, 60));
        }
    }

    #[test]
    fn visuals_follow_palette_brightness() {
        assert!(visuals_for(&web_dark()).dark_mode);
        assert!(!visuals_for(&web_light()).dark_mode);
    }

    #[test]
    fn default_theme_is_seeded_by_the_sky() {
        let mut cfg = Config {
            theme: ThemePref::Dark,
            ..Default::default()
        };
        let green = simple_palette(&cfg, [10, 200, 30], true);
        let purple = simple_palette(&cfg, [138, 100, 255], true);
        assert!(is_dark(&green));
        assert_ne!(green.a0, purple.a0);
        // The scheme comes back from the palette.
        assert_eq!(rgb(scheme(&green).primary), green.a0);
        cfg.theme = ThemePref::Light;
        assert!(!is_dark(&simple_palette(&cfg, [10, 200, 30], true)));
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
            family("round-display"),
            family("round-medium"),
        ] {
            let id = FontId::new(14.0, fam.clone());
            let missing: Vec<char> = "°↑↓←→·…✕☂✦⚠•▶❚—↻✓Aa09%"
                .chars()
                .filter(|c| !fonts.has_glyph(&id, *c))
                .collect();
            assert!(missing.is_empty(), "{fam:?} missing glyphs: {missing:?}");
        }
    }
}
