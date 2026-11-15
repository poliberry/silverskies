//! Shared UI assets for every Silver Skies app: the bundled DejaVu fonts, the
//! Pixel Weather SVG icon set (recoloured to the active palette), SVG
//! rasterizing via resvg, egui texture caching and window-icon decoding.

use egui::{ColorImage, Context, Id, TextureHandle, TextureOptions};
use ss_core::theme::Palette;
use ss_core::weather::icons::PixelIcon;

/// Bundled fonts (Bitstream Vera licence, see `assets/fonts/LICENSE`).
pub mod fonts {
    pub const DEJAVU_SANS_MONO: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");
    pub const DEJAVU_SANS: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
}

/// Raw Pixel Weather SVG source for an icon.
pub fn svg_source(icon: PixelIcon) -> &'static str {
    match icon {
        PixelIcon::Sunny => include_str!("../assets/pixel-weather/sunny.svg"),
        PixelIcon::ClearNight => include_str!("../assets/pixel-weather/clear_night.svg"),
        PixelIcon::MostlySunny => include_str!("../assets/pixel-weather/mostly_sunny.svg"),
        PixelIcon::MostlyClearNight => {
            include_str!("../assets/pixel-weather/mostly_clear_night.svg")
        }
        PixelIcon::PartlyCloudy => include_str!("../assets/pixel-weather/partly_cloudy.svg"),
        PixelIcon::PartlyCloudyNight => {
            include_str!("../assets/pixel-weather/partly_cloudy_night.svg")
        }
        PixelIcon::Cloudy => include_str!("../assets/pixel-weather/cloudy.svg"),
        PixelIcon::Drizzle => include_str!("../assets/pixel-weather/drizzle.svg"),
        PixelIcon::WintryMix => include_str!("../assets/pixel-weather/wintry_mix.svg"),
        PixelIcon::SleetHail => include_str!("../assets/pixel-weather/sleet_hail.svg"),
        PixelIcon::HeavyRain => include_str!("../assets/pixel-weather/heavy_rain.svg"),
        PixelIcon::Flurries => include_str!("../assets/pixel-weather/flurries.svg"),
        PixelIcon::HeavySnow => include_str!("../assets/pixel-weather/heavy_snow.svg"),
        PixelIcon::StrongThunderstorms => {
            include_str!("../assets/pixel-weather/strong_thunderstorms.svg")
        }
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

/// Recolour the Pixel Weather source palette to the app palette.
pub fn reskin(svg: &str, p: &Palette) -> String {
    let sun = hex(p.a0);
    let precip_light = hex(lighten(p.a0, 0.35));
    let cloud = hex(over(p.text2, p.bg));
    let light = hex(p.text);

    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(i) = rest.find('#') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let n = after.bytes().take_while(|b| b.is_ascii_hexdigit()).count();
        let token = after[..n].to_ascii_lowercase();
        // The whole hex run is compared, so "#fff" never touches longer
        // colours such as "#fffa00".
        let repl = match token.as_str() {
            "fcbd00" | "619dff" => Some(&sun),
            "8ab4f8" => Some(&precip_light),
            "bdc1c6" => Some(&cloud),
            "e8eaed" | "fff" | "ffffff" => Some(&light),
            _ => None,
        };
        match repl {
            Some(r) => out.push_str(r),
            None => {
                out.push('#');
                out.push_str(&after[..n]);
            }
        }
        rest = &after[n..];
    }
    out.push_str(rest);
    out
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

/// Cached texture for a weather icon, displayed at `px` points square.
pub fn weather_texture(ctx: &Context, icon: PixelIcon, p: &Palette, px: u32) -> TextureHandle {
    let raster = device_px(ctx, px);
    let id = Id::unique(("pw", icon.name(), px, raster, p.a0, p.text, p.bg, p.text2));
    if let Some(t) = ctx.data_mut(|d| d.get_temp::<TextureHandle>(id)) {
        return t;
    }
    let image = rasterize(&reskin(svg_source(icon), p), raster, raster)
        .unwrap_or_else(|| ColorImage::filled([1, 1], egui::Color32::TRANSPARENT));
    let tex = ctx.load_texture(
        format!("pw-{}-{px}", icon.name()),
        image,
        TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// Ready-to-add weather icon image, `px` points square.
pub fn weather_image(ctx: &Context, icon: PixelIcon, p: &Palette, px: u32) -> egui::Image<'static> {
    let tex = weather_texture(ctx, icon, p, px);
    egui::Image::from_texture(&tex).fit_to_exact_size(egui::vec2(px as f32, px as f32))
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
    use ss_core::theme::dark_base;

    const SOURCES: [&str; 6] = [
        "#fcbd00", "#bdc1c6", "#e8eaed", "#fff", "#8ab4f8", "#619dff",
    ];

    pub(crate) fn has_pixels(img: &ColorImage) -> bool {
        img.pixels.iter().any(|c| c.a() > 0)
    }

    #[test]
    fn reskin_replaces_all_source_colours() {
        let p = dark_base();
        for icon in PixelIcon::ALL {
            let out = reskin(svg_source(icon), &p).to_ascii_lowercase();
            for s in SOURCES {
                assert!(!out.contains(s), "{} still has {s}", icon.name());
            }
        }
    }

    #[test]
    fn reskin_keeps_none_and_longer_hex() {
        let p = dark_base();
        let out = reskin(r##"<a fill="none" b="#fff" c="#ffffff" d="#fffa00"/>"##, &p);
        assert!(out.contains(r#"fill="none""#));
        assert!(out.contains(r##"d="#fffa00""##));
        assert!(!out.contains(r##""#fff""##));
        assert!(!out.contains("#ffffff"));
    }

    #[test]
    fn icons_rasterize() {
        let p = dark_base();
        for icon in PixelIcon::ALL {
            let img = rasterize(&reskin(svg_source(icon), &p), 48, 48)
                .unwrap_or_else(|| panic!("{} failed to rasterize", icon.name()));
            assert_eq!(img.size, [48, 48]);
            assert!(has_pixels(&img), "{} is empty", icon.name());
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
