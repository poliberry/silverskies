//! Storm cells from a radar mosaic, anywhere there is radar coverage:
//! connected cores of high reflectivity in a dBZ grid, matched between two
//! frames for motion. This is a simple cell tracker in the spirit of the
//! NEXRAD storm-cell identification algorithm, but with only reflectivity
//! to go on: no tornado/mesocyclone signatures or hail estimates.

use crate::geo::{bearing_deg, haversine_km, world_to_lonlat};
use crate::storms::{CellSource, StormCell, Tvs};

/// A reflectivity grid made of Web-Mercator tiles: `dbz[y * w + x]` in dBZ,
/// `None` where there is no echo. `(x0, y0)` is the world pixel of the
/// top-left corner at `zoom`.
#[derive(Debug, Clone)]
pub struct DbzGrid {
    pub w: usize,
    pub h: usize,
    pub zoom: u8,
    pub x0: f64,
    pub y0: f64,
    pub dbz: Vec<Option<f32>>,
}

impl DbzGrid {
    pub fn new(w: usize, h: usize, zoom: u8, x0: f64, y0: f64) -> Self {
        Self {
            w,
            h,
            zoom,
            x0,
            y0,
            dbz: vec![None; w * h],
        }
    }

    /// `(lon, lat)` of the centre of pixel (`x`, `y`).
    pub fn lonlat(&self, x: f64, y: f64) -> (f64, f64) {
        world_to_lonlat(self.x0 + x + 0.5, self.y0 + y + 0.5, f64::from(self.zoom))
    }

    /// Ground size of one pixel at row `y`, km.
    fn pixel_km(&self, y: f64) -> f64 {
        let (_, lat) = self.lonlat(0.0, y);
        40_075.016 * lat.to_radians().cos() / (256.0 * 2f64.powi(i32::from(self.zoom)))
    }
}

/// RainViewer-style greyscale ("colour scheme 0") pixel → dBZ.
pub fn dbz_from_pixel(v: u8, alpha: u8) -> Option<f32> {
    (alpha > 0 && v > 0).then(|| f32::from(v) - 32.0)
}

/// A reflectivity core.
#[derive(Debug, Clone, PartialEq)]
pub struct Core {
    pub lon: f64,
    pub lat: f64,
    pub max_dbz: f32,
    pub area_km2: f64,
}

/// Connected regions (8-neighbour) at or above `threshold` dBZ with at least
/// `min_km2` of area, strongest first. Centroids are reflectivity-weighted.
pub fn detect(grid: &DbzGrid, threshold: f32, min_km2: f64) -> Vec<Core> {
    let (w, h) = (grid.w, grid.h);
    let mut seen = vec![false; w * h];
    let mut cores = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if seen[start] || grid.dbz[start].is_none_or(|d| d < threshold) {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let (mut sx, mut sy, mut sw, mut max, mut area) = (0.0, 0.0, 0.0, f32::MIN, 0.0);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            let d = grid.dbz[i].unwrap_or(threshold);
            // Weight by linear reflectivity so the core, not the fringe, wins.
            let wgt = 10f64.powf(f64::from(d) / 10.0);
            sx += x as f64 * wgt;
            sy += y as f64 * wgt;
            sw += wgt;
            max = max.max(d);
            area += grid.pixel_km(y as f64).powi(2);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if !seen[j] && grid.dbz[j].is_some_and(|d| d >= threshold) {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        if area >= min_km2 {
            let (lon, lat) = grid.lonlat(sx / sw, sy / sw);
            cores.push(Core {
                lon,
                lat,
                max_dbz: max,
                area_km2: area,
            });
        }
    }
    cores.sort_by(|a, b| b.max_dbz.total_cmp(&a.max_dbz));
    cores
}

/// Match each current core to the nearest unclaimed previous core that it
/// could have moved from in `minutes` (at most `max_kt`), strongest first.
/// Returns, per current core, its motion as (from°, kt) when matched.
pub fn track(prev: &[Core], cur: &[Core], minutes: f64, max_kt: f64) -> Vec<Option<(f64, f64)>> {
    let max_km = max_kt * 1.852 * minutes / 60.0;
    let mut used = vec![false; prev.len()];
    cur.iter()
        .map(|c| {
            let best = prev
                .iter()
                .enumerate()
                .filter(|(i, _)| !used[*i])
                .map(|(i, p)| (i, haversine_km(p.lat, p.lon, c.lat, c.lon)))
                .filter(|(_, d)| *d <= max_km)
                .min_by(|a, b| a.1.total_cmp(&b.1))?;
            used[best.0] = true;
            let p = &prev[best.0];
            let kt = best.1 / 1.852 / (minutes / 60.0);
            let heading = bearing_deg(p.lat, p.lon, c.lat, c.lon);
            Some(((heading + 180.0).rem_euclid(360.0), kt))
        })
        .collect()
}

/// Storm cells (in the shared [`StormCell`] shape) from two consecutive
/// mosaic grids `minutes` apart.
pub fn mosaic_cells(prev: &DbzGrid, cur: &DbzGrid, minutes: f64, valid: &str) -> Vec<StormCell> {
    const THRESHOLD: f32 = 40.0;
    const MIN_KM2: f64 = 20.0;
    let a = detect(prev, THRESHOLD, MIN_KM2);
    let b = detect(cur, THRESHOLD, MIN_KM2);
    let motion = track(&a, &b, minutes.max(1.0), 70.0);
    b.iter()
        .zip(motion)
        .enumerate()
        .map(|(i, (c, m))| {
            let (from, kt) = m.unwrap_or((0.0, 0.0));
            StormCell {
                radar: "MOSAIC".into(),
                id: format!("M{}", i + 1),
                lon: c.lon,
                lat: c.lat,
                tvs: Tvs::None,
                meso: 0,
                posh: 0.0,
                poh: 0.0,
                max_hail_in: 0.0,
                vil: 0.0,
                max_dbz: f64::from(c.max_dbz),
                max_dbz_height_kft: 0.0,
                top_kft: 0.0,
                motion_from_deg: from,
                speed_kt: kt,
                valid: valid.to_string(),
                source: CellSource::Mosaic,
                area_km2: c.area_km2,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::lonlat_to_world;

    /// A grid around (lon, lat) at zoom 7 with circular cores painted in.
    fn grid_with(cores: &[(f64, f64, f32, f64)]) -> DbzGrid {
        let zoom = 7u8;
        let (cx, cy) = lonlat_to_world(10.0, 48.0, f64::from(zoom));
        let (w, h) = (300usize, 300usize);
        let mut g = DbzGrid::new(w, h, zoom, cx - 150.0, cy - 150.0);
        for (lon, lat, dbz, r_px) in cores {
            let (px, py) = lonlat_to_world(*lon, *lat, f64::from(zoom));
            let (px, py) = (px - g.x0, py - g.y0);
            for y in 0..h {
                for x in 0..w {
                    let d = ((x as f64 - px).powi(2) + (y as f64 - py).powi(2)).sqrt();
                    if d <= *r_px {
                        // Stronger toward the middle; light rain around it.
                        g.dbz[y * w + x] = Some(dbz - (d / r_px * 12.0) as f32);
                    } else if d <= r_px * 2.0 && g.dbz[y * w + x].is_none() {
                        g.dbz[y * w + x] = Some(20.0);
                    }
                }
            }
        }
        g
    }

    #[test]
    fn pixels_decode_to_dbz() {
        assert_eq!(dbz_from_pixel(78, 255), Some(46.0));
        assert_eq!(dbz_from_pixel(0, 255), None);
        assert_eq!(dbz_from_pixel(90, 0), None);
    }

    #[test]
    fn finds_cores_and_ignores_light_rain() {
        let g = grid_with(&[(10.0, 48.0, 58.0, 8.0), (10.6, 48.3, 50.0, 8.0)]);
        let cores = detect(&g, 40.0, 20.0);
        assert_eq!(cores.len(), 2, "{cores:?}");
        assert!(cores[0].max_dbz >= 57.0);
        assert!((cores[0].lon - 10.0).abs() < 0.05 && (cores[0].lat - 48.0).abs() < 0.05);
        // 8 px radius at zoom 7 near 48°N ≈ 0.8 km/px → ~130 km² (above threshold part).
        assert!(
            cores[0].area_km2 > 30.0 && cores[0].area_km2 < 400.0,
            "{}",
            cores[0].area_km2
        );
    }

    #[test]
    fn tracks_motion_between_frames() {
        // Moved 0.2° east in 10 minutes at 48°N ≈ 14.9 km → ~48 kt, from the west.
        let a = grid_with(&[(10.0, 48.0, 55.0, 6.0)]);
        let b = grid_with(&[(10.2, 48.0, 55.0, 6.0)]);
        let cells = mosaic_cells(&a, &b, 10.0, "2026-06-01T12:00:00Z");
        assert_eq!(cells.len(), 1);
        let c = &cells[0];
        assert!(
            (c.motion_from_deg - 270.0).abs() < 5.0,
            "{}",
            c.motion_from_deg
        );
        assert!((c.speed_kt - 48.0).abs() < 6.0, "{}", c.speed_kt);
        assert_eq!(c.source, CellSource::Mosaic);
        assert_eq!(c.tvs, Tvs::None);
    }

    #[test]
    fn far_jumps_are_not_matched() {
        let a = detect(&grid_with(&[(10.0, 48.0, 55.0, 6.0)]), 40.0, 20.0);
        let b = detect(&grid_with(&[(11.5, 48.0, 55.0, 6.0)]), 40.0, 20.0);
        // ~112 km in 10 min would be ~360 kt: a different storm, no motion.
        assert_eq!(track(&a, &b, 10.0, 70.0), vec![None]);
    }
}
