//! 3D radar and terrain scenes for Pro's 3D view.
//!
//! Two kinds of radar volume feed a scene:
//! * **Measured** — a NEXRAD Level II volume scan: every elevation tilt the
//!   radar swept, so each echo is placed at its true height (beam geometry
//!   with the standard 4/3-earth refraction model).
//! * **Estimated** — anywhere else, the 2D radar mosaic extruded into
//!   columns whose height grows with reflectivity. This shows where storms
//!   are and roughly how strong, not their measured structure.
//!
//! Both become [`Voxel`]s on a local east/north/up grid (km) around the
//! scene centre, alongside a terrain height grid.

use crate::cells::DbzGrid;
use crate::geo::{haversine_km, lonlat_to_world};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Earth radius, km.
const EARTH_KM: f64 = 6371.0;
/// Effective-earth factor for standard beam refraction.
const KE: f64 = 4.0 / 3.0;

/// One radial of reflectivity from a Level II sweep.
#[derive(Debug, Clone, PartialEq)]
pub struct Radial {
    pub azimuth_deg: f32,
    /// The antenna's elevation for this radial.
    pub elevation_deg: f32,
    pub first_gate_km: f32,
    pub gate_km: f32,
    /// dBZ per gate; `NaN` = no echo / below threshold.
    pub dbz: Vec<f32>,
}

/// One elevation tilt.
#[derive(Debug, Clone, PartialEq)]
pub struct Sweep {
    pub elevation_deg: f32,
    pub radials: Vec<Radial>,
}

/// A decoded NEXRAD volume scan (reflectivity only).
#[derive(Debug, Clone, PartialEq)]
pub struct Volume {
    pub site: String,
    pub lat: f64,
    pub lon: f64,
    /// Antenna height above sea level, m.
    pub height_m: f64,
    /// Scan time, RFC 3339.
    pub time: String,
    pub sweeps: Vec<Sweep>,
}

/// Height above the radar (km) and ground distance (km) of a beam at
/// `range_km` and `elev_deg`, with 4/3-earth refraction.
pub fn beam(range_km: f64, elev_deg: f64) -> (f64, f64) {
    let a = KE * EARTH_KM;
    let e = elev_deg.to_radians();
    let h = (range_km * range_km + a * a + 2.0 * range_km * a * e.sin()).sqrt() - a;
    let s = a * (range_km * e.cos() / (a + h)).asin();
    (h, s)
}

/// A filled cell of the 3D grid: centre (east, north, up) in km from the
/// scene centre, up measured from sea level.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Voxel {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub dbz: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RadarSource {
    /// A NEXRAD volume scan (measured heights).
    Nexrad { site: String, time: String },
    /// The global mosaic, extruded (estimated heights).
    Mosaic { time: String },
}

/// Terrain heights on a square grid centred on the scene: `n × n` samples
/// `cell_km` apart, row 0 = north; metres above sea level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Terrain {
    pub n: usize,
    pub cell_km: f32,
    pub heights_m: Vec<f32>,
}

impl Terrain {
    /// Height (m) at local (east, north) km, bilinear; 0 outside.
    pub fn height_at(&self, x: f32, y: f32) -> f32 {
        let half = (self.n as f32 - 1.0) * self.cell_km / 2.0;
        let fx = (x + half) / self.cell_km;
        let fy = (half - y) / self.cell_km;
        if fx < 0.0 || fy < 0.0 || fx > (self.n - 1) as f32 || fy > (self.n - 1) as f32 {
            return 0.0;
        }
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.n - 1), (y0 + 1).min(self.n - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let h = |c: usize, r: usize| self.heights_m[r * self.n + c];
        let top = h(x0, y0) * (1.0 - tx) + h(x1, y0) * tx;
        let bot = h(x0, y1) * (1.0 - tx) + h(x1, y1) * tx;
        top * (1.0 - ty) + bot * ty
    }
}

/// Satellite imagery draped over the terrain: a square RGB image covering
/// the scene (north up), `size` pixels a side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Imagery {
    pub size: usize,
    pub rgb: Vec<u8>,
    /// Credit to show with it.
    pub attribution: String,
}

/// Resample web-mercator imagery onto the scene square. `pixel(wx, wy)`
/// returns the RGB at a world pixel of zoom `zoom` (`None` where missing:
/// left dark grey).
pub fn imagery_from_tiles(
    lat0: f64,
    lon0: f64,
    half_km: f32,
    size: usize,
    zoom: u8,
    attribution: &str,
    pixel: impl Fn(f64, f64) -> Option<[u8; 3]>,
) -> Imagery {
    let mut rgb = vec![40u8; size * size * 3];
    let deg_lat = 1.0 / EARTH_KM.to_radians();
    let deg_lon = deg_lat / lat0.to_radians().cos().max(0.05);
    let step = 2.0 * f64::from(half_km) / size as f64;
    for r in 0..size {
        let y = f64::from(half_km) - (r as f64 + 0.5) * step;
        let lat = lat0 + y * deg_lat;
        for c in 0..size {
            let x = -f64::from(half_km) + (c as f64 + 0.5) * step;
            let lon = lon0 + x * deg_lon;
            let (wx, wy) = lonlat_to_world(lon, lat, f64::from(zoom));
            if let Some(px) = pixel(wx, wy) {
                let i = (r * size + c) * 3;
                rgb[i..i + 3].copy_from_slice(&px);
            }
        }
    }
    Imagery {
        size,
        rgb,
        attribution: attribution.to_string(),
    }
}

/// Everything the 3D view draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub lat: f64,
    pub lon: f64,
    /// Half the width of the scene, km.
    pub half_km: f32,
    pub terrain: Option<Terrain>,
    /// Satellite imagery for the terrain (when it loaded).
    #[serde(default)]
    pub imagery: Option<Imagery>,
    pub voxels: Vec<Voxel>,
    /// Voxel size: horizontal and vertical, km.
    pub voxel_km: f32,
    pub voxel_dz_km: f32,
    pub source: Option<RadarSource>,
    /// Where the radar is, local km (NEXRAD only).
    pub radar_xy: Option<(f32, f32)>,
}

/// Local east/north km of (`lat`, `lon`) from the centre (equirectangular;
/// fine over a few hundred km).
pub fn local_km(lat0: f64, lon0: f64, lat: f64, lon: f64) -> (f32, f32) {
    let x = (lon - lon0).to_radians() * EARTH_KM * lat0.to_radians().cos();
    let y = (lat - lat0).to_radians() * EARTH_KM;
    (x as f32, y as f32)
}

/// Accumulates max dBZ per voxel.
struct Grid {
    km: f32,
    dz: f32,
    cells: HashMap<(i16, i16, i16), f32>,
}

impl Grid {
    fn new(km: f32, dz: f32) -> Self {
        Self {
            km,
            dz,
            cells: HashMap::new(),
        }
    }
    fn add(&mut self, x: f32, y: f32, z: f32, dbz: f32) {
        let k = (
            (x / self.km).floor() as i16,
            (y / self.km).floor() as i16,
            (z / self.dz).floor() as i16,
        );
        let e = self.cells.entry(k).or_insert(f32::MIN);
        *e = e.max(dbz);
    }
    /// The voxels, strongest kept when over `max`.
    fn finish(self, max: usize) -> Vec<Voxel> {
        let mut v: Vec<Voxel> = self
            .cells
            .into_iter()
            .map(|((i, j, k), dbz)| Voxel {
                x: (f32::from(i) + 0.5) * self.km,
                y: (f32::from(j) + 0.5) * self.km,
                z: (f32::from(k) + 0.5) * self.dz,
                dbz,
            })
            .collect();
        if v.len() > max {
            v.sort_by(|a, b| b.dbz.total_cmp(&a.dbz));
            v.truncate(max);
        }
        v
    }
}

/// How to grid echoes into voxels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoxelSpec {
    /// Keep echoes within this many km of the centre (east/west, north/south).
    pub half_km: f32,
    pub min_dbz: f32,
    /// Voxel size: horizontal, vertical (km).
    pub km: f32,
    pub dz: f32,
    /// At most this many voxels (the strongest kept).
    pub max: usize,
}

/// Voxels from a measured volume: every gate at or above `spec.min_dbz`,
/// placed by beam geometry and gridded (max dBZ per cell), relative to
/// (`lat0`, `lon0`).
pub fn volume_voxels(vol: &Volume, lat0: f64, lon0: f64, spec: VoxelSpec) -> Vec<Voxel> {
    let VoxelSpec {
        half_km,
        min_dbz,
        km,
        dz,
        max,
    } = spec;
    let (rx, ry) = local_km(lat0, lon0, vol.lat, vol.lon);
    let site_km = (vol.height_m / 1000.0) as f32;
    let mut g = Grid::new(km, dz);
    for sw in &vol.sweeps {
        for r in &sw.radials {
            let (sa, ca) = r.azimuth_deg.to_radians().sin_cos();
            for (i, &d) in r.dbz.iter().enumerate() {
                // NaN (no echo) fails this too.
                if d.is_nan() || d < min_dbz {
                    continue;
                }
                let range = r.first_gate_km + r.gate_km * i as f32;
                let (h, s) = beam(f64::from(range), f64::from(r.elevation_deg));
                let (x, y) = (rx + s as f32 * sa, ry + s as f32 * ca);
                if x.abs() > half_km || y.abs() > half_km {
                    continue;
                }
                g.add(x, y, site_km + h as f32, d);
            }
        }
    }
    g.finish(max)
}

/// Estimated echo-top height (km above ground) for a column of the given
/// maximum reflectivity: light rain stays low, intense cores reach the
/// upper troposphere. A rough climatological relation, for display only.
pub fn estimated_top_km(dbz: f32) -> f32 {
    (2.0 + (dbz - 10.0).max(0.0) * 0.24).clamp(1.5, 15.0)
}

/// Voxels from the 2D mosaic: each echo extruded to [`estimated_top_km`]
/// above the terrain, weakening toward the top.
pub fn mosaic_voxels(
    grid: &DbzGrid,
    lat0: f64,
    lon0: f64,
    spec: VoxelSpec,
    terrain: Option<&Terrain>,
) -> Vec<Voxel> {
    let VoxelSpec {
        half_km,
        min_dbz,
        km,
        dz,
        max,
    } = spec;
    let mut g = Grid::new(km, dz);
    let stride = 1;
    for py in (0..grid.h).step_by(stride) {
        for px in (0..grid.w).step_by(stride) {
            let Some(d) = grid.dbz[py * grid.w + px] else {
                continue;
            };
            if d < min_dbz {
                continue;
            }
            let (lon, lat) = grid.lonlat(px as f64, py as f64);
            let (x, y) = local_km(lat0, lon0, lat, lon);
            if x.abs() > half_km || y.abs() > half_km {
                continue;
            }
            let ground = terrain.map_or(0.0, |t| t.height_at(x, y) / 1000.0);
            let top = estimated_top_km(d);
            let mut z = dz / 2.0;
            while z < top {
                let f = z / top;
                g.add(x, y, ground + z, d - 18.0 * f * f);
                z += dz;
            }
        }
    }
    g.finish(max)
}

/// Terrain from Terrarium-encoded elevation tiles (AWS Terrain Tiles):
/// `height = R·256 + G + B/256 − 32768` m. `pixel(wx, wy)` returns the RGB
/// at a world pixel of zoom `z` (or `None` where no tile loaded).
pub fn terrain_from_tiles(
    lat0: f64,
    lon0: f64,
    half_km: f32,
    n: usize,
    zoom: u8,
    pixel: impl Fn(f64, f64) -> Option<[u8; 3]>,
) -> Terrain {
    let cell = 2.0 * half_km / (n - 1) as f32;
    let mut heights = vec![0.0f32; n * n];
    let deg_lat = 1.0 / (EARTH_KM.to_radians());
    let deg_lon = deg_lat / lat0.to_radians().cos().max(0.05);
    for r in 0..n {
        for c in 0..n {
            let x = -half_km + c as f32 * cell;
            let y = half_km - r as f32 * cell;
            let lat = lat0 + f64::from(y) * deg_lat;
            let lon = lon0 + f64::from(x) * deg_lon;
            let (wx, wy) = lonlat_to_world(lon, lat, f64::from(zoom));
            if let Some([rr, gg, bb]) = pixel(wx, wy) {
                let h = f32::from(rr) * 256.0 + f32::from(gg) + f32::from(bb) / 256.0 - 32768.0;
                // Sea floor reads negative: show the sea surface instead.
                heights[r * n + c] = h.max(0.0);
            }
        }
    }
    Terrain {
        n,
        cell_km: cell,
        heights_m: heights,
    }
}

/// The NEXRAD site to use for a place: the nearest within `max_km`.
pub fn nearest_site<T>(
    lat: f64,
    lon: f64,
    sites: &[T],
    pos: impl Fn(&T) -> (f64, f64),
    max_km: f64,
) -> Option<(&T, f64)> {
    sites
        .iter()
        .map(|s| {
            let (la, lo) = pos(s);
            (s, haversine_km(lat, lon, la, lo))
        })
        .filter(|(_, d)| *d <= max_km)
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beam_heights_match_the_standard_model() {
        // 0.5° at 100 km: about 1.5 km up (0.87 from the tilt + 0.59 curvature).
        let (h, s) = beam(100.0, 0.5);
        assert!((h - 1.46).abs() < 0.05, "{h}");
        assert!((s - 100.0).abs() < 0.5, "{s}");
        // Steeper tilts climb fast.
        let (h, _) = beam(100.0, 10.0);
        assert!(h > 17.0 && h < 19.0, "{h}");
    }

    #[test]
    fn volume_gates_land_where_the_beam_points() {
        // One radial due east at 0.5°, a 50 dBZ gate at 100 km.
        let mut dbz = vec![f32::NAN; 400];
        dbz[399] = 50.0; // first gate 0.25 km, 0.25 km apart → 100 km
        let vol = Volume {
            site: "KTST".into(),
            lat: 35.0,
            lon: -97.0,
            height_m: 400.0,
            time: String::new(),
            sweeps: vec![Sweep {
                elevation_deg: 0.5,
                radials: vec![Radial {
                    azimuth_deg: 90.0,
                    elevation_deg: 0.5,
                    first_gate_km: 0.25,
                    gate_km: 0.25,
                    dbz,
                }],
            }],
        };
        let spec = VoxelSpec {
            half_km: 230.0,
            min_dbz: 10.0,
            km: 2.0,
            dz: 0.5,
            max: 1000,
        };
        let v = volume_voxels(&vol, 35.0, -97.0, spec);
        assert_eq!(v.len(), 1);
        let p = v[0];
        assert!((p.x - 100.0).abs() < 2.0 && p.y.abs() < 2.0, "{p:?}");
        assert!((p.z - 1.86).abs() < 0.5, "{p:?}"); // 0.4 km site + ~1.46 km
        assert_eq!(p.dbz, 50.0);
    }

    #[test]
    fn imagery_is_resampled_north_up() {
        // Red in the northern half of the world tile, blue in the south.
        let (_, cy) = lonlat_to_world(10.0, 45.0, 9.0);
        let img = imagery_from_tiles(45.0, 10.0, 20.0, 8, 9, "test", |_, wy| {
            Some(if wy < cy { [255, 0, 0] } else { [0, 0, 255] })
        });
        assert_eq!(img.rgb.len(), 8 * 8 * 3);
        assert_eq!(&img.rgb[0..3], &[255, 0, 0], "top row is north");
        let last = (8 * 8 - 1) * 3;
        assert_eq!(&img.rgb[last..last + 3], &[0, 0, 255]);
    }

    #[test]
    fn stronger_echoes_reach_higher() {
        assert!(estimated_top_km(55.0) > estimated_top_km(30.0));
        assert!(estimated_top_km(75.0) <= 15.0);
        assert!(estimated_top_km(0.0) >= 1.5);
    }

    #[test]
    fn terrain_decodes_terrarium_and_interpolates() {
        // A flat 1000 m plateau: R·256 + G − 32768 = 1000 → R=131, G=232.
        let t = terrain_from_tiles(40.0, -105.0, 50.0, 11, 8, |_, _| Some([131, 232, 0]));
        assert_eq!(t.n, 11);
        assert!((t.cell_km - 10.0).abs() < 1e-4);
        assert!(t.heights_m.iter().all(|h| (*h - 1000.0).abs() < 0.01));
        assert!((t.height_at(3.0, -7.0) - 1000.0).abs() < 0.01);
        assert_eq!(t.height_at(500.0, 0.0), 0.0);
        // Below sea level reads as the sea surface.
        let sea = terrain_from_tiles(0.0, 0.0, 10.0, 3, 8, |_, _| Some([120, 0, 0]));
        assert!(sea.heights_m.iter().all(|h| *h == 0.0));
    }

    #[test]
    fn mosaic_columns_are_extruded() {
        let zoom = 7u8;
        let (cx, cy) = lonlat_to_world(10.0, 48.0, f64::from(zoom));
        let mut g = DbzGrid::new(40, 40, zoom, cx - 20.0, cy - 20.0);
        g.dbz[20 * 40 + 20] = Some(55.0);
        let spec = VoxelSpec {
            half_km: 100.0,
            min_dbz: 15.0,
            km: 2.0,
            dz: 1.0,
            max: 10_000,
        };
        let v = mosaic_voxels(&g, 48.0, 10.0, spec, None);
        let top = v.iter().map(|p| p.z).fold(0.0, f32::max);
        assert!(top > 8.0 && top <= estimated_top_km(55.0) + 0.5, "{top}");
        // Weaker aloft than at the bottom.
        let low = v.iter().min_by(|a, b| a.z.total_cmp(&b.z)).unwrap();
        let high = v.iter().max_by(|a, b| a.z.total_cmp(&b.z)).unwrap();
        assert!(low.dbz > high.dbz);
    }
}
