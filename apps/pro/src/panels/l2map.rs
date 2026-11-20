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
    }
}
