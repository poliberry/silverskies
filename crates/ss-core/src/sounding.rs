//! Forecast soundings: a vertical profile (pressure levels with height,
//! temperature, dewpoint and wind) and the severe-weather diagnostics a
//! meteorologist reads off it — a surface-based parcel (LCL, CAPE, CIN,
//! lifted index), freezing level, lapse rate, precipitable water, 0–6 km
//! shear, Bunkers storm motion, storm-relative helicity, and the composite
//! Supercell (SCP) and Significant Tornado (STP) parameters.

use serde::{Deserialize, Serialize};

const G: f64 = 9.80665;
const RD: f64 = 287.04;
const CP: f64 = 1005.7;
const LV: f64 = 2.501e6;
const EPS: f64 = 0.622;
const K: f64 = 273.15;

/// One level of the profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Level {
    pub pressure_hpa: f64,
    /// Height above mean sea level, m.
    pub height_m: f64,
    pub temp_c: f64,
    pub dewpoint_c: f64,
    pub wind_kt: f64,
    /// Direction the wind blows from, degrees.
    pub wind_dir_deg: f64,
}

impl Level {
    /// Wind components (u east, v north), m/s.
    pub fn uv(&self) -> (f64, f64) {
        let s = self.wind_kt * 0.514_444;
        let d = self.wind_dir_deg.to_radians();
        (-s * d.sin(), -s * d.cos())
    }
}

/// A sounding: levels ordered from the surface upward.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sounding {
    /// Forecast valid time (local, `YYYY-MM-DDTHH:MM`).
    pub time: String,
    /// Ground elevation, m.
    pub elevation_m: f64,
    pub levels: Vec<Level>,
}

/// Saturation vapour pressure over water, hPa (Bolton 1980).
pub fn sat_vapor_hpa(t_c: f64) -> f64 {
    6.112 * (17.67 * t_c / (t_c + 243.5)).exp()
}

/// Mixing ratio, kg/kg, for vapour pressure `e` at pressure `p` (hPa).
fn mixing_ratio(e: f64, p: f64) -> f64 {
    EPS * e / (p - e).max(1e-3)
}

/// Virtual temperature (K) of air at `t_c` with mixing ratio `w`.
fn virtual_k(t_c: f64, w: f64) -> f64 {
    (t_c + K) * (1.0 + w / EPS) / (1.0 + w)
}

/// Saturated (pseudo-)adiabatic lapse dT/dp (K per hPa) at `t_c`, `p` hPa.
fn moist_dtdp(t_c: f64, p: f64) -> f64 {
    let t = t_c + K;
    let ws = mixing_ratio(sat_vapor_hpa(t_c), p);
    let num = (RD * t + LV * ws) / p;
    let den = CP + (LV * LV * ws * EPS) / (RD * t * t);
    num / den
}

/// Lifting condensation level temperature (°C) from a parcel's T and Td
/// (Bolton 1980, eq. 15).
pub fn lcl_temp_c(t_c: f64, td_c: f64) -> f64 {
    let (t, td) = (t_c + K, td_c + K);
    1.0 / (1.0 / (td - 56.0) + (t / td).ln() / 800.0) + 56.0 - K
}

/// Severe-weather diagnostics for a sounding.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Indices {
    /// Surface-based CAPE, J/kg.
    pub cape: f64,
    /// Surface-based CIN, J/kg (≤ 0).
    pub cin: f64,
    /// Lifted index at 500 hPa, °C.
    pub lifted_index: f64,
    /// LCL height above ground, m.
    pub lcl_m: f64,
    pub lcl_pressure_hpa: f64,
    /// Level of free convection height above ground, m.
    pub lfc_m: Option<f64>,
    /// Equilibrium level height above ground, m.
    pub el_m: Option<f64>,
    /// Freezing level above ground, m.
    pub freezing_m: Option<f64>,
    /// 700–500 hPa lapse rate, °C/km.
    pub lapse_700_500: Option<f64>,
    /// Precipitable water, mm.
    pub pwat_mm: f64,
    /// 0–6 km bulk shear, kt.
    pub shear_06_kt: f64,
    /// 0–1 km bulk shear, kt.
    pub shear_01_kt: f64,
    /// Bunkers right-mover storm motion (direction from °, speed kt).
    pub bunkers_rm: (f64, f64),
    /// Storm-relative helicity for the right mover, m²/s².
    pub srh_01: f64,
    pub srh_03: f64,
    /// Supercell composite parameter.
    pub scp: f64,
    /// Significant tornado parameter (fixed layer).
    pub stp: f64,
}

/// A parcel trace: (pressure hPa, parcel temperature °C) from the surface up.
pub type ParcelTrace = Vec<(f64, f64)>;

impl Sounding {
    fn surface(&self) -> Option<&Level> {
        self.levels.first()
    }

    /// Height above ground of a level.
    fn agl(&self, l: &Level) -> f64 {
        l.height_m - self.elevation_m
    }

    /// Profile value at height `h_agl` (m), linearly interpolated.
    fn at_height(&self, h_agl: f64, f: impl Fn(&Level) -> f64) -> Option<f64> {
        let ls = &self.levels;
        for w in ls.windows(2) {
            let (h0, h1) = (self.agl(&w[0]), self.agl(&w[1]));
            if h_agl >= h0 && h_agl <= h1 && h1 > h0 {
                let t = (h_agl - h0) / (h1 - h0);
                return Some(f(&w[0]) + (f(&w[1]) - f(&w[0])) * t);
            }
        }
        None
    }

    /// Wind (u, v) m/s at height above ground.
    pub fn wind_at(&self, h_agl: f64) -> Option<(f64, f64)> {
        Some((
            self.at_height(h_agl, |l| l.uv().0)?,
            self.at_height(h_agl, |l| l.uv().1)?,
        ))
    }

    /// Temperature at pressure `p` (log-p interpolation).
    fn temp_at_p(&self, p: f64) -> Option<f64> {
        self.interp_p(p, |l| l.temp_c)
    }

    fn interp_p(&self, p: f64, f: impl Fn(&Level) -> f64) -> Option<f64> {
        for w in self.levels.windows(2) {
            let (p0, p1) = (w[0].pressure_hpa, w[1].pressure_hpa);
            if p <= p0 && p >= p1 && p0 > p1 {
                let t = (p0.ln() - p.ln()) / (p0.ln() - p1.ln());
                return Some(f(&w[0]) + (f(&w[1]) - f(&w[0])) * t);
            }
        }
        None
    }

    fn height_at_p(&self, p: f64) -> Option<f64> {
        self.interp_p(p, |l| self.agl(l))
    }

    /// The surface parcel lifted dry to its LCL, then moist above.
    pub fn parcel(&self) -> ParcelTrace {
        let Some(s) = self.surface() else {
            return Vec::new();
        };
        let top = self
            .levels
            .last()
            .map(|l| l.pressure_hpa)
            .unwrap_or(s.pressure_hpa);
        let t_lcl = lcl_temp_c(s.temp_c, s.dewpoint_c);
        let p_lcl = s.pressure_hpa * ((t_lcl + K) / (s.temp_c + K)).powf(CP / RD);
        let mut out = Vec::new();
        let mut p = s.pressure_hpa;
        let step = 5.0;
        // Dry adiabat (potential temperature conserved).
        while p > p_lcl && p > top {
            let t = (s.temp_c + K) * (p / s.pressure_hpa).powf(RD / CP) - K;
            out.push((p, t));
            p -= step;
        }
        out.push((p_lcl.max(top), t_lcl));
        // Moist adiabat, integrated in pressure.
        let (mut p, mut t) = (p_lcl, t_lcl);
        while p - step >= top {
            let dt = moist_dtdp(t, p) * step;
            t -= dt;
            p -= step;
            out.push((p, t));
        }
        out
    }

    /// All the diagnostics (empty defaults if the profile is too short).
    pub fn indices(&self) -> Indices {
        let mut ix = Indices::default();
        let Some(s) = self.surface().copied() else {
            return ix;
        };
        if self.levels.len() < 3 {
            return ix;
        }

        // --- parcel: LCL, CAPE/CIN, LFC/EL, LI ---
        let trace = self.parcel();
        let t_lcl = lcl_temp_c(s.temp_c, s.dewpoint_c);
        ix.lcl_pressure_hpa = s.pressure_hpa * ((t_lcl + K) / (s.temp_c + K)).powf(CP / RD);
        ix.lcl_m = self
            .height_at_p(ix.lcl_pressure_hpa)
            .unwrap_or(0.0)
            .max(0.0);
        let (mut cape, mut cin) = (0.0, 0.0);
        let mut lfc: Option<f64> = None;
        let mut el: Option<f64> = None;
        let w_sfc = mixing_ratio(sat_vapor_hpa(s.dewpoint_c), s.pressure_hpa);
        for w in trace.windows(2) {
            let (p0, tp0) = w[0];
            let (p1, tp1) = w[1];
            let (Some(te0), Some(te1)) = (self.temp_at_p(p0), self.temp_at_p(p1)) else {
                continue;
            };
            let (Some(td0), Some(td1)) = (
                self.interp_p(p0, |l| l.dewpoint_c),
                self.interp_p(p1, |l| l.dewpoint_c),
            ) else {
                continue;
            };
            // Parcel mixing ratio: constant below the LCL, saturated above.
            let wp = |p: f64, t: f64| {
                if p >= ix.lcl_pressure_hpa {
                    w_sfc
                } else {
                    mixing_ratio(sat_vapor_hpa(t), p)
                }
            };
            let tvp = 0.5 * (virtual_k(tp0, wp(p0, tp0)) + virtual_k(tp1, wp(p1, tp1)));
            let tve = 0.5
                * (virtual_k(te0, mixing_ratio(sat_vapor_hpa(td0), p0))
                    + virtual_k(te1, mixing_ratio(sat_vapor_hpa(td1), p1)));
            // dz from the hypsometric relation over the layer.
            let dz = RD * tve / G * (p0 / p1).ln();
            let b = G * (tvp - tve) / tve * dz;
            let h = self.height_at_p(p1).unwrap_or(0.0);
            if b > 0.0 {
                if lfc.is_none() && p1 <= ix.lcl_pressure_hpa {
                    lfc = Some(h);
                }
                cape += b;
                el = Some(h);
            } else if lfc.is_none() {
                cin += b;
            }
        }
        ix.cape = cape;
        ix.cin = if lfc.is_some() { cin.min(0.0) } else { 0.0 };
        ix.lfc_m = lfc;
        ix.el_m = el;
        if let (Some(te), Some((_, tp))) = (
            self.temp_at_p(500.0),
            trace
                .iter()
                .min_by(|a, b| (a.0 - 500.0).abs().total_cmp(&(b.0 - 500.0).abs())),
        ) {
            ix.lifted_index = te - tp;
        }

        // --- temperature structure ---
        ix.freezing_m = self.levels.windows(2).find_map(|w| {
            let (a, b) = (w[0], w[1]);
            (a.temp_c > 0.0 && b.temp_c <= 0.0).then(|| {
                let t = a.temp_c / (a.temp_c - b.temp_c);
                self.agl(&a) + (self.agl(&b) - self.agl(&a)) * t
            })
        });
        if s.temp_c <= 0.0 {
            ix.freezing_m = Some(0.0);
        }
        if let (Some(t7), Some(t5), Some(z7), Some(z5)) = (
            self.temp_at_p(700.0),
            self.temp_at_p(500.0),
            self.height_at_p(700.0),
            self.height_at_p(500.0),
        ) {
            if z5 > z7 {
                ix.lapse_700_500 = Some((t7 - t5) / (z5 - z7) * 1000.0);
            }
        }
        // Precipitable water: (1/g) ∫ q dp.
        let mut pw = 0.0;
        for w in self.levels.windows(2) {
            let q = |l: &Level| {
                let r = mixing_ratio(sat_vapor_hpa(l.dewpoint_c), l.pressure_hpa);
                r / (1.0 + r)
            };
            let dp = (w[0].pressure_hpa - w[1].pressure_hpa) * 100.0;
            pw += 0.5 * (q(&w[0]) + q(&w[1])) * dp / G;
        }
        ix.pwat_mm = pw; // kg/m² == mm

        // --- kinematics ---
        let wind = |h: f64| self.wind_at(h);
        let mag_kt = |a: (f64, f64), b: (f64, f64)| {
            ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() / 0.514_444
        };
        if let (Some(w0), Some(w1), Some(w6)) = (wind(0.0), wind(1000.0), wind(6000.0)) {
            ix.shear_01_kt = mag_kt(w0, w1);
            ix.shear_06_kt = mag_kt(w0, w6);
        }
        // Bunkers (2000): 0–6 km mean wind plus 7.5 m/s to the right of the
        // shear between the 0–0.5 km and 5.5–6 km mean winds.
        let mean = |a: f64, b: f64| -> Option<(f64, f64)> {
            let n = 10;
            let mut acc = (0.0, 0.0);
            for i in 0..=n {
                let (u, v) = wind(a + (b - a) * f64::from(i) / f64::from(n))?;
                acc.0 += u;
                acc.1 += v;
            }
            Some((acc.0 / f64::from(n + 1), acc.1 / f64::from(n + 1)))
        };
        if let (Some(m), Some(lo), Some(hi)) =
            (mean(0.0, 6000.0), mean(0.0, 500.0), mean(5500.0, 6000.0))
        {
            let sh = (hi.0 - lo.0, hi.1 - lo.1);
            let mag = (sh.0 * sh.0 + sh.1 * sh.1).sqrt().max(1e-6);
            let rm = (m.0 + 7.5 * sh.1 / mag, m.1 - 7.5 * sh.0 / mag);
            let speed = (rm.0 * rm.0 + rm.1 * rm.1).sqrt();
            let from = (rm.0.atan2(rm.1).to_degrees() + 180.0).rem_euclid(360.0);
            ix.bunkers_rm = (from, speed / 0.514_444);
            let srh = |top: f64| -> f64 {
                let n = (top / 100.0) as usize;
                let mut h = 0.0;
                for i in 0..n {
                    let (Some(a), Some(b)) = (wind(i as f64 * 100.0), wind((i + 1) as f64 * 100.0))
                    else {
                        break;
                    };
                    h += (b.0 - rm.0) * (a.1 - rm.1) - (a.0 - rm.0) * (b.1 - rm.1);
                }
                h
            };
            ix.srh_01 = srh(1000.0);
            ix.srh_03 = srh(3000.0);
        }

        // --- composites (SPC definitions, surface-based parcel) ---
        let shear_ms = ix.shear_06_kt * 0.514_444;
        let shear_scp = if shear_ms < 10.0 {
            0.0
        } else {
            (shear_ms / 20.0).min(1.0)
        };
        let cin_scp = if ix.cin > -40.0 { 1.0 } else { -40.0 / ix.cin };
        ix.scp = (ix.cape / 1000.0) * (ix.srh_03 / 50.0) * shear_scp * cin_scp;
        let lcl_term = if ix.lcl_m < 1000.0 {
            1.0
        } else if ix.lcl_m > 2000.0 {
            0.0
        } else {
            (2000.0 - ix.lcl_m) / 1000.0
        };
        let shear_stp = if shear_ms < 12.5 {
            0.0
        } else {
            (shear_ms / 20.0).min(1.5)
        };
        let cin_stp = if ix.cin > -50.0 {
            1.0
        } else if ix.cin < -200.0 {
            0.0
        } else {
            (200.0 + ix.cin) / 150.0
        };
        ix.stp =
            ((ix.cape / 1500.0) * lcl_term * (ix.srh_01 / 150.0) * shear_stp * cin_stp).max(0.0);
        ix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A classic loaded-gun spring sounding: warm moist boundary layer,
    /// steep mid-level lapse rates, veering winds strengthening with height.
    fn severe() -> Sounding {
        let rows = [
            // p, z(msl), T, Td, kt, dir
            (970.0, 360.0, 30.0, 22.0, 15.0, 160.0),
            (950.0, 540.0, 28.0, 20.5, 30.0, 175.0),
            (925.0, 770.0, 25.5, 19.0, 38.0, 190.0),
            (900.0, 1000.0, 23.5, 17.0, 40.0, 200.0),
            (850.0, 1480.0, 20.0, 13.0, 42.0, 215.0),
            (800.0, 1980.0, 17.0, 5.0, 40.0, 225.0),
            (700.0, 3080.0, 9.0, -6.0, 42.0, 235.0),
            (600.0, 4300.0, 0.0, -15.0, 48.0, 240.0),
            (500.0, 5720.0, -11.0, -25.0, 55.0, 245.0),
            (400.0, 7400.0, -24.0, -38.0, 65.0, 250.0),
            (300.0, 9480.0, -40.0, -52.0, 75.0, 255.0),
            (250.0, 10750.0, -49.0, -60.0, 80.0, 255.0),
            (200.0, 12200.0, -55.0, -66.0, 70.0, 255.0),
        ];
        Sounding {
            time: "2026-05-20T18:00".into(),
            elevation_m: 360.0,
            levels: rows
                .iter()
                .map(|&(p, z, t, td, kt, d)| Level {
                    pressure_hpa: p,
                    height_m: z,
                    temp_c: t,
                    dewpoint_c: td,
                    wind_kt: kt,
                    wind_dir_deg: d,
                })
                .collect(),
        }
    }

    #[test]
    fn thermo_helpers_match_reference_values() {
        assert!((sat_vapor_hpa(0.0) - 6.112).abs() < 0.01);
        assert!((sat_vapor_hpa(20.0) - 23.37).abs() < 0.1);
        // T 30, Td 22 → LCL temperature about 20.3 °C.
        assert!(
            (lcl_temp_c(30.0, 22.0) - 20.3).abs() < 0.5,
            "{}",
            lcl_temp_c(30.0, 22.0)
        );
    }

    #[test]
    fn severe_sounding_reads_as_severe() {
        let ix = severe().indices();
        assert!(ix.cape > 2500.0 && ix.cape < 6000.0, "CAPE {}", ix.cape);
        assert!(ix.lifted_index < -6.0, "LI {}", ix.lifted_index);
        assert!(ix.lcl_m > 600.0 && ix.lcl_m < 1400.0, "LCL {}", ix.lcl_m);
        assert!(ix.shear_06_kt > 35.0, "shear {}", ix.shear_06_kt);
        assert!(ix.srh_01 > 100.0, "SRH1 {}", ix.srh_01);
        assert!(ix.srh_03 > ix.srh_01, "SRH3 {}", ix.srh_03);
        assert!(ix.scp > 4.0, "SCP {}", ix.scp);
        assert!(ix.stp > 1.0, "STP {}", ix.stp);
        let lr = ix.lapse_700_500.unwrap();
        assert!(lr > 6.5 && lr < 9.0, "lapse {lr}");
        assert!(ix.freezing_m.unwrap() > 3000.0);
        assert!(ix.pwat_mm > 25.0 && ix.pwat_mm < 60.0, "PW {}", ix.pwat_mm);
        // Bunkers right mover: to the right of the mean wind, 20–45 kt.
        let (dir, spd) = ix.bunkers_rm;
        assert!(dir > 200.0 && dir < 290.0, "RM dir {dir}");
        assert!(spd > 20.0 && spd < 50.0, "RM speed {spd}");
    }

    #[test]
    fn stable_cold_sounding_has_no_cape_and_no_composites() {
        let mut s = severe();
        // A cold, stable column: 4 °C/km, dry.
        for l in s.levels.iter_mut() {
            l.temp_c = -2.0 - 0.004 * (l.height_m - 360.0);
            l.dewpoint_c = l.temp_c - 10.0;
        }
        let ix = s.indices();
        assert!(ix.cape < 10.0, "{}", ix.cape);
        assert!(ix.lifted_index > 0.0);
        assert_eq!(ix.stp, 0.0);
        assert_eq!(ix.freezing_m, Some(0.0));
    }

    #[test]
    fn parcel_cools_dry_then_moist() {
        let s = severe();
        let tr = s.parcel();
        assert!(tr.len() > 50);
        assert!(tr
            .windows(2)
            .all(|w| w[1].0 < w[0].0 && w[1].1 <= w[0].1 + 1e-9));
        // Moist ascent cools more slowly than dry: compare per-hPa rates.
        let dry = (tr[0].1 - tr[3].1) / (tr[0].0 - tr[3].0);
        let n = tr.len();
        let moist = (tr[n / 2].1 - tr[n / 2 + 3].1) / (tr[n / 2].0 - tr[n / 2 + 3].0);
        assert!(dry > 0.0 && moist > 0.0);
    }

    #[test]
    fn empty_sounding_is_harmless() {
        let s = Sounding {
            time: String::new(),
            elevation_m: 0.0,
            levels: vec![],
        };
        assert_eq!(s.indices(), Indices::default());
        assert!(s.parcel().is_empty());
    }
}
