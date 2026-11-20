//! MRMS hail swaths and rotation tracks as map images (US): the grid is
//! cropped to where there's anything to show, coloured, and laid out in
//! Web Mercator rows for the map's [`ss_map::GeoImageLayer`].

use egui::{Color32, ColorImage};
use ss_net::mrms::{Grid, TrackKind};

/// A track ready to draw, with its (cropped) grid for hover readouts.
pub struct Track {
    pub kind: TrackKind,
    pub minutes: u32,
    pub grid: Grid,
    pub image: ColorImage,
    /// West, south, east, north.
    pub bounds: [f64; 4],
}

/// Grid reduction per product: MESH is 0.01°, rotation tracks 0.005°;
/// both end up ~2 km.
pub fn factor(kind: TrackKind) -> usize {
    match kind {
        TrackKind::Hail => 2,
        TrackKind::Rotation => 4,
        TrackKind::Swath => 1,
    }
}

/// Smallest value drawn (hail mm, rotation 10⁻³ s⁻¹): 0.5 in hail, and
/// rotation above the background noise.
pub fn threshold(kind: TrackKind) -> f32 {
    match kind {
        TrackKind::Hail => 12.7,
        TrackKind::Rotation => 8.0,
        // Cores this strong usually carry hail.
        TrackKind::Swath => 50.0,
    }
}

const HAIL_STOPS: [(f32, [u8; 3]); 7] = [
    (12.7, [0, 200, 80]),     // 0.5 in
    (19.05, [180, 230, 0]),   // 0.75 in
    (25.4, [255, 220, 0]),    // 1 in
    (38.1, [255, 140, 0]),    // 1.5 in
    (50.8, [240, 30, 0]),     // 2 in
    (69.85, [170, 0, 60]),    // 2.75 in
    (101.6, [255, 255, 255]), // 4 in
];

const ROTATION_STOPS: [(f32, [u8; 3]); 6] = [
    (8.0, [170, 150, 255]),
    (10.0, [140, 80, 255]),
    (13.0, [200, 0, 255]),
    (17.0, [255, 0, 170]),
    (22.0, [255, 70, 110]),
    (30.0, [255, 255, 255]),
];

/// Storm swaths (max composite dBZ, worldwide), in the hail swath colours.
const SWATH_STOPS: [(f32, [u8; 3]); 5] = [
    (50.0, [0, 200, 80]),
    (55.0, [255, 220, 0]),
    (60.0, [255, 140, 0]),
    (65.0, [240, 30, 0]),
    (70.0, [200, 0, 200]),
];

pub fn stops(kind: TrackKind) -> &'static [(f32, [u8; 3])] {
    match kind {
        TrackKind::Hail => &HAIL_STOPS,
        TrackKind::Rotation => &ROTATION_STOPS,
        TrackKind::Swath => &SWATH_STOPS,
    }
}

/// The colour for a value (stepped), or transparent below the threshold.
pub fn color(kind: TrackKind, v: f32) -> Color32 {
    match stops(kind).iter().rev().find(|(t, _)| v >= *t) {
        Some((_, c)) => Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 215),
        None => Color32::TRANSPARENT,
    }
}

/// Clear cells at or above the threshold with fewer than two such
/// neighbours: lone pixels are noise or clutter, real tracks are swaths.
fn despeckle(grid: &mut Grid, thr: f32) {
    let (nx, ny) = (grid.nx, grid.ny);
    let on = |v: f32| v >= thr;
    let lone: Vec<usize> = (0..nx * ny)
        .filter(|&i| on(grid.values[i]))
        .filter(|&i| {
            let (r, c) = ((i / nx) as isize, (i % nx) as isize);
            let mut n = 0;
            for dr in -1..=1 {
                for dc in -1..=1 {
                    let (rr, cc) = (r + dr, c + dc);
                    if (dr, dc) != (0, 0)
                        && rr >= 0
                        && cc >= 0
                        && (rr as usize) < ny
                        && (cc as usize) < nx
                        && on(grid.values[rr as usize * nx + cc as usize])
                    {
                        n += 1;
                    }
                }
            }
            n < 2
        })
        .collect();
    for i in lone {
        grid.values[i] = f32::NAN;
    }
}

/// A value as people read it: hail in inches, rotation in s⁻¹.
pub fn describe(kind: TrackKind, v: f32) -> String {
    match kind {
        TrackKind::Hail => format!("hail up to {:.2} in ({v:.0} mm)", v / 25.4),
        TrackKind::Rotation => format!("rotation {:.3} s⁻¹", v / 1000.0),
        TrackKind::Swath => {
            let what = match v {
                v if v >= 60.0 => "large hail possible",
                v if v >= 55.0 => "hail likely",
                _ => "intense core, hail possible",
            };
            format!("strongest echo {v:.0} dBZ ({what})")
        }
    }
}

/// A dark edge around the painted areas, so tracks stand out over the
/// radar colours beneath.
fn outline(image: &mut ColorImage) {
    let [w, h] = image.size;
    let edge = Color32::from_black_alpha(200);
    let filled: Vec<bool> = image.pixels.iter().map(|c| c.a() > 0).collect();
    for y in 0..h {
        for x in 0..w {
            if filled[y * w + x] {
                continue;
            }
            let touches = [(0isize, 1isize), (0, -1), (1, 0), (-1, 0)]
                .iter()
                .any(|&(dx, dy)| {
                    let (nx, ny) = (x as isize + dx, y as isize + dy);
                    nx >= 0
                        && ny >= 0
                        && (nx as usize) < w
                        && (ny as usize) < h
                        && filled[ny as usize * w + nx as usize]
                });
            if touches {
                image.pixels[y * w + x] = edge;
            }
        }
    }
}

/// Crop to the cells at or above the threshold (plus a margin) and paint
/// them; None when there's nothing to show.
pub fn render(mut grid: Grid, kind: TrackKind, minutes: u32) -> Option<Track> {
    let thr = threshold(kind);
    despeckle(&mut grid, thr);
    let (mut r0, mut r1, mut c0, mut c1) = (usize::MAX, 0, usize::MAX, 0);
    for (i, v) in grid.values.iter().enumerate() {
        if *v >= thr {
            let (r, c) = (i / grid.nx, i % grid.nx);
            r0 = r0.min(r);
            r1 = r1.max(r);
            c0 = c0.min(c);
            c1 = c1.max(c);
        }
    }
    if r0 > r1 {
        return None;
    }
    let pad = 4;
    let (r0, c0) = (r0.saturating_sub(pad), c0.saturating_sub(pad));
    let (r1, c1) = ((r1 + pad).min(grid.ny - 1), (c1 + pad).min(grid.nx - 1));
    let (nx, ny) = (c1 - c0 + 1, r1 - r0 + 1);
    let mut values = Vec::with_capacity(nx * ny);
    for r in r0..=r1 {
        values.extend_from_slice(&grid.values[r * grid.nx + c0..=r * grid.nx + c1]);
    }
    let grid = Grid {
        lat0: grid.lat0 - r0 as f64 * grid.dlat,
        lon0: grid.lon0 + c0 as f64 * grid.dlon,
        nx,
        ny,
        values,
        ..grid
    };
    let bounds = grid.bounds();
    let [_, south, _, north] = bounds;
    // Rows evenly spaced in mercator: the same width, as many rows as the
    // mercator height needs at this column spacing.
    let merc = |lat: f64| {
        (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0)
            .tan()
            .ln()
    };
    let (my0, my1) = (merc(north), merc(south));
    let rad_per_col = grid.dlon.to_radians();
    let h = (((my0 - my1) / rad_per_col).round() as usize).clamp(1, 8192);
    let w = nx.min(8192);
    let mut image = ColorImage::filled([w, h], Color32::TRANSPARENT);
    for y in 0..h {
        let my = my0 + (my1 - my0) * (y as f64 + 0.5) / h as f64;
        let lat = (2.0 * my.exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees();
        let r = ((grid.lat0 - lat) / grid.dlat).round();
        if r < 0.0 || r >= ny as f64 {
            continue;
        }
        let row = &grid.values[r as usize * nx..][..nx];
        for (x, v) in row.iter().take(w).enumerate() {
            if *v >= thr {
                image.pixels[y * w + x] = color(kind, *v);
            }
        }
    }
    outline(&mut image);
    Some(Track {
        kind,
        minutes,
        grid,
        image,
        bounds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        let mut values = vec![f32::NAN; 100 * 50];
        // A 3×3 swath of ~1.2 in hail, and a lone 2 in pixel (noise).
        for r in 19..=21 {
            for c in 29..=31 {
                values[r * 100 + c] = 30.0;
            }
        }
        values[40 * 100 + 80] = 50.0;
        Grid {
            lat0: 40.0,
            lon0: -100.0,
            dlat: 0.02,
            dlon: 0.02,
            nx: 100,
            ny: 50,
            values,
            time: 0,
        }
    }

    #[test]
    fn crops_to_the_track_and_colours_it() {
        let t = render(grid(), TrackKind::Hail, 60).unwrap();
        assert_eq!((t.grid.nx, t.grid.ny), (3 + 2 * 4, 3 + 2 * 4));
        let [w, s, e, n] = t.bounds;
        assert!(w < -99.4 && e > -99.4 && s < 39.6 && n > 39.6);
        assert_eq!(t.grid.at(39.6, -99.4), Some(30.0));
        let painted = t.image.pixels.iter().filter(|c| c.a() > 0).count();
        assert!(painted >= 1, "{painted}");
        assert_eq!(color(TrackKind::Hail, 13.0).a(), 215);
        assert_eq!(color(TrackKind::Hail, 8.0), Color32::TRANSPARENT);
    }

    #[test]
    fn nothing_to_show_is_none() {
        let mut g = grid();
        g.values.iter_mut().for_each(|v| *v = 1.0);
        assert!(render(g, TrackKind::Hail, 60).is_none());
    }

    #[test]
    fn describes_values() {
        assert_eq!(
            describe(TrackKind::Hail, 44.45),
            "hail up to 1.75 in (44 mm)"
        );
        assert_eq!(describe(TrackKind::Rotation, 12.0), "rotation 0.012 s⁻¹");
    }
}
