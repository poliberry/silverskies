//! The animated sky behind the temperature card: a gradient for the
//! condition and time of day, with drifting clouds, falling rain or snow,
//! fog banks, twinkling stars, sun or moon glow and lightning flashes.
//! Everything is drawn from the clock (no state), deterministic per index.

use egui::epaint::RectShape;
use egui::{pos2, vec2, Color32, ColorImage, Painter, Pos2, Rect, Shape, Stroke, TextureHandle};

/// What the sky shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Clear,
    PartlyCloudy,
    Overcast,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Thunder,
}

/// The sky for a WMO weather code.
pub fn kind(code: i32) -> Kind {
    match code {
        0 | 1 => Kind::Clear,
        2 => Kind::PartlyCloudy,
        3 => Kind::Overcast,
        45 | 48 => Kind::Fog,
        51..=57 => Kind::Drizzle,
        61..=67 | 80..=82 => Kind::Rain,
        71..=77 | 85 | 86 => Kind::Snow,
        95..=99 => Kind::Thunder,
        _ => Kind::Overcast,
    }
}

/// Top and bottom colours of the gradient.
pub fn gradient(k: Kind, day: bool) -> ([u8; 3], [u8; 3]) {
    match (k, day) {
        (Kind::Clear, true) => ([0x2f, 0x7f, 0xe0], [0x8c, 0xc8, 0xff]),
        (Kind::Clear, false) => ([0x0b, 0x15, 0x30], [0x26, 0x3a, 0x6b]),
        (Kind::PartlyCloudy, true) => ([0x3d, 0x82, 0xd6], [0xa5, 0xc8, 0xee]),
        (Kind::PartlyCloudy, false) => ([0x10, 0x1a, 0x35], [0x2c, 0x3d, 0x63]),
        (Kind::Overcast, true) => ([0x5f, 0x6f, 0x83], [0x9f, 0xab, 0xb8]),
        (Kind::Overcast, false) => ([0x1e, 0x24, 0x30], [0x3a, 0x43, 0x52]),
        (Kind::Fog, true) => ([0x7e, 0x8a, 0x96], [0xb9, 0xc0, 0xc6]),
        (Kind::Fog, false) => ([0x2a, 0x2f, 0x37], [0x4b, 0x52, 0x5c]),
        (Kind::Drizzle | Kind::Rain, true) => ([0x3f, 0x4f, 0x65], [0x72, 0x83, 0x98]),
        (Kind::Drizzle | Kind::Rain, false) => ([0x14, 0x1a, 0x24], [0x2d, 0x37, 0x47]),
        (Kind::Snow, true) => ([0x6f, 0x89, 0xa6], [0xb8, 0xc8, 0xd8]),
        (Kind::Snow, false) => ([0x1f, 0x2a, 0x3a], [0x4a, 0x5b, 0x72]),
        (Kind::Thunder, true) => ([0x2a, 0x2f, 0x3d], [0x55, 0x5a, 0x6e]),
        (Kind::Thunder, false) => ([0x10, 0x12, 0x1a], [0x2b, 0x2e, 0x3d]),
    }
}

/// A repeatable pseudo-random number in 0..1 for (index, salt).
fn h(i: usize, salt: u32) -> f32 {
    let mut x = (i as u32).wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Is `p` inside the rounded rectangle?
fn inside(p: Pos2, r: Rect, radius: f32) -> bool {
    if !r.contains(p) {
        return false;
    }
    let cx = p.x.clamp(r.left() + radius, r.right() - radius);
    let cy = p.y.clamp(r.top() + radius, r.bottom() - radius);
    p.distance(pos2(cx, cy)) <= radius
}

fn white(a: f32) -> Color32 {
    Color32::from_white_alpha((a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// The gradient as a small texture, cached per colour pair.
fn gradient_texture(ctx: &egui::Context, top: [u8; 3], bottom: [u8; 3]) -> TextureHandle {
    let id = egui::Id::unique(("sky_gradient", top, bottom));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return t;
    }
    let n = 64;
    let px: Vec<Color32> = (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32;
            let m = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
            Color32::from_rgb(
                m(top[0], bottom[0]),
                m(top[1], bottom[1]),
                m(top[2], bottom[2]),
            )
        })
        .collect();
    let tex = ctx.load_texture(
        format!("sky-{top:?}-{bottom:?}"),
        ColorImage::new([1, n], px),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// Paint the sky into `rect` (rounded by `radius`) at time `t` seconds.
/// With `animate` off it's a still frame.
pub fn paint(
    ctx: &egui::Context,
    painter: &Painter,
    rect: Rect,
    radius: f32,
    code: i32,
    day: bool,
    t: f64,
) {
    let k = kind(code);
    let (top, bottom) = gradient(k, day);
    let tex = gradient_texture(ctx, top, bottom);
    painter.add(Shape::Rect(
        RectShape::filled(rect, radius, Color32::WHITE)
            .with_texture(tex.id(), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))),
    ));
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let t = t as f32;
    let (w, hgt) = (rect.width(), rect.height());

    // Stars on clear and partly cloudy nights.
    if !day && matches!(k, Kind::Clear | Kind::PartlyCloudy) {
        for i in 0..46 {
            let p = rect.min + vec2(h(i, 1) * w, h(i, 2) * hgt * 0.8);
            if !inside(p, rect, radius) {
                continue;
            }
            let tw = 0.35 + 0.65 * (t * (0.6 + h(i, 3) * 1.6) + h(i, 4) * 6.3).sin().abs();
            painter.circle_filled(p, 0.7 + h(i, 5) * 1.1, white(0.85 * tw));
        }
    }
    // Sun or moon glow, high up between the text and the icon.
    if matches!(k, Kind::Clear | Kind::PartlyCloudy) {
        let c = rect.min + vec2(w * 0.56, hgt * 0.16);
        if day {
            for (r, a) in [(170.0, 0.05), (115.0, 0.08), (70.0, 0.12), (34.0, 0.35)] {
                painter.circle_filled(
                    c,
                    r,
                    Color32::from_rgba_unmultiplied(255, 236, 170, (a * 255.0) as u8),
                );
            }
            // Slowly turning rays.
            for i in 0..12 {
                let a = t * 0.05 + i as f32 * std::f32::consts::TAU / 12.0;
                let d = vec2(a.cos(), a.sin());
                painter.line_segment(
                    [
                        c + d * 44.0,
                        c + d * (90.0 + 18.0 * (t * 0.8 + i as f32).sin()),
                    ],
                    Stroke::new(3.0, Color32::from_rgba_unmultiplied(255, 240, 190, 30)),
                );
            }
        } else {
            for (r, a) in [(80.0, 0.05), (44.0, 0.09)] {
                painter.circle_filled(c, r, white(a));
            }
            painter.circle_filled(c, 18.0, Color32::from_rgb(0xe8, 0xec, 0xf4));
            painter.circle_filled(
                c + vec2(7.0, -5.0),
                16.0,
                Color32::from_rgb(top[0], top[1], top[2]),
            );
        }
    }
    // Clouds drifting right.
    let (clouds, cloud_a, dark) = match k {
        Kind::PartlyCloudy => (3, 0.55, false),
        Kind::Overcast | Kind::Drizzle => (6, 0.4, false),
        Kind::Rain | Kind::Snow => (6, 0.35, !day),
        Kind::Thunder => (7, 0.45, true),
        _ => (0, 0.0, false),
    };
    for i in 0..clouds {
        let size = hgt * (0.09 + h(i, 11) * 0.06);
        let span = w + size * 6.0;
        let x = rect.left() - size * 3.0 + (h(i, 12) * span + t * (6.0 + h(i, 13) * 10.0)) % span;
        let y = rect.top() + hgt * (0.2 + h(i, 14) * 0.35);
        // Opaque, pre-blended with the sky behind it, so the puffs merge
        // into one cloud instead of showing their overlaps.
        let f = ((y - rect.top()) / hgt).clamp(0.0, 1.0);
        let sky = |i: usize| f32::from(top[i]) + (f32::from(bottom[i]) - f32::from(top[i])) * f;
        let tint: [f32; 3] = if dark {
            [40.0, 44.0, 58.0]
        } else {
            [255.0, 255.0, 255.0]
        };
        let mix = |i: usize| (sky(i) + (tint[i] - sky(i)) * cloud_a).round() as u8;
        let col = Color32::from_rgb(mix(0), mix(1), mix(2));
        for (dx, dy, s) in [
            (-1.3, 0.25, 0.7),
            (-0.5, -0.15, 1.0),
            (0.4, -0.3, 0.9),
            (1.2, 0.2, 0.75),
            (0.0, 0.35, 0.95),
        ] {
            painter.circle_filled(pos2(x + dx * size, y + dy * size), size * s, col);
        }
    }
    // Fog banks.
    if k == Kind::Fog {
        for i in 0..5 {
            let bw = w * (0.6 + h(i, 21) * 0.5);
            let span = w + bw;
            let x = rect.left() - bw + (h(i, 22) * span + t * (8.0 + h(i, 23) * 8.0)) % span;
            let y = rect.top() + hgt * (0.25 + i as f32 * 0.14);
            let band = Rect::from_min_size(pos2(x, y), vec2(bw, hgt * 0.1));
            painter.rect_filled(band, hgt * 0.05, white(0.14 + h(i, 24) * 0.1));
        }
    }
    // Rain and drizzle: slanted streaks.
    let drops = match k {
        Kind::Drizzle => 40,
        Kind::Rain => 80,
        Kind::Thunder => 90,
        _ => 0,
    };
    for i in 0..drops {
        let speed = hgt * (1.2 + h(i, 31) * 0.8);
        let len = if k == Kind::Drizzle {
            6.0
        } else {
            11.0 + h(i, 32) * 7.0
        };
        let y = rect.top() + (h(i, 33) * hgt + t * speed) % (hgt + len) - len;
        let x = rect.left() + (h(i, 34) * w + (y - rect.top()) * 0.18) % w;
        let p = pos2(x, y);
        if !inside(p, rect, radius) || !inside(p + vec2(len * 0.18, len), rect, radius) {
            continue;
        }
        painter.line_segment(
            [p, p + vec2(len * 0.18, len)],
            Stroke::new(1.2, white(0.35 + h(i, 35) * 0.25)),
        );
    }
    // Snow: flakes falling and swaying.
    if k == Kind::Snow {
        for i in 0..70 {
            let speed = 18.0 + h(i, 41) * 26.0;
            let y = rect.top() + (h(i, 42) * hgt + t * speed) % (hgt + 6.0) - 3.0;
            let x =
                rect.left() + h(i, 43) * w + (t * (0.6 + h(i, 44)) + h(i, 45) * 6.0).sin() * 8.0;
            let p = pos2(x, y);
            if inside(p, rect, radius) {
                painter.circle_filled(p, 1.2 + h(i, 46) * 1.8, white(0.7 + h(i, 47) * 0.3));
            }
        }
    }
    // Lightning: a flash and a bolt every few seconds.
    if k == Kind::Thunder {
        let period = 5.5;
        let n = (t / period).floor() as usize;
        let since = t - n as f32 * period - h(n, 51) * 2.0;
        if (0.0..0.35).contains(&since) {
            let fade = 1.0 - since / 0.35;
            painter.rect_filled(rect, radius, white(0.22 * fade));
            let mut p = pos2(
                rect.left() + w * (0.25 + h(n, 52) * 0.4),
                rect.top() + hgt * 0.2,
            );
            let mut pts = vec![p];
            for j in 0..6 {
                p += vec2((h(n * 7 + j, 53) - 0.5) * 30.0, hgt * 0.1);
                pts.push(p);
            }
            painter.add(Shape::line(pts, Stroke::new(2.0, white(0.85 * fade))));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_map_to_skies() {
        assert_eq!(kind(0), Kind::Clear);
        assert_eq!(kind(2), Kind::PartlyCloudy);
        assert_eq!(kind(48), Kind::Fog);
        assert_eq!(kind(53), Kind::Drizzle);
        assert_eq!(kind(81), Kind::Rain);
        assert_eq!(kind(86), Kind::Snow);
        assert_eq!(kind(96), Kind::Thunder);
    }

    #[test]
    fn randoms_are_stable_and_in_range() {
        for i in 0..1000 {
            let v = h(i, 7);
            assert!((0.0..1.0).contains(&v));
            assert_eq!(v, h(i, 7));
        }
        assert_ne!(h(1, 1), h(2, 1));
    }

    #[test]
    fn rounded_corners_exclude_the_corner() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        assert!(inside(pos2(50.0, 50.0), r, 20.0));
        assert!(!inside(pos2(1.0, 1.0), r, 20.0));
        assert!(inside(pos2(20.0, 1.0), r, 20.0));
    }
}
