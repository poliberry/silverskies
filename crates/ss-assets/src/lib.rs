//! Shared UI assets for every Silver Skies app: the bundled fonts, Google's
//! V1 weather icon set (light and dark variants), SVG
//! rasterizing via resvg, egui texture caching and window-icon decoding.

pub mod glow;
pub mod menubar;

use egui::{ColorImage, Context, Id, TextureHandle, TextureOptions};
use ss_core::weather::icons::v1_icon;

/// Bundled fonts. DejaVu: Bitstream Vera licence (`assets/fonts/LICENSE`).
/// Google Sans, Google Sans Flex and Google Sans Code: SIL Open Font
/// Licence (`assets/fonts/google-sans/OFL-*.txt`), as static instances
/// subset to Latin + punctuation by hand from the variable fonts.
pub mod fonts {
    pub const DEJAVU_SANS_MONO: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");
    pub const DEJAVU_SANS: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
    pub const GOOGLE_SANS: &[u8] =
        include_bytes!("../assets/fonts/google-sans/GoogleSans-Regular.ttf");
    pub const GOOGLE_SANS_SEMIBOLD: &[u8] =
        include_bytes!("../assets/fonts/google-sans/GoogleSans-SemiBold.ttf");
    /// Flex at normal width, weight 300: light headings and numbers.
    pub const GOOGLE_SANS_FLEX_LIGHT: &[u8] =
        include_bytes!("../assets/fonts/google-sans/GoogleSansFlex-Light.ttf");
    /// Flex at width 125, weight 250: the big temperature.
    pub const GOOGLE_SANS_FLEX_WIDE_THIN: &[u8] =
        include_bytes!("../assets/fonts/google-sans/GoogleSansFlex-WideThin.ttf");
    /// Flex at width 125, weight 500: compact temperatures.
    pub const GOOGLE_SANS_FLEX_WIDE_MEDIUM: &[u8] =
        include_bytes!("../assets/fonts/google-sans/GoogleSansFlex-WideMedium.ttf");
    /// Flex, rounded (ROND 100): Material 3 Expressive display numbers.
    pub const GOOGLE_SANS_FLEX_ROUND_DISPLAY: &[u8] =
        include_bytes!("../assets/fonts/google-sans/GoogleSansFlex-RoundDisplay.ttf");
    /// Flex, rounded, medium weight: titles and labels.
    pub const GOOGLE_SANS_FLEX_ROUND_MEDIUM: &[u8] =
        include_bytes!("../assets/fonts/google-sans/GoogleSansFlex-RoundMedium.ttf");
    /// Flex, rounded, regular weight: body text.
    pub const GOOGLE_SANS_FLEX_ROUND_REGULAR: &[u8] =
        include_bytes!("../assets/fonts/google-sans/GoogleSansFlex-RoundRegular.ttf");
    pub const GOOGLE_SANS_CODE: &[u8] =
        include_bytes!("../assets/fonts/google-sans/GoogleSansCode-Regular.ttf");
    pub const GOOGLE_SANS_CODE_MEDIUM: &[u8] =
        include_bytes!("../assets/fonts/google-sans/GoogleSansCode-Medium.ttf");
}

/// Raw SVG of a Google V1 weather icon by name (see
/// `ss_core::weather::icons::V1_ICONS`), in its dark-theme or light-theme
/// variant. Unknown names fall back to `not_available`.
pub fn v1_svg(name: &str, dark: bool) -> &'static str {
    match (name, dark) {
        ("clear_day", false) => include_str!("../assets/weather-v1/light/clear_day.svg"),
        ("clear_day", true) => include_str!("../assets/weather-v1/dark/clear_day.svg"),
        ("clear_night", false) => include_str!("../assets/weather-v1/light/clear_night.svg"),
        ("clear_night", true) => include_str!("../assets/weather-v1/dark/clear_night.svg"),
        ("partly_cloudy_day", false) => {
            include_str!("../assets/weather-v1/light/partly_cloudy_day.svg")
        }
        ("partly_cloudy_day", true) => {
            include_str!("../assets/weather-v1/dark/partly_cloudy_day.svg")
        }
        ("partly_cloudy_night", false) => {
            include_str!("../assets/weather-v1/light/partly_cloudy_night.svg")
        }
        ("partly_cloudy_night", true) => {
            include_str!("../assets/weather-v1/dark/partly_cloudy_night.svg")
        }
        ("mostly_clear_night", false) => {
            include_str!("../assets/weather-v1/light/mostly_clear_night.svg")
        }
        ("mostly_clear_night", true) => {
            include_str!("../assets/weather-v1/dark/mostly_clear_night.svg")
        }
        ("cloudy", false) => include_str!("../assets/weather-v1/light/cloudy.svg"),
        ("cloudy", true) => include_str!("../assets/weather-v1/dark/cloudy.svg"),
        ("haze_fog", false) => include_str!("../assets/weather-v1/light/haze_fog.svg"),
        ("haze_fog", true) => include_str!("../assets/weather-v1/dark/haze_fog.svg"),
        ("drizzle", false) => include_str!("../assets/weather-v1/light/drizzle.svg"),
        ("drizzle", true) => include_str!("../assets/weather-v1/dark/drizzle.svg"),
        ("icy", false) => include_str!("../assets/weather-v1/light/icy.svg"),
        ("icy", true) => include_str!("../assets/weather-v1/dark/icy.svg"),
        ("rain_showers", false) => include_str!("../assets/weather-v1/light/rain_showers.svg"),
        ("rain_showers", true) => include_str!("../assets/weather-v1/dark/rain_showers.svg"),
        ("heavy_rain", false) => include_str!("../assets/weather-v1/light/heavy_rain.svg"),
        ("heavy_rain", true) => include_str!("../assets/weather-v1/dark/heavy_rain.svg"),
        ("flurries", false) => include_str!("../assets/weather-v1/light/flurries.svg"),
        ("flurries", true) => include_str!("../assets/weather-v1/dark/flurries.svg"),
        ("snow_showers", false) => include_str!("../assets/weather-v1/light/snow_showers.svg"),
        ("snow_showers", true) => include_str!("../assets/weather-v1/dark/snow_showers.svg"),
        ("heavy_snow", false) => include_str!("../assets/weather-v1/light/heavy_snow.svg"),
        ("heavy_snow", true) => include_str!("../assets/weather-v1/dark/heavy_snow.svg"),
        ("scattered_rain_showers_day", false) => {
            include_str!("../assets/weather-v1/light/scattered_rain_showers_day.svg")
        }
        ("scattered_rain_showers_day", true) => {
            include_str!("../assets/weather-v1/dark/scattered_rain_showers_day.svg")
        }
        ("scattered_rain_showers_night", false) => {
            include_str!("../assets/weather-v1/light/scattered_rain_showers_night.svg")
        }
        ("scattered_rain_showers_night", true) => {
            include_str!("../assets/weather-v1/dark/scattered_rain_showers_night.svg")
        }
        ("scattered_snow_showers_day", false) => {
            include_str!("../assets/weather-v1/light/scattered_snow_showers_day.svg")
        }
        ("scattered_snow_showers_day", true) => {
            include_str!("../assets/weather-v1/dark/scattered_snow_showers_day.svg")
        }
        ("scattered_snow_showers_night", false) => {
            include_str!("../assets/weather-v1/light/scattered_snow_showers_night.svg")
        }
        ("scattered_snow_showers_night", true) => {
            include_str!("../assets/weather-v1/dark/scattered_snow_showers_night.svg")
        }
        ("thunderstorms_day", false) => {
            include_str!("../assets/weather-v1/light/thunderstorms_day.svg")
        }
        ("thunderstorms_day", true) => {
            include_str!("../assets/weather-v1/dark/thunderstorms_day.svg")
        }
        ("thunderstorms_night", false) => {
            include_str!("../assets/weather-v1/light/thunderstorms_night.svg")
        }
        ("thunderstorms_night", true) => {
            include_str!("../assets/weather-v1/dark/thunderstorms_night.svg")
        }
        ("strong_thunderstorms", false) => {
            include_str!("../assets/weather-v1/light/strong_thunderstorms.svg")
        }
        ("strong_thunderstorms", true) => {
            include_str!("../assets/weather-v1/dark/strong_thunderstorms.svg")
        }
        ("not_available", false) => include_str!("../assets/weather-v1/light/not_available.svg"),
        ("not_available", true) => include_str!("../assets/weather-v1/dark/not_available.svg"),
        (_, true) => include_str!("../assets/weather-v1/dark/not_available.svg"),
        (_, false) => include_str!("../assets/weather-v1/light/not_available.svg"),
    }
}

pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Mix `c` towards white by `t` (0..=1).
pub fn lighten(c: [u8; 3], t: f32) -> [u8; 3] {
    c.map(|v| (v as f32 + (255.0 - v as f32) * t).round() as u8)
}

/// Alpha-blend an RGBA colour onto an opaque background.
pub fn over(fg: [u8; 4], bg: [u8; 3]) -> [u8; 3] {
    let a = fg[3] as f32 / 255.0;
    [0, 1, 2].map(|i| (fg[i] as f32 * a + bg[i] as f32 * (1.0 - a)).round() as u8)
}

/// Render an SVG to fit `width_px` x `height_px` (uniform scale, centred).
/// Embedded PNG `<image>`s (the brand marks' gradients) are supported.
pub fn rasterize(svg: &str, width_px: u32, height_px: u32) -> Option<ColorImage> {
    use resvg::{tiny_skia, usvg};
    if width_px == 0 || height_px == 0 {
        return None;
    }
    let tree = usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = (width_px as f32 / size.width()).min(height_px as f32 / size.height());
    let dx = (width_px as f32 - size.width() * scale) / 2.0;
    let dy = (height_px as f32 - size.height() * scale) / 2.0;
    let mut pixmap = tiny_skia::Pixmap::new(width_px, height_px)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy),
        &mut pixmap.as_mut(),
    );
    Some(ColorImage::from_rgba_premultiplied(
        [width_px as usize, height_px as usize],
        pixmap.data(),
    ))
}

/// Width / height of an SVG's intrinsic size, if it parses.
pub fn svg_aspect(svg: &str) -> Option<f32> {
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &Default::default()).ok()?;
    let s = tree.size();
    Some(s.width() / s.height().max(1e-3))
}

/// Points -> physical pixels for crisp rasterizing.
pub fn device_px(ctx: &Context, px: u32) -> u32 {
    ((px as f32 * ctx.pixels_per_point()).round() as u32).max(1)
}

/// Cached texture for an arbitrary SVG rendered `height_pt` points tall.
/// `key` must change whenever `svg` does.
pub fn svg_texture(ctx: &Context, key: &str, svg: &str, height_pt: u32) -> TextureHandle {
    let h = device_px(ctx, height_pt);
    let id = Id::unique(("ss-svg", key, h));
    if let Some(t) = ctx.data_mut(|d| d.get_temp::<TextureHandle>(id)) {
        return t;
    }
    let aspect = svg_aspect(svg).unwrap_or(1.0);
    let w = ((h as f32 * aspect).round() as u32).max(1);
    let image = rasterize(svg, w, h)
        .unwrap_or_else(|| ColorImage::filled([1, 1], egui::Color32::TRANSPARENT));
    let tex = ctx.load_texture(format!("svg-{key}-{h}"), image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// Cached texture for the weather icon of a WMO `code`, `px` points square.
/// `dark` picks the dark-theme variant.
pub fn weather_texture(
    ctx: &Context,
    code: u8,
    is_day: bool,
    dark: bool,
    px: u32,
) -> TextureHandle {
    let name = v1_icon(code, is_day);
    let raster = device_px(ctx, px);
    let id = Id::unique(("wx-v1", name, dark, px, raster));
    if let Some(t) = ctx.data_mut(|d| d.get_temp::<TextureHandle>(id)) {
        return t;
    }
    let image = rasterize(v1_svg(name, dark), raster, raster)
        .unwrap_or_else(|| ColorImage::filled([1, 1], egui::Color32::TRANSPARENT));
    let tex = ctx.load_texture(format!("wx-{name}-{px}"), image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// Ready-to-add weather icon image for a WMO `code`, `px` points square.
pub fn weather_image(
    ctx: &Context,
    code: u8,
    is_day: bool,
    dark: bool,
    px: u32,
) -> egui::Image<'static> {
    let tex = weather_texture(ctx, code, is_day, dark, px);
    egui::Image::from_texture(&tex).fit_to_exact_size(egui::vec2(px as f32, px as f32))
}

/// Cached texture of an embedded PNG (e.g. a raster logo). `key` must be
/// unique per image.
pub fn png_texture(ctx: &Context, key: &str, png: &[u8]) -> TextureHandle {
    let id = Id::unique(("ss-png", key));
    if let Some(t) = ctx.data_mut(|d| d.get_temp::<TextureHandle>(id)) {
        return t;
    }
    let image = image::load_from_memory(png)
        .map(|i| {
            let rgba = i.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
        })
        .unwrap_or_else(|_| ColorImage::filled([1, 1], egui::Color32::TRANSPARENT));
    let tex = ctx.load_texture(format!("png-{key}"), image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// Decode a PNG window icon.
pub fn icon_data(png: &[u8]) -> std::sync::Arc<egui::IconData> {
    let img = image::load_from_memory(png)
        .map(|i| i.to_rgba8())
        .unwrap_or_else(|_| image::RgbaImage::new(1, 1));
    let (width, height) = img.dimensions();
    std::sync::Arc::new(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn has_pixels(img: &ColorImage) -> bool {
        img.pixels.iter().any(|c| c.a() > 0)
    }

    #[test]
    fn every_v1_icon_rasterizes_in_both_themes() {
        for name in ss_core::weather::icons::V1_ICONS {
            for dark in [true, false] {
                let img = rasterize(v1_svg(name, dark), 48, 48)
                    .unwrap_or_else(|| panic!("{name} failed to rasterize"));
                assert!(has_pixels(&img), "{name} (dark={dark}) is empty");
            }
        }
    }

    #[test]
    fn bundled_fonts_parse() {
        for (name, bytes) in [
            ("sans", fonts::GOOGLE_SANS),
            ("sans-semibold", fonts::GOOGLE_SANS_SEMIBOLD),
            ("flex-light", fonts::GOOGLE_SANS_FLEX_LIGHT),
            ("flex-wide-thin", fonts::GOOGLE_SANS_FLEX_WIDE_THIN),
            ("flex-wide-medium", fonts::GOOGLE_SANS_FLEX_WIDE_MEDIUM),
            ("code", fonts::GOOGLE_SANS_CODE),
            ("code-medium", fonts::GOOGLE_SANS_CODE_MEDIUM),
        ] {
            assert!(bytes.len() > 10_000, "{name} too small");
            assert_eq!(&bytes[..4], &[0, 1, 0, 0], "{name} is not a TrueType font");
        }
    }

    #[test]
    fn embedded_png_images_render() {
        // 1x1 opaque red PNG inside an SVG <image>.
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
        let svg = format!(
            r#"<svg width="4" height="4" viewBox="0 0 4 4" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><image x="0" y="0" width="4" height="4" preserveAspectRatio="none" xlink:href="data:image/png;base64,{png}"/></svg>"#
        );
        let img = rasterize(&svg, 4, 4).unwrap();
        let c = img.pixels[5];
        assert!(c.r() > 200 && c.a() > 200, "got {c:?}");
    }

    #[test]
    fn aspect_reads_viewbox() {
        let svg = r#"<svg width="212" height="60" viewBox="0 0 212 60" xmlns="http://www.w3.org/2000/svg"/>"#;
        let a = svg_aspect(svg).unwrap();
        assert!((a - 212.0 / 60.0).abs() < 1e-3);
    }
}
