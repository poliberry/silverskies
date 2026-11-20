//! Level II products on the 2D radar map: one tilt of a site's newest scan,
//! drawn into an image around the radar (rows spaced in Web Mercator, as
//! the map is) on a worker thread.

use std::sync::Arc;

use crossbeam_channel::Receiver;
use egui::{Color32, ColorImage};
use ss_core::radar3d::{L2Product, Moment, SiteScan, AZIMUTHS};

/// Image size (pixels a side).
const SIZE: usize = 1400;
/// Effective earth radius for beam bending (4/3 earth), km.
const KE_A: f32 = 8494.7;

/// A rendered tilt.
pub struct L2Image {
    pub image: ColorImage,
    /// West, south, east, north.
    pub bounds: [f64; 4],
    pub elevation_deg: f32,
}

/// Draw tilt `tilt` of `scan` as `product`. `motion` (east, north, m/s) is
/// subtracted for storm-relative velocity.
pub fn render(
    scan: &SiteScan,
    product: L2Product,
    tilt: usize,
    motion: [f32; 2],
) -> Option<L2Image> {
    let p = &scan.polar;
    let k = tilt.min(p.sweeps.len().checked_sub(1)?);
    let sw = p.sweeps[k];
    let moment = product.moment();
    let data = p.moment(moment)?;
    let layer = p.width * AZIMUTHS;
    let lay = &data[k * layer..(k + 1) * layer];
    // Other products are shown only where this tilt has an echo of at
    // least 5 dBZ (hides clear-air noise), unless it has no reflectivity.
    let refl = &p.data[k * layer..(k + 1) * layer];
    let mask = moment != Moment::Ref && refl.iter().any(|&b| b != 0);
    let reach = sw.first_gate_km + sw.gate_km * sw.gates as f32;
    // The box around the radar, in degrees, rows evenly spaced in mercator.
    let deg_lat = f64::from(reach) / 111.19;
    let deg_lon = deg_lat / scan.lat.to_radians().cos().max(0.05);
    let (west, east) = (scan.lon - deg_lon, scan.lon + deg_lon);
    let (south, north) = (scan.lat - deg_lat, scan.lat + deg_lat);
    let merc = |lat: f64| {
        (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0)
            .tan()
            .ln()
    };
    let (my0, my1) = (merc(north), merc(south));
    let km_lon = 111.19 * scan.lat.to_radians().cos();
    let cos_e = sw.elevation_deg.to_radians().cos();
    let mut px = vec![Color32::TRANSPARENT; SIZE * SIZE];
    for j in 0..SIZE {
        let my = my0 + (my1 - my0) * (j as f64 + 0.5) / SIZE as f64;
        let lat = (2.0 * my.exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees();
        let y = ((lat - scan.lat) * 111.19) as f32;
        for i in 0..SIZE {
            let lon = west + (east - west) * (i as f64 + 0.5) / SIZE as f64;
            let x = ((lon - scan.lon) * km_lon) as f32;
            let s = x.hypot(y);
            if s > reach || s < 0.5 {
                continue;
            }
            // Ground distance to slant range along the beam.
            let r = s / cos_e + s * s / (2.0 * KE_A) * sw.elevation_deg.to_radians().sin();
            let gate = ((r - sw.first_gate_km) / sw.gate_km).round();
            if gate < 0.0 || gate as usize >= sw.gates {
                continue;
            }
            let mut az = x.atan2(y).to_degrees();
            if az < 0.0 {
                az += 360.0;
            }
            let bin = ((az / 360.0 * AZIMUTHS as f32) as usize).min(AZIMUTHS - 1);
            let b = lay[bin * p.width + gate as usize];
            if mask
                && Moment::Ref
                    .decode(refl[bin * p.width + gate as usize])
                    .is_none_or(|r| r < 5.0)
            {
                continue;
            }
            let Some(mut v) = moment.decode(b) else {
                continue;
            };
            if moment == Moment::Ref && v < 5.0 {
                continue;
            }
            if product == L2Product::StormRelative {
                v -= (motion[0] * x + motion[1] * y) / s;
            }
            let c = product.color(v);
            px[j * SIZE + i] = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 235);
        }
    }
    Some(L2Image {
        image: ColorImage::new([SIZE, SIZE], px),
        bounds: [west, south, east, north],
        elevation_deg: sw.elevation_deg,
    })
}

/// Height of a cross section's top, km above sea level.
pub const XS_TOP_KM: f32 = 16.0;
/// Cross-section image size.
pub const XS_W: usize = 720;
pub const XS_H: usize = 300;

/// A vertical slice.
pub struct XsImage {
    pub image: ColorImage,
    pub length_km: f32,
}

/// The encoded value of tilt `k` at slant range `r`, azimuth `az`.
fn tilt_value(scan: &SiteScan, data: &[u8], k: usize, r: f32, az: f32) -> u8 {
    let p = &scan.polar;
    let sw = p.sweeps[k];
    let gate = ((r - sw.first_gate_km) / sw.gate_km).round();
    if gate < 0.0 || gate as usize >= sw.gates {
        return 0;
    }
    let bin = ((az / 360.0 * AZIMUTHS as f32) as usize).min(AZIMUTHS - 1);
    data[k * p.width * AZIMUTHS + bin * p.width + gate as usize]
}

/// The slice under the line from `a` to `b` ((lon, lat)), from the ground
/// to [`XS_TOP_KM`]: each point is turned into the radar's range, azimuth
/// and beam elevation, and taken from the tilt(s) around it (close tilts
/// blended, far-apart ones only within their own ~1° beam).
pub fn cross_section(
    scan: &SiteScan,
    product: L2Product,
    a: (f64, f64),
    b: (f64, f64),
    motion: [f32; 2],
) -> Option<XsImage> {
    let p = &scan.polar;
    let n = p.sweeps.len();
    if n == 0 {
        return None;
    }
    let moment = product.moment();
    let data = p.moment(moment)?;
    let km_lon = 111.19 * scan.lat.to_radians().cos();
    let local = |(lon, lat): (f64, f64)| {
        (
            ((lon - scan.lon) * km_lon) as f32,
            ((lat - scan.lat) * 111.19) as f32,
        )
    };
    let (ax, ay) = local(a);
    let (bx, by) = local(b);
    let length_km = (bx - ax).hypot(by - ay);
    let elev: Vec<f32> = p.sweeps.iter().map(|s| s.elevation_deg).collect();
    let mut px = vec![Color32::TRANSPARENT; XS_W * XS_H];
    for i in 0..XS_W {
        let t = (i as f32 + 0.5) / XS_W as f32;
        let (x, y) = (ax + (bx - ax) * t, ay + (by - ay) * t);
        let s = x.hypot(y);
        if s < 0.5 {
            continue;
        }
        let mut az = x.atan2(y).to_degrees();
        if az < 0.0 {
            az += 360.0;
        }
        for j in 0..XS_H {
            let z = XS_TOP_KM * (XS_H - 1 - j) as f32 / (XS_H - 1) as f32;
            let h = z - p.radar[2];
            let e = (h - s * s / (2.0 * KE_A)).atan2(s).to_degrees();
            let r = s.hypot(h);
            if e < elev[0] - 0.5 || e > elev[n - 1] + 0.5 {
                continue;
            }
            let k = elev.iter().rposition(|&el| e >= el).unwrap_or(0);
            let val = |k: usize| moment.decode(tilt_value(scan, data, k, r, az));
            let v = if k + 1 < n && e > elev[k] {
                let gap = elev[k + 1] - elev[k];
                let f = (e - elev[k]) / gap;
                match (val(k), val(k + 1)) {
                    (Some(lo), Some(hi)) if gap <= 3.0 => Some(lo + (hi - lo) * f),
                    (lo, hi) if gap <= 3.0 => lo.or(hi),
                    (lo, _) if e - elev[k] < 0.6 => lo,
                    (_, hi) if elev[k + 1] - e < 0.6 => hi,
                    _ => None,
                }
            } else {
                val(k)
            };
            let Some(mut v) = v else { continue };
            // Hide the speckle of very weak returns in the slice.
            if moment == Moment::Ref && v < 10.0 {
                continue;
            }
            if product == L2Product::StormRelative {
                v -= (motion[0] * x + motion[1] * y) / s;
            }
            let c = product.color(v);
            px[j * XS_W + i] = Color32::from_rgb(c[0], c[1], c[2]);
        }
    }
    Some(XsImage {
        image: ColorImage::new([XS_W, XS_H], px),
        length_km,
    })
}

/// Cross sections on a worker thread.
#[derive(Default)]
pub struct XsWorker {
    key: Option<String>,
    rx: Option<Receiver<Option<XsImage>>>,
}

impl XsWorker {
    pub fn update(
        &mut self,
        scan: &Arc<SiteScan>,
        product: L2Product,
        line: ((f64, f64), (f64, f64)),
        motion: [f32; 2],
    ) -> Option<Option<XsImage>> {
        let key = format!("{}:{}:{product:?}:{line:?}", scan.site, scan.time);
        if self.key.as_deref() != Some(key.as_str()) {
            self.key = Some(key);
            let (tx, rx) = crossbeam_channel::bounded(1);
            let scan = scan.clone();
            let spawned = std::thread::Builder::new()
                .name("ss-l2-xsect".into())
                .spawn(move || {
                    let _ = tx.send(cross_section(&scan, product, line.0, line.1, motion));
                });
            self.rx = spawned.ok().map(|_| rx);
        }
        let got = self.rx.as_ref()?.try_recv().ok()?;
        self.rx = None;
        Some(got)
    }

    pub fn clear(&mut self) {
        self.key = None;
        self.rx = None;
    }

    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }
}

/// Renders on a worker thread when what's wanted changes.
#[derive(Default)]
pub struct L2Map {
    key: Option<String>,
    rx: Option<Receiver<Option<L2Image>>>,
    /// The tilt shown (degrees), once drawn.
    pub elevation: Option<f32>,
}

impl L2Map {
    /// Ask for `product` at `tilt` of `scan`; returns a finished image when
    /// one arrives.
    pub fn update(
        &mut self,
        scan: &Arc<SiteScan>,
        product: L2Product,
        tilt: usize,
        motion: [f32; 2],
    ) -> Option<Option<L2Image>> {
        let key = format!("{}:{}:{product:?}:{tilt}:{motion:?}", scan.site, scan.time);
        if self.key.as_deref() != Some(key.as_str()) {
            self.key = Some(key);
            let (tx, rx) = crossbeam_channel::bounded(1);
            let scan = scan.clone();
            let spawned = std::thread::Builder::new()
                .name("ss-l2-map".into())
                .spawn(move || {
                    let _ = tx.send(render(&scan, product, tilt, motion));
                });
            self.rx = spawned.ok().map(|_| rx);
        }
        let got = self.rx.as_ref()?.try_recv().ok()?;
        self.rx = None;
        self.elevation = got.as_ref().map(|g| g.elevation_deg);
        Some(got)
    }

    /// Forget the current image (the product was turned off).
    pub fn clear(&mut self) {
        self.key = None;
        self.rx = None;
        self.elevation = None;
    }

    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss_core::radar3d::{PolarSweep, PolarVolume};

    #[test]
    fn draws_an_echo_east_of_the_radar() {
        // One tilt, 400 gates of 0.25 km; an echo 40–50 km out, due east.
        let width = 400;
        let mut data = vec![0u8; width * AZIMUTHS];
        // A few degrees wide around due east.
        for bin in AZIMUTHS / 4 - 6..AZIMUTHS / 4 + 6 {
            for g in 160..200 {
                data[bin * width + g] = Moment::Ref.encode(50.0);
            }
        }
        let scan = SiteScan {
            site: "KTST".into(),
            time: String::new(),
            lat: 35.0,
            lon: -97.0,
            polar: PolarVolume {
                radar: [0.0; 3],
                width,
                sweeps: vec![PolarSweep {
                    elevation_deg: 0.5,
                    first_gate_km: 0.0,
                    gate_km: 0.25,
                    gates: width,
                }],
                data,
                moments: Vec::new(),
            },
        };
        let img = render(&scan, L2Product::Reflectivity, 0, [0.0; 2]).unwrap();
        // Middle row, ~45 km east of centre (the box spans ±100 km).
        let i = SIZE / 2 + (45.0 / 200.0 * SIZE as f32) as usize;
        let c = img.image.pixels[SIZE / 2 * SIZE + i];
        assert!(c.a() > 200, "{c:?}");
        // West stays empty.
        assert_eq!(img.image.pixels[SIZE / 2 * SIZE + SIZE / 4].a(), 0);
        assert!(img.bounds[2] > img.bounds[0] && img.bounds[3] > img.bounds[1]);
        // No velocity in this scan.
        assert!(render(&scan, L2Product::Velocity, 0, [0.0; 2]).is_none());
        // A slice due east through the echo: filled low down 40–50 km out,
        // empty high above it (a 0.5° beam is ~0.4 km up there).
        let deg = 1.0 / (111.19 * 35f64.to_radians().cos());
        let xs = cross_section(
            &scan,
            L2Product::Reflectivity,
            (-97.0, 35.0),
            (-97.0 + 60.0 * deg, 35.0),
            [0.0; 2],
        )
        .unwrap();
        assert!((xs.length_km - 60.0).abs() < 0.5);
        let col = (45.0 / 60.0 * XS_W as f32) as usize;
        let filled: Vec<usize> = (0..XS_H)
            .filter(|&j| xs.image.pixels[j * XS_W + col].a() > 0)
            .collect();
        assert!(!filled.is_empty());
        let top_km = XS_TOP_KM * (XS_H - 1 - filled[0]) as f32 / (XS_H - 1) as f32;
        assert!(top_km < 1.5, "{top_km}");
    }
}
