//! Forecast model maps: an ECMWF field coloured over the whole world (Web
//! Mercator rows for the map), with isolines for pressure and 500 hPa
//! height, and values for hover readouts.

use egui::{Color32, ColorImage};
use ss_core::types::{ModelField, UnitPref};
use ss_net::grib::Field;

/// Image width (finer than the 0.25° grid, interpolated, so contours and
/// edges stay smooth when zoomed in).
const W: usize = 2048;
/// Mercator's latitude limit.
pub const MAX_LAT: f64 = 85.05;

/// A rendered field: the image, its bounds, and the values (in the
/// field's display base unit) for hover.
pub struct ModelMap {
    pub field: ModelField,
    pub step: u32,
    pub image: ColorImage,
    pub bounds: [f64; 4],
    pub grid: Field,
}

/// Turn the raw fields into one grid in a base unit: °C, mm/h, m/s, hPa,
/// J/kg, %, dam.
pub fn combine(field: ModelField, raw: &[Field]) -> Option<Field> {
    let first = raw.first()?.clone();
    let map = |f: &dyn Fn(f32) -> f32| Field {
        values: first.values.iter().map(|v| f(*v)).collect(),
        ..first.clone()
    };
    Some(match field {
        ModelField::Temperature => map(&|k| k - 273.15),
        ModelField::Precipitation => map(&|r| r * 3600.0),
        ModelField::Gusts => first.clone(),
        ModelField::Pressure => map(&|p| p / 100.0),
        ModelField::Cape => first.clone(),
        ModelField::Cloud => {
            // 0–1 in the open data; percent either way.
            let max = first.values.iter().copied().fold(0f32, f32::max);
            if max <= 1.5 {
                map(&|c| c * 100.0)
            } else {
                first.clone()
            }
        }
        ModelField::Height500 => map(&|z| z / 10.0),
        ModelField::Wind => {
            let v = raw.get(1)?;
            Field {
                values: first
                    .values
                    .iter()
                    .zip(&v.values)
                    .map(|(u, v)| u.hypot(*v))
                    .collect(),
                ..first.clone()
            }
        }
    })
}

/// Colour stops in the base unit.
fn stops(field: ModelField) -> &'static [(f32, [u8; 4])] {
    match field {
        ModelField::Temperature => &[
            (-40.0, [120, 40, 160, 200]),
            (-25.0, [70, 70, 200, 200]),
            (-10.0, [60, 140, 230, 200]),
            (0.0, [140, 210, 240, 200]),
            (8.0, [120, 200, 120, 200]),
            (16.0, [230, 220, 90, 200]),
            (24.0, [245, 160, 60, 200]),
            (32.0, [225, 70, 50, 200]),
            (42.0, [140, 20, 40, 200]),
        ],
        ModelField::Precipitation => &[
            (0.1, [150, 200, 255, 140]),
            (1.0, [60, 140, 240, 200]),
            (3.0, [40, 190, 90, 215]),
            (8.0, [250, 220, 40, 225]),
            (20.0, [240, 110, 30, 230]),
            (50.0, [200, 30, 160, 235]),
        ],
        ModelField::Wind | ModelField::Gusts => &[
            (0.0, [70, 110, 170, 110]),
            (5.0, [60, 170, 170, 170]),
            (10.0, [90, 190, 90, 190]),
            (17.0, [230, 210, 60, 200]),
            (25.0, [240, 130, 40, 210]),
            (33.0, [220, 40, 50, 220]),
            (45.0, [170, 30, 170, 230]),
        ],
        ModelField::Pressure => &[
            (970.0, [120, 60, 170, 120]),
            (990.0, [70, 120, 210, 110]),
            (1005.0, [110, 190, 220, 90]),
            (1015.0, [190, 210, 150, 80]),
            (1025.0, [230, 190, 110, 100]),
            (1040.0, [220, 110, 70, 120]),
        ],
        ModelField::Cape => &[
            (100.0, [120, 200, 120, 120]),
            (500.0, [230, 220, 80, 180]),
            (1000.0, [245, 160, 50, 200]),
            (2000.0, [230, 70, 40, 215]),
            (3000.0, [200, 30, 120, 225]),
            (4500.0, [150, 40, 200, 235]),
        ],
        ModelField::Cloud => &[
            (10.0, [220, 225, 235, 0]),
            (50.0, [220, 225, 235, 110]),
            (100.0, [245, 247, 250, 210]),
        ],
        ModelField::Height500 => &[
            (480.0, [110, 60, 170, 140]),
            (520.0, [70, 130, 210, 130]),
            (552.0, [120, 200, 160, 120]),
            (576.0, [230, 200, 100, 130]),
            (592.0, [220, 110, 70, 140]),
        ],
    }
}

/// Values below the first stop are left clear for these.
fn clear_below(field: ModelField) -> bool {
    matches!(
        field,
        ModelField::Precipitation | ModelField::Cape | ModelField::Cloud
    )
}

/// Isoline spacing (base unit), for fields drawn with lines.
fn isoline(field: ModelField) -> Option<f32> {
    match field {
        ModelField::Pressure => Some(4.0),
        ModelField::Height500 => Some(6.0),
        _ => None,
    }
}

pub fn color(field: ModelField, v: f32) -> Color32 {
    let st = stops(field);
    if !v.is_finite() || (clear_below(field) && v < st[0].0) {
        return Color32::TRANSPARENT;
    }
    let c = match st.iter().position(|(t, _)| v < *t) {
        Some(0) => st[0].1,
        None => st[st.len() - 1].1,
        Some(i) => {
            let ((a, ca), (b, cb)) = (st[i - 1], st[i]);
            let t = ((v - a) / (b - a)).clamp(0.0, 1.0);
            let mut o = [0u8; 4];
            for k in 0..4 {
                o[k] = (f32::from(ca[k]) + (f32::from(cb[k]) - f32::from(ca[k])) * t).round() as u8;
            }
            o
        }
    };
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// Colour the whole world, rows evenly spaced in Mercator.
pub fn render(field: ModelField, run: i64, step: u32, grid: Field) -> ModelMap {
    let merc = |lat: f64| {
        (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0)
            .tan()
            .ln()
    };
    let (my0, my1) = (merc(MAX_LAT), merc(-MAX_LAT));
    let h = W; // the world is square in Mercator at ±85.05°
    let mut vals = vec![f32::NAN; W * h];
    for y in 0..h {
        let my = my0 + (my1 - my0) * (y as f64 + 0.5) / h as f64;
        let lat = (2.0 * my.exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees();
        for x in 0..W {
            let lon = -180.0 + (x as f64 + 0.5) * 360.0 / W as f64;
            vals[y * W + x] = grid.bilinear(lat, lon).unwrap_or(f32::NAN);
        }
    }
    let mut px: Vec<Color32> = vals.iter().map(|v| color(field, *v)).collect();
    if let Some(step_v) = isoline(field) {
        let line = Color32::from_rgba_unmultiplied(20, 20, 30, 210);
        let band = |v: f32| (v / step_v).floor() as i32;
        for y in 0..h - 1 {
            for x in 0..W - 1 {
                let v = vals[y * W + x];
                if !v.is_finite() {
                    continue;
                }
                let (r, d) = (vals[y * W + x + 1], vals[(y + 1) * W + x]);
                if (r.is_finite() && band(r) != band(v)) || (d.is_finite() && band(d) != band(v)) {
                    px[y * W + x] = line;
                }
            }
        }
    }
    let _ = run;
    ModelMap {
        field,
        step,
        image: ColorImage::new([W, h], px),
        bounds: [-180.0, -MAX_LAT, 180.0, MAX_LAT],
        grid,
    }
}

/// A value as people read it, in their units.
pub fn describe(field: ModelField, v: f32, units: UnitPref) -> String {
    let metric = units == UnitPref::C;
    let speed = |ms: f32| {
        if metric {
            format!("{:.0} km/h", ms * 3.6)
        } else {
            format!("{:.0} mph", ms * 2.237)
        }
    };
    match field {
        ModelField::Temperature => {
            if metric {
                format!("{v:.1} °C")
            } else {
                format!("{:.0} °F", v * 9.0 / 5.0 + 32.0)
            }
        }
        ModelField::Precipitation => {
            if metric {
                format!("{v:.1} mm/h")
            } else {
                format!("{:.2} in/h", v / 25.4)
            }
        }
        ModelField::Wind => format!("wind {}", speed(v)),
        ModelField::Gusts => format!("gusts {}", speed(v)),
        ModelField::Pressure => format!("{v:.0} hPa"),
        ModelField::Cape => format!("{v:.0} J/kg"),
        ModelField::Cloud => format!("{v:.0}% cloud"),
        ModelField::Height500 => format!("{v:.0} dam"),
    }
}

/// Colour-strip range and label for the legend.
pub fn legend(field: ModelField, units: UnitPref) -> (f32, f32, String) {
    let st = stops(field);
    let (lo, hi) = (st[0].0, st[st.len() - 1].0);
    let text = format!(
        "{} … {}",
        describe(field, lo, units),
        describe(field, hi, units)
    );
    (lo, hi, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(v: f32) -> Field {
        Field {
            ni: 1440,
            nj: 721,
            lat0: 90.0,
            lon0: -180.0,
            dlat: 0.25,
            dlon: 0.25,
            values: vec![v; 1440 * 721],
        }
    }

    #[test]
    fn combines_and_converts() {
        let t = combine(ModelField::Temperature, &[grid(293.15)]).unwrap();
        assert!((t.values[0] - 20.0).abs() < 1e-4);
        let w = combine(ModelField::Wind, &[grid(3.0), grid(4.0)]).unwrap();
        assert!((w.values[5] - 5.0).abs() < 1e-5);
        let c = combine(ModelField::Cloud, &[grid(0.5)]).unwrap();
        assert!((c.values[0] - 50.0).abs() < 1e-4);
        assert!(combine(ModelField::Wind, &[grid(1.0)]).is_none());
    }

    #[test]
    fn colours_and_text() {
        assert_eq!(color(ModelField::Precipitation, 0.01), Color32::TRANSPARENT);
        assert!(color(ModelField::Precipitation, 5.0).a() > 0);
        assert!(color(ModelField::Temperature, -60.0).a() > 0);
        assert_eq!(
            describe(ModelField::Temperature, 20.0, UnitPref::F),
            "68 °F"
        );
        assert_eq!(
            describe(ModelField::Wind, 10.0, UnitPref::C),
            "wind 36 km/h"
        );
    }

    #[test]
    fn draws_isobars() {
        // A pressure field rising west to east crosses 4 hPa bands.
        let mut g = grid(1000.0);
        for (i, v) in g.values.iter_mut().enumerate() {
            *v = 990.0 + (i % 1440) as f32 * 0.02;
        }
        let m = render(ModelField::Pressure, 0, 0, g);
        let line = Color32::from_rgba_unmultiplied(20, 20, 30, 210);
        let dark = m.image.pixels.iter().filter(|c| **c == line).count();
        assert!(dark > 1000, "{dark}");
    }
}
