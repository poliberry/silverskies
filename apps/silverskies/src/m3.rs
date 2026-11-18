//! Material 3 Expressive building blocks for egui: a tonal colour scheme
//! generated from one seed colour, and the expressive shapes (cookies,
//! flowers, bursts, wavy lines, the morphing loading indicator).
//!
//! Colour: Material 3 derives every role from tonal palettes in the HCT
//! colour space. This uses OKLCH instead (tone mapped through luminance),
//! which gives the same structure — a primary, secondary, tertiary, neutral
//! and neutral-variant palette at fixed tones per role — with a little less
//! precision and no extra dependencies.

use egui::epaint::{Mesh, PathStroke};
use egui::{pos2, Color32, Painter, Pos2, Shape, Stroke};

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB (0..=255) → OKLCH (L 0..1, C, h degrees).
pub fn rgb_to_oklch(rgb: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = rgb.map(|v| srgb_to_linear(v as f32 / 255.0));
    let l = 0.412_221_5 * r + 0.536_332_55 * g + 0.051_445_99 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    let ll = 0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_;
    let a = 1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_;
    let bb = 0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_;
    let c = (a * a + bb * bb).sqrt();
    let h = bb.atan2(a).to_degrees().rem_euclid(360.0);
    (ll, c, h)
}

/// OKLCH → linear sRGB (may be out of gamut).
fn oklch_to_linear(l: f32, c: f32, h: f32) -> [f32; 3] {
    let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ]
}

/// OKLCH → sRGB, reducing chroma until the colour fits the sRGB gamut.
pub fn oklch_to_rgb(l: f32, c: f32, h: f32) -> [u8; 3] {
    let l = l.clamp(0.0, 1.0);
    let mut c = c.max(0.0);
    for _ in 0..40 {
        let lin = oklch_to_linear(l, c, h);
        if lin.iter().all(|v| (-0.0005..=1.0005).contains(v)) {
            return lin.map(|v| (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8);
        }
        c *= 0.9;
    }
    let lin = oklch_to_linear(l, 0.0, h);
    lin.map(|v| (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8)
}

/// Material "tone" is CIE L* (0..=100); OKLab L is the cube root of
/// relative luminance, so convert through Y.
pub fn tone_to_oklab_l(tone: f32) -> f32 {
    let y = if tone > 8.0 {
        ((tone + 16.0) / 116.0).powi(3)
    } else {
        tone / 903.3
    };
    y.cbrt()
}

/// A tonal palette: one hue and chroma, any tone 0 (black) ..= 100 (white).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tonal {
    pub hue: f32,
    pub chroma: f32,
}

impl Tonal {
    /// The colour at `tone` (0..=100).
    pub fn tone(&self, tone: f32) -> Color32 {
        // OKLCH lightness is close to perceived tone; the extremes are
        // exactly black and white.
        if tone <= 0.0 {
            return Color32::BLACK;
        }
        if tone >= 100.0 {
            return Color32::WHITE;
        }
        let l = tone_to_oklab_l(tone);
        let [r, g, b] = oklch_to_rgb(l, self.chroma, self.hue);
        Color32::from_rgb(r, g, b)
    }
}

/// The Material 3 colour roles this app uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scheme {
    pub dark: bool,
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub secondary: Color32,
    pub secondary_container: Color32,
    pub on_secondary_container: Color32,
    pub tertiary: Color32,
    pub tertiary_container: Color32,
    pub on_tertiary_container: Color32,
    pub error: Color32,
    pub error_container: Color32,
    pub on_error_container: Color32,
    pub surface: Color32,
    pub surface_container_lowest: Color32,
    pub surface_container_low: Color32,
    pub surface_container: Color32,
    pub surface_container_high: Color32,
    pub surface_container_highest: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub outline: Color32,
    pub outline_variant: Color32,
    pub inverse_surface: Color32,
    pub inverse_on_surface: Color32,
}

impl Scheme {
    /// Build a scheme from a seed colour (Material's "tonal spot" variant,
    /// with a little more chroma for the Expressive look).
    pub fn from_seed(seed: [u8; 3], dark: bool) -> Scheme {
        let (_, c, h) = rgb_to_oklch(seed);
        // Greys get a gentle default chroma so containers still read as tinted.
        let base_c = c.max(0.06);
        let primary = Tonal {
            hue: h,
            chroma: base_c.clamp(0.09, 0.16),
        };
        let secondary = Tonal {
            hue: h,
            chroma: (base_c * 0.35).clamp(0.025, 0.05),
        };
        let tertiary = Tonal {
            hue: (h + 60.0).rem_euclid(360.0),
            chroma: (base_c * 0.8).clamp(0.06, 0.12),
        };
        let neutral = Tonal {
            hue: h,
            chroma: 0.008,
        };
        let neutral_v = Tonal {
            hue: h,
            chroma: 0.018,
        };
        let error = Tonal {
            hue: 27.0,
            chroma: 0.17,
        };
        if dark {
            Scheme {
                dark,
                primary: primary.tone(80.0),
                on_primary: primary.tone(20.0),
                primary_container: primary.tone(30.0),
                on_primary_container: primary.tone(90.0),
                secondary: secondary.tone(80.0),
                secondary_container: secondary.tone(30.0),
                on_secondary_container: secondary.tone(90.0),
                tertiary: tertiary.tone(80.0),
                tertiary_container: tertiary.tone(30.0),
                on_tertiary_container: tertiary.tone(90.0),
                error: error.tone(80.0),
                error_container: error.tone(30.0),
                on_error_container: error.tone(90.0),
                surface: neutral.tone(6.0),
                surface_container_lowest: neutral.tone(4.0),
                surface_container_low: neutral.tone(10.0),
                surface_container: neutral.tone(12.0),
                surface_container_high: neutral.tone(17.0),
                surface_container_highest: neutral.tone(22.0),
                on_surface: neutral.tone(90.0),
                on_surface_variant: neutral_v.tone(80.0),
                outline: neutral_v.tone(60.0),
                outline_variant: neutral_v.tone(30.0),
                inverse_surface: neutral.tone(90.0),
                inverse_on_surface: neutral.tone(20.0),
            }
        } else {
            Scheme {
                dark,
                primary: primary.tone(40.0),
                on_primary: primary.tone(100.0),
                primary_container: primary.tone(90.0),
                on_primary_container: primary.tone(10.0),
                secondary: secondary.tone(40.0),
                secondary_container: secondary.tone(90.0),
                on_secondary_container: secondary.tone(10.0),
                tertiary: tertiary.tone(40.0),
                tertiary_container: tertiary.tone(90.0),
                on_tertiary_container: tertiary.tone(10.0),
                error: error.tone(40.0),
                error_container: error.tone(90.0),
                on_error_container: error.tone(10.0),
                surface: neutral.tone(98.0),
                surface_container_lowest: neutral.tone(100.0),
                surface_container_low: neutral.tone(96.0),
                surface_container: neutral.tone(94.0),
                surface_container_high: neutral.tone(92.0),
                surface_container_highest: neutral.tone(90.0),
                on_surface: neutral.tone(10.0),
                on_surface_variant: neutral_v.tone(30.0),
                outline: neutral_v.tone(50.0),
                outline_variant: neutral_v.tone(80.0),
                inverse_surface: neutral.tone(20.0),
                inverse_on_surface: neutral.tone(95.0),
            }
        }
    }
}

/// WCAG relative luminance contrast ratio between two colours.
#[cfg(test)]
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let lum = |c: Color32| {
        let [r, g, b] = [c.r(), c.g(), c.b()].map(|v| srgb_to_linear(v as f32 / 255.0));
        0.2126 * r + 0.7152 * g + 0.0722 * b
    };
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `c` with alpha `a` (0..=255), for state layers.
pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

// ---------------------------------------------------------------------------
// Shape
// ---------------------------------------------------------------------------

/// Material shape-scale corner radii, in points.
pub mod corner {
    pub const SMALL: u8 = 8;
    pub const LARGE: u8 = 16;
    pub const LARGE_INCREASED: u8 = 20;
    pub const EXTRA_LARGE: u8 = 28;
    pub const FULL: u8 = 255;
}

/// Outline of a star-shaped "cookie"/"flower" with `lobes` bumps: the
/// radius swings by `depth` (0 = circle) around `radius`.
pub fn lobed_points(center: Pos2, radius: f32, lobes: u32, depth: f32, rotation: f32) -> Vec<Pos2> {
    let n = (lobes.max(1) * 24).max(96);
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            // A raised-cosine bump keeps the lobes soft like Material's cookies.
            let bump = 0.5 + 0.5 * (lobes as f32 * (t + rotation)).cos();
            let r = radius * (1.0 - depth + depth * bump * 2.0).max(0.0) / (1.0 + depth);
            center + egui::vec2(t.cos() * r, t.sin() * r)
        })
        .collect()
}

/// Fill a star-shaped outline (fan from the centre; valid because every
/// point is visible from the centre).
pub fn fill_star(painter: &Painter, center: Pos2, pts: &[Pos2], fill: Color32) {
    if pts.len() < 3 {
        return;
    }
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, fill);
    for p in pts {
        mesh.colored_vertex(*p, fill);
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(mesh);
}

/// Draw a Material "cookie" (soft lobed) shape.
pub fn cookie(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    lobes: u32,
    rotation: f32,
    fill: Color32,
) {
    let pts = lobed_points(center, radius, lobes, 0.07, rotation);
    fill_star(painter, center, &pts, fill);
}

/// Draw a Material "sunny"/burst shape: many short points.
pub fn sunny(painter: &Painter, center: Pos2, radius: f32, rotation: f32, fill: Color32) {
    let pts = lobed_points(center, radius, 12, 0.045, rotation);
    fill_star(painter, center, &pts, fill);
}

/// Points along a wave that follows the polyline `path`.
pub fn wavy(path: &[Pos2], amplitude: f32, wavelength: f32) -> Vec<Pos2> {
    let mut out = Vec::new();
    let mut dist = 0.0;
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg = b - a;
        let len = seg.length();
        if len <= f32::EPSILON {
            continue;
        }
        let dir = seg / len;
        let normal = egui::vec2(-dir.y, dir.x);
        let steps = (len / 2.0).ceil().max(1.0) as usize;
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let d = dist + len * t;
            let off = (d / wavelength * std::f32::consts::TAU).sin() * amplitude;
            out.push(a + seg * t + normal * off);
        }
        dist += len;
    }
    if let Some(last) = path.last() {
        out.push(*last);
    }
    out
}

/// Material 3 Expressive loading indicator: a shape that rotates and morphs
/// between a soft cookie and a sharper burst. `t` is time in seconds.
pub fn loading_indicator(
    painter: &Painter,
    center: Pos2,
    size: f32,
    t: f64,
    color: Color32,
    contained: Option<Color32>,
) {
    if let Some(bg) = contained {
        painter.circle_filled(center, size * 0.5, bg);
    }
    let phase = ((t * 0.8).sin() * 0.5 + 0.5) as f32;
    let lobes = if phase > 0.5 { 7 } else { 9 };
    let depth = 0.05 + 0.12 * phase;
    let rot = (t * 1.6) as f32;
    let r = size * if contained.is_some() { 0.32 } else { 0.42 };
    let pts = lobed_points(center, r, lobes, depth, rot);
    fill_star(painter, center, &pts, color);
    painter.ctx().request_repaint();
}

/// A thick rounded line (Material's progress/track style).
pub fn rounded_line(painter: &Painter, a: Pos2, b: Pos2, width: f32, color: Color32) {
    painter.line_segment([a, b], Stroke::new(width, color));
    painter.circle_filled(a, width / 2.0, color);
    painter.circle_filled(b, width / 2.0, color);
}

/// Stroke a polyline with round joins and caps.
pub fn polyline(painter: &Painter, pts: Vec<Pos2>, width: f32, color: Color32) {
    if let (Some(a), Some(b)) = (pts.first().copied(), pts.last().copied()) {
        painter.add(Shape::line(pts, PathStroke::new(width, color)));
        painter.circle_filled(a, width / 2.0, color);
        painter.circle_filled(b, width / 2.0, color);
    }
}

/// Convenience: a point at polar coordinates around `c`.
pub fn polar(c: Pos2, r: f32, angle: f32) -> Pos2 {
    pos2(c.x + angle.cos() * r, c.y + angle.sin() * r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklch_roundtrips_srgb() {
        for rgb in [
            [255, 0, 0],
            [12, 200, 90],
            [138, 100, 255],
            [128, 128, 128],
            [0, 0, 0],
        ] {
            let (l, c, h) = rgb_to_oklch(rgb);
            let back = oklch_to_rgb(l, c, h);
            for i in 0..3 {
                assert!(
                    (back[i] as i32 - rgb[i] as i32).abs() <= 1,
                    "{rgb:?} -> {back:?}"
                );
            }
        }
    }

    #[test]
    fn tones_run_dark_to_light() {
        let t = Tonal {
            hue: 280.0,
            chroma: 0.12,
        };
        assert_eq!(t.tone(0.0), Color32::BLACK);
        assert_eq!(t.tone(100.0), Color32::WHITE);
        let lum = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
        assert!(lum(t.tone(30.0)) < lum(t.tone(60.0)));
        // Tone is L*: a neutral T50 is the classic 18% grey (#777).
        let g = Tonal {
            hue: 0.0,
            chroma: 0.0,
        }
        .tone(50.0);
        assert!((g.r() as i32 - 0x77).abs() <= 2, "{g:?}");
    }

    #[test]
    fn schemes_have_readable_contrast() {
        for seed in [
            [138, 100, 255],
            [255, 200, 60],
            [60, 130, 210],
            [140, 150, 170],
            [255, 120, 40],
        ] {
            for dark in [true, false] {
                let s = Scheme::from_seed(seed, dark);
                let pairs = [
                    ("on_surface/surface", s.on_surface, s.surface),
                    (
                        "on_surface/container_high",
                        s.on_surface,
                        s.surface_container_high,
                    ),
                    ("on_primary/primary", s.on_primary, s.primary),
                    (
                        "on_primary_container",
                        s.on_primary_container,
                        s.primary_container,
                    ),
                    (
                        "on_secondary_container",
                        s.on_secondary_container,
                        s.secondary_container,
                    ),
                    (
                        "on_tertiary_container",
                        s.on_tertiary_container,
                        s.tertiary_container,
                    ),
                    (
                        "on_error_container",
                        s.on_error_container,
                        s.error_container,
                    ),
                ];
                for (name, fg, bg) in pairs {
                    let c = contrast(fg, bg);
                    assert!(c >= 4.5, "{seed:?} dark={dark} {name}: {c:.2}");
                }
                assert!(contrast(s.on_surface_variant, s.surface_container) >= 3.0);
            }
        }
    }

    #[test]
    fn lobed_shapes_stay_within_radius() {
        let c = pos2(50.0, 50.0);
        for p in lobed_points(c, 20.0, 9, 0.1, 0.3) {
            assert!((p - c).length() <= 20.01);
            assert!((p - c).length() >= 14.0);
        }
    }

    #[test]
    fn wavy_follows_the_path() {
        let pts = wavy(&[pos2(0.0, 0.0), pos2(100.0, 0.0)], 3.0, 20.0);
        assert!(pts.len() > 10);
        assert!(pts
            .iter()
            .all(|p| p.y.abs() <= 3.01 && (-0.01..=100.01).contains(&p.x)));
        assert!(pts.iter().any(|p| p.y > 2.0) && pts.iter().any(|p| p.y < -2.0));
    }
}
