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
    /// The other moments this radial carries (velocity, dual-pol), encoded.
    pub moments: Vec<MomentData>,
}

/// A Level II moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Moment {
    /// Reflectivity, dBZ.
    #[default]
    Ref,
    /// Radial velocity, m/s (positive away from the radar).
    Vel,
    /// Differential reflectivity, dB.
    Zdr,
    /// Correlation coefficient (ρhv), 0–1.
    Rho,
}

impl Moment {
    pub const ALL: [Moment; 4] = [Moment::Ref, Moment::Vel, Moment::Zdr, Moment::Rho];

    /// The range of values kept, mapped onto bytes 1–255 (0 = no data).
    pub fn range(self) -> (f32, f32) {
        match self {
            Moment::Ref => (-31.5, 95.5),
            Moment::Vel => (-64.0, 64.0),
            Moment::Zdr => (-8.0, 8.0),
            Moment::Rho => (0.2, 1.05),
        }
    }

    /// Encode a value as a byte (never 0).
    pub fn encode(self, v: f32) -> u8 {
        match self {
            // Half-dB steps, as the radar sends them.
            Moment::Ref => ((v + 32.0) * 2.0).round().clamp(1.0, 255.0) as u8,
            _ => {
                let (lo, hi) = self.range();
                (1.0 + (v - lo) / (hi - lo) * 254.0)
                    .round()
                    .clamp(1.0, 255.0) as u8
            }
        }
    }

    /// The value of a byte (`None` for 0, no data).
    pub fn decode(self, b: u8) -> Option<f32> {
        if b == 0 {
            return None;
        }
        Some(match self {
            Moment::Ref => f32::from(b) / 2.0 - 32.0,
            _ => {
                let (lo, hi) = self.range();
                lo + (f32::from(b) - 1.0) / 254.0 * (hi - lo)
            }
        })
    }
}

/// A Level II product to display (reflectivity, velocity, dual-pol).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum L2Product {
    #[default]
    Reflectivity,
    Velocity,
    StormRelative,
    Zdr,
    Cc,
}

impl L2Product {
    pub const ALL: [L2Product; 5] = [
        L2Product::Reflectivity,
        L2Product::Velocity,
        L2Product::StormRelative,
        L2Product::Cc,
        L2Product::Zdr,
    ];

    pub fn moment(self) -> Moment {
        match self {
            L2Product::Reflectivity => Moment::Ref,
            L2Product::Velocity | L2Product::StormRelative => Moment::Vel,
            L2Product::Zdr => Moment::Zdr,
            L2Product::Cc => Moment::Rho,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            L2Product::Reflectivity => "Reflectivity",
            L2Product::Velocity => "Velocity",
            L2Product::StormRelative => "Storm-relative velocity",
            L2Product::Zdr => "Differential reflectivity (ZDR)",
            L2Product::Cc => "Correlation coefficient (CC)",
        }
    }

    /// The colour scale: (value, RGB) stops.
    pub fn stops(self) -> &'static [(f32, [u8; 3])] {
        match self {
            L2Product::Reflectivity => &[
                (10.0, [0x40, 0x9c, 0xd8]),
                (20.0, [0x2e, 0xb8, 0x4a]),
                (30.0, [0x1f, 0x8f, 0x2a]),
                (35.0, [0xd8, 0xd8, 0x20]),
                (40.0, [0xf0, 0xb4, 0x00]),
                (45.0, [0xf0, 0x78, 0x00]),
                (50.0, [0xe8, 0x14, 0x14]),
                (55.0, [0xb0, 0x00, 0x00]),
                (60.0, [0xe0, 0x20, 0xe0]),
                (68.0, [0xff, 0xff, 0xff]),
            ],
            L2Product::Velocity | L2Product::StormRelative => &[
                (-64.0, [0x9c, 0xff, 0xb0]),
                (-40.0, [0x00, 0xe0, 0x50]),
                (-20.0, [0x00, 0x96, 0x2e]),
                (-5.0, [0x1d, 0x4f, 0x2a]),
                (0.0, [0x6a, 0x6a, 0x6a]),
                (5.0, [0x5a, 0x1d, 0x1d]),
                (20.0, [0xb0, 0x14, 0x14]),
                (40.0, [0xff, 0x30, 0x30]),
                (64.0, [0xff, 0xb4, 0xd2]),
            ],
            L2Product::Zdr => &[
                (-4.0, [0x50, 0x50, 0x50]),
                (-1.0, [0x9a, 0x9a, 0x9a]),
                (0.0, [0xc8, 0xc8, 0xc8]),
                (0.5, [0x3c, 0x3c, 0xc8]),
                (1.0, [0x3c, 0xb4, 0xdc]),
                (1.5, [0x3c, 0xc8, 0x5a]),
                (2.0, [0xe6, 0xe6, 0x3c]),
                (3.0, [0xf0, 0x96, 0x28]),
                (4.0, [0xe6, 0x32, 0x32]),
                (6.0, [0xe6, 0x3c, 0xdc]),
                (8.0, [0xff, 0xff, 0xff]),
            ],
            L2Product::Cc => &[
                (0.2, [0x14, 0x14, 0x50]),
                (0.45, [0x28, 0x28, 0xa0]),
                (0.65, [0x3c, 0x78, 0xdc]),
                (0.8, [0x28, 0xc8, 0x78]),
                (0.9, [0xe6, 0xdc, 0x28]),
                (0.95, [0xf0, 0x8c, 0x1e]),
                (0.98, [0xdc, 0x28, 0x28]),
                (1.0, [0xc8, 0x3c, 0xc8]),
                (1.05, [0xff, 0xbe, 0xff]),
            ],
        }
    }

    /// Short name for compact labels.
    pub fn short(self) -> &'static str {
        match self {
            L2Product::Reflectivity => "REF",
            L2Product::Velocity => "VEL",
            L2Product::StormRelative => "SRV",
            L2Product::Zdr => "ZDR",
            L2Product::Cc => "CC",
        }
    }

    /// Colour of a value on this product's scale.
    pub fn color(self, v: f32) -> [u8; 3] {
        let st = self.stops();
        let mut c = st[0].1;
        for w in st.windows(2) {
            if v >= w[0].0 && v < w[1].0 {
                let t = (v - w[0].0) / (w[1].0 - w[0].0);
                let l = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8;
                return [
                    l(w[0].1[0], w[1].1[0]),
                    l(w[0].1[1], w[1].1[1]),
                    l(w[0].1[2], w[1].1[2]),
                ];
            }
            if v >= w[1].0 {
                c = w[1].1;
            }
        }
        c
    }
}

/// One moment of a radial: gate geometry and encoded values
/// ([`Moment::encode`]; 0 = no data).
#[derive(Debug, Clone, PartialEq)]
pub struct MomentData {
    pub moment: Moment,
    pub first_gate_km: f32,
    pub gate_km: f32,
    pub values: Vec<u8>,
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

/// Everything the 3D view draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub lat: f64,
    pub lon: f64,
    /// Half the width of the scene, km.
    pub half_km: f32,
    pub terrain: Option<Terrain>,
    pub voxels: Vec<Voxel>,
    /// Voxel size: horizontal and vertical, km.
    pub voxel_km: f32,
    pub voxel_dz_km: f32,
    pub source: Option<RadarSource>,
    /// Where the radar is, local km (NEXRAD only).
    pub radar_xy: Option<(f32, f32)>,
    /// The measured scan itself, for volume rendering (NEXRAD only).
    #[serde(skip)]
    pub polar: Option<PolarVolume>,
}

/// A radar's newest Level II scan, laid out around the radar itself.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteScan {
    pub site: String,
    /// Scan time, RFC 3339.
    pub time: String,
    pub lat: f64,
    pub lon: f64,
    pub polar: PolarVolume,
}

/// Azimuth bins per sweep in a [`PolarVolume`] (0.5° each).
pub const AZIMUTHS: usize = 720;
/// Range kept from the radar, km.
pub const POLAR_RANGE_KM: f32 = 300.0;
/// At most this many tilts.
pub const MAX_SWEEPS: usize = 24;

/// One tilt of a [`PolarVolume`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolarSweep {
    pub elevation_deg: f32,
    pub first_gate_km: f32,
    pub gate_km: f32,
    pub gates: usize,
}

/// A Level II reflectivity scan laid out to be sampled directly, in the
/// radar's own coordinates, by a volume renderer: one layer per tilt (lowest
/// first), [`AZIMUTHS`] rows (clockwise from north, 0.5° each) of `width`
/// gates.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PolarVolume {
    /// The antenna: east and north of the scene centre (km), height above sea
    /// level (km).
    pub radar: [f32; 3],
    pub width: usize,
    pub sweeps: Vec<PolarSweep>,
    /// Reflectivity: `width × AZIMUTHS` bytes per tilt, tilt after tilt:
    /// 0 = no echo, otherwise `(dBZ + 32) × 2`.
    pub data: Vec<u8>,
    /// Other moments on the same layout (empty where a tilt lacks them),
    /// encoded with [`Moment::encode`].
    pub moments: Vec<(Moment, Vec<u8>)>,
}

impl PolarVolume {
    /// The bytes of `m` (reflectivity included), if the scan has it.
    pub fn moment(&self, m: Moment) -> Option<&[u8]> {
        if m == Moment::Ref {
            return Some(&self.data);
        }
        self.moments
            .iter()
            .find(|(k, _)| *k == m)
            .map(|(_, d)| d.as_slice())
    }
}

/// dBZ from a [`PolarVolume`] byte (`None` for no echo).
pub fn polar_dbz(b: u8) -> Option<f32> {
    (b > 0).then(|| f32::from(b) / 2.0 - 32.0)
}

/// Lay out `vol` for volume rendering in the scene around (`lat0`, `lon0`).
/// Every cut at the same tilt (split cuts send reflectivity and dual-pol in
/// one, velocity in another; SAILS repeats the lowest) is merged onto one
/// layout: reflectivity and each moment from the cut with the most radials
/// carrying it, on the reflectivity's gates.
pub fn polar_volume(vol: &Volume, lat0: f64, lon0: f64) -> PolarVolume {
    let mut sweeps: Vec<&Sweep> = vol
        .sweeps
        .iter()
        .filter(|s| !s.radials.is_empty())
        .collect();
    sweeps.sort_by(|a, b| a.elevation_deg.total_cmp(&b.elevation_deg));
    // Tilts: groups of cuts within 0.15° of each other.
    let mut groups: Vec<Vec<&Sweep>> = Vec::new();
    for s in sweeps {
        match groups.last_mut() {
            Some(g) if (s.elevation_deg - g[0].elevation_deg).abs() < 0.15 => g.push(s),
            _ => groups.push(vec![s]),
        }
    }
    groups.truncate(MAX_SWEEPS);
    let refs: Vec<&Sweep> = groups
        .iter()
        .map(|g| {
            *g.iter()
                .max_by_key(|s| s.radials.len())
                .expect("group is not empty")
        })
        .collect();
    let meta: Vec<PolarSweep> = refs
        .iter()
        .map(|s| {
            let r = &s.radials[0];
            let gate = r.gate_km.max(0.05);
            let most = s.radials.iter().map(|r| r.dbz.len()).max().unwrap_or(0);
            let reach = ((POLAR_RANGE_KM - r.first_gate_km) / gate).ceil().max(1.0) as usize;
            PolarSweep {
                elevation_deg: s.elevation_deg,
                first_gate_km: r.first_gate_km,
                gate_km: gate,
                gates: most.min(reach).max(1),
            }
        })
        .collect();
    let width = meta.iter().map(|m| m.gates).max().unwrap_or(1);
    let layer = width * AZIMUTHS;
    let n = refs.len();
    // The nearest radial to each azimuth bin (None across gaps).
    let bins = |s: &Sweep, has: &dyn Fn(&Radial) -> bool| -> Vec<Option<usize>> {
        let mut idx: Vec<usize> = (0..s.radials.len())
            .filter(|&i| has(&s.radials[i]))
            .collect();
        idx.sort_by(|&a, &b| {
            s.radials[a]
                .azimuth_deg
                .total_cmp(&s.radials[b].azimuth_deg)
        });
        let az: Vec<f32> = idx
            .iter()
            .map(|&i| s.radials[i].azimuth_deg.rem_euclid(360.0))
            .collect();
        (0..AZIMUTHS)
            .map(|bin| {
                if az.is_empty() {
                    return None;
                }
                let centre = (bin as f32 + 0.5) * 360.0 / AZIMUTHS as f32;
                let i = az.partition_point(|&a| a < centre);
                let cand = [i % az.len(), (i + az.len() - 1) % az.len()];
                let diff = |j: usize| {
                    let d = (az[j] - centre).abs();
                    d.min(360.0 - d)
                };
                let j = if diff(cand[0]) <= diff(cand[1]) {
                    cand[0]
                } else {
                    cand[1]
                };
                (diff(j) <= 1.5).then(|| idx[j])
            })
            .collect()
    };
    let mut data = vec![0u8; layer * n];
    for (k, s) in refs.iter().enumerate() {
        let out = &mut data[k * layer..(k + 1) * layer];
        for (bin, r) in bins(s, &|_| true).into_iter().enumerate() {
            let Some(r) = r else { continue };
            let row = &mut out[bin * width..(bin + 1) * width];
            for (g, &d) in s.radials[r].dbz.iter().take(meta[k].gates).enumerate() {
                if !d.is_nan() {
                    row[g] = Moment::Ref.encode(d);
                }
            }
        }
    }
    let mut moments = Vec::new();
    for m in [Moment::Vel, Moment::Zdr, Moment::Rho] {
        let mut out = vec![0u8; layer * n];
        let mut any = false;
        for (k, g) in groups.iter().enumerate() {
            let carries = |r: &Radial| r.moments.iter().any(|d| d.moment == m);
            let Some(s) = g
                .iter()
                .filter(|s| s.radials.iter().any(carries))
                .max_by_key(|s| s.radials.iter().filter(|r| carries(r)).count())
            else {
                continue;
            };
            any = true;
            let pm = meta[k];
            let lay = &mut out[k * layer..(k + 1) * layer];
            for (bin, r) in bins(s, &carries).into_iter().enumerate() {
                let Some(r) = r else { continue };
                let Some(d) = s.radials[r].moments.iter().find(|d| d.moment == m) else {
                    continue;
                };
                let row = &mut lay[bin * width..(bin + 1) * width];
                for (i, &v) in d.values.iter().enumerate() {
                    if v == 0 {
                        continue;
                    }
                    // Onto the reflectivity's gates.
                    let range = d.first_gate_km + d.gate_km * i as f32;
                    let gi = ((range - pm.first_gate_km) / pm.gate_km).round();
                    if gi >= 0.0 && (gi as usize) < pm.gates {
                        row[gi as usize] = v;
                    }
                }
            }
        }
        if any {
            moments.push((m, out));
        }
    }
    let (rx, ry) = local_km(lat0, lon0, vol.lat, vol.lon);
    PolarVolume {
        radar: [rx, ry, (vol.height_m / 1000.0) as f32],
        width,
        sweeps: meta,
        data,
        moments,
    }
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
                    moments: Vec::new(),
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
    fn lays_out_a_polar_volume() {
        let radial = |az: f32, elev: f32, v: f32| Radial {
            azimuth_deg: az,
            elevation_deg: elev,
            first_gate_km: 2.0,
            gate_km: 0.25,
            dbz: vec![f32::NAN, v, v],
            moments: vec![MomentData {
                moment: Moment::Vel,
                first_gate_km: 2.0,
                gate_km: 0.25,
                values: vec![0, Moment::Vel.encode(-v / 2.0), Moment::Vel.encode(v / 2.0)],
            }],
        };
        let sweep = |elev: f32, v: f32, step: f32| Sweep {
            elevation_deg: elev,
            radials: (0..(360.0 / step) as usize)
                .map(|i| radial(i as f32 * step, elev, v))
                .collect(),
        };
        let vol = Volume {
            site: "KTST".into(),
            lat: 35.0,
            lon: -97.0,
            height_m: 400.0,
            time: String::new(),
            // A repeated low tilt: the finer one is kept. Out of order on purpose.
            sweeps: vec![
                sweep(1.5, 40.0, 1.0),
                sweep(0.5, 20.0, 1.0),
                sweep(0.48, 30.0, 0.5),
            ],
        };
        let p = polar_volume(&vol, 35.0, -97.0);
        assert_eq!(p.sweeps.len(), 2);
        assert!((p.sweeps[0].elevation_deg - 0.48).abs() < 1e-6);
        assert_eq!(p.width, 3);
        assert_eq!(p.data.len(), 3 * AZIMUTHS * 2);
        assert_eq!(p.data[0], 0, "no echo stays empty");
        assert_eq!(polar_dbz(p.data[1]), Some(30.0));
        // Second tilt, bin 400 (200°).
        let at = AZIMUTHS * 3 + 400 * 3 + 2;
        assert_eq!(polar_dbz(p.data[at]), Some(40.0));
        assert!(p.radar[0].abs() < 1e-3 && (p.radar[2] - 0.4).abs() < 1e-6);
        // Velocity rides along on the same layout.
        let vel = p.moment(Moment::Vel).unwrap();
        assert_eq!(vel.len(), p.data.len());
        let v = Moment::Vel.decode(vel[2]).unwrap();
        assert!((v - 15.0).abs() < 0.3, "{v}");
        assert!(p.moment(Moment::Rho).is_none());
    }

    #[test]
    fn moments_round_trip() {
        for (m, v) in [
            (Moment::Ref, 42.5),
            (Moment::Vel, -23.0),
            (Moment::Zdr, 1.5),
            (Moment::Rho, 0.97),
        ] {
            let b = m.encode(v);
            assert!(b > 0);
            let back = m.decode(b).unwrap();
            let (lo, hi) = m.range();
            assert!((back - v).abs() <= (hi - lo) / 254.0, "{m:?} {v} {back}");
        }
        assert_eq!(Moment::Vel.decode(0), None);
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
