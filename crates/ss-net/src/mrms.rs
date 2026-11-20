//! NOAA Multi-Radar Multi-Sensor (MRMS) grids over the US from NOAA's open
//! bucket on AWS: hail size (MESH) and rotation tracks, the maximum over the
//! last hour(s). Files are gzipped GRIB2 with PNG packing on a regular
//! lat/lon grid, decoded here directly (just what MRMS uses).

use std::io::Read;

use crate::{Net, NetError, Result};

const BUCKET: &str = "https://noaa-mrms-pds.s3.amazonaws.com";

/// A field the app shows as a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackKind {
    /// Maximum Estimated Size of Hail (mm).
    Hail,
    /// Rotation track: max low-level (0–2 km) azimuthal shear (s⁻¹).
    Rotation,
    /// Worldwide stand-in for hail swaths, computed by the app (not MRMS):
    /// the maximum composite reflectivity over the period (dBZ).
    Swath,
}

/// Roughly where MRMS covers (the US and southern Canada).
pub fn covers(lat: f64, lon: f64) -> bool {
    (20.0..=55.0).contains(&lat) && (-130.0..=-60.0).contains(&lon)
}

impl TrackKind {
    /// The MRMS product folder for the maximum over `minutes`
    /// (30, 60, 120, 240, 360 or 1440).
    pub fn product(self, minutes: u32) -> String {
        match self {
            TrackKind::Hail => format!("MESH_Max_{minutes}min_00.50"),
            TrackKind::Rotation => format!("RotationTrack{minutes}min_00.50"),
            TrackKind::Swath => String::new(),
        }
    }
}

/// The periods MRMS publishes maxima for, in minutes.
pub const PERIODS: [u32; 6] = [30, 60, 120, 240, 360, 1440];

/// A regular lat/lon grid, rows north to south, columns west to east.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// Centre of the first (north-west) cell.
    pub lat0: f64,
    pub lon0: f64,
    pub dlat: f64,
    pub dlon: f64,
    pub nx: usize,
    pub ny: usize,
    /// Row-major; NaN where there's no data.
    pub values: Vec<f32>,
    /// Unix seconds.
    pub time: i64,
}

impl Grid {
    /// The value at a point, or None outside the grid / without data.
    pub fn at(&self, lat: f64, lon: f64) -> Option<f32> {
        let r = ((self.lat0 - lat) / self.dlat).round();
        let c = ((lon - self.lon0) / self.dlon).round();
        if r < 0.0 || c < 0.0 || r >= self.ny as f64 || c >= self.nx as f64 {
            return None;
        }
        let v = self.values[r as usize * self.nx + c as usize];
        v.is_finite().then_some(v)
    }

    /// [west, south, east, north] of the cell edges.
    pub fn bounds(&self) -> [f64; 4] {
        [
            self.lon0 - self.dlon / 2.0,
            self.lat0 - (self.ny as f64 - 0.5) * self.dlat,
            self.lon0 + (self.nx as f64 - 0.5) * self.dlon,
            self.lat0 + self.dlat / 2.0,
        ]
    }
}

fn bad(m: impl Into<String>) -> NetError {
    NetError::Parse(format!("MRMS: {}", m.into()))
}

fn u32_at(b: &[u8], i: usize) -> Result<u32> {
    b.get(i..i + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| bad("truncated"))
}

/// GRIB's sign-and-magnitude integers.
fn signed32(v: u32) -> i64 {
    let m = i64::from(v & 0x7fff_ffff);
    if v & 0x8000_0000 != 0 {
        -m
    } else {
        m
    }
}

fn signed16(b: &[u8]) -> i32 {
    let v = u16::from_be_bytes([b[0], b[1]]);
    let m = i32::from(v & 0x7fff);
    if v & 0x8000 != 0 {
        -m
    } else {
        m
    }
}

/// Decode an MRMS GRIB2 message (gunzipped), keeping every `factor`-th
/// row and column as the maximum of the block (so narrow tracks survive).
/// Values below zero (MRMS's "no coverage" / "missing") become NaN.
pub fn decode(grib: &[u8], factor: usize, time: i64) -> Result<Grid> {
    if grib.get(0..4) != Some(b"GRIB") || grib.get(7) != Some(&2) {
        return Err(bad("not GRIB2"));
    }
    let factor = factor.max(1);
    let mut i = 16;
    let (mut ni, mut nj) = (0usize, 0usize);
    let (mut la1, mut lo1, mut di, mut dj, mut scan) = (0.0, 0.0, 0.0, 0.0, 0u8);
    let (mut r, mut e, mut d) = (0f32, 0i32, 0i32);
    let mut png_data: Option<&[u8]> = None;
    while i + 5 <= grib.len() {
        if grib.get(i..i + 4) == Some(b"7777") {
            break;
        }
        let len = u32_at(grib, i)? as usize;
        let sec = grib
            .get(i..i + len)
            .ok_or_else(|| bad("truncated section"))?;
        if len < 5 {
            return Err(bad("bad section"));
        }
        match sec[4] {
            3 => {
                let tmpl = u16::from_be_bytes([sec[12], sec[13]]);
                if tmpl != 0 || sec.len() < 72 {
                    return Err(bad(format!("grid template {tmpl}")));
                }
                ni = u32_at(sec, 30)? as usize;
                nj = u32_at(sec, 34)? as usize;
                la1 = signed32(u32_at(sec, 46)?) as f64 * 1e-6;
                lo1 = signed32(u32_at(sec, 50)?) as f64 * 1e-6;
                di = u32_at(sec, 63)? as f64 * 1e-6;
                dj = u32_at(sec, 67)? as f64 * 1e-6;
                scan = sec[71];
            }
            5 => {
                let tmpl = u16::from_be_bytes([sec[9], sec[10]]);
                if tmpl != 41 || sec.len() < 20 {
                    return Err(bad(format!("packing template {tmpl}")));
                }
                r = f32::from_be_bytes([sec[11], sec[12], sec[13], sec[14]]);
                e = signed16(&sec[15..17]);
                d = signed16(&sec[17..19]);
            }
            6 if sec.get(5).copied() != Some(255) => return Err(bad("bitmaps unsupported")),
            7 => png_data = Some(&sec[5..]),
            _ => {}
        }
        i += len;
    }
    let png_data = png_data.ok_or_else(|| bad("no data section"))?;
    if ni == 0 || nj == 0 {
        return Err(bad("no grid"));
    }
    // Rows run north to south, columns west to east (MRMS's layout).
    if scan & 0b1110_0000 != 0 {
        return Err(bad(format!("scanning mode {scan:#x}")));
    }
    let lon0 = if lo1 > 180.0 { lo1 - 360.0 } else { lo1 };

    let mut dec = png::Decoder::new(std::io::Cursor::new(png_data));
    dec.set_transformations(png::Transformations::IDENTITY);
    let mut reader = dec.read_info().map_err(|e| bad(e.to_string()))?;
    let info = reader.info();
    let (w, h) = (info.width as usize, info.height as usize);
    if w != ni || h != nj {
        return Err(bad("image size doesn't match the grid"));
    }
    let bpp = info.bytes_per_pixel();
    if !(1..=4).contains(&bpp) {
        return Err(bad("pixel format"));
    }
    let (two_e, ten_d) = (2f32.powi(e), 10f32.powi(-d));
    let (nx, ny) = (ni.div_ceil(factor), nj.div_ceil(factor));
    let mut values = vec![f32::NAN; nx * ny];
    let mut row_i = 0usize;
    while let Some(row) = reader.next_row().map_err(|e| bad(e.to_string()))? {
        let data = row.data();
        let out = &mut values[(row_i / factor) * nx..][..nx];
        for (x, px) in data.chunks_exact(bpp).take(ni).enumerate() {
            let raw = px.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b));
            let v = (r + raw as f32 * two_e) * ten_d;
            if v >= 0.0 {
                let o = &mut out[x / factor];
                if o.is_nan() || *o < v {
                    *o = v;
                }
            }
        }
        row_i += 1;
    }
    let f = factor as f64;
    Ok(Grid {
        lat0: la1 - (f - 1.0) / 2.0 * dj,
        lon0: lon0 + (f - 1.0) / 2.0 * di,
        dlat: dj * f,
        dlon: di * f,
        nx,
        ny,
        values,
        time,
    })
}

/// Time of an MRMS file from its name (`…_20261120-093000.grib2.gz`).
pub fn file_time(key: &str) -> Option<i64> {
    let s = key.rsplit('_').next()?.get(0..15)?;
    let t = chrono::NaiveDateTime::parse_from_str(s, "%Y%m%d-%H%M%S").ok()?;
    Some(t.and_utc().timestamp())
}

impl Net {
    /// The newest grid of a CONUS MRMS product, reduced by `factor`.
    pub async fn mrms_latest(&self, product: &str, factor: usize) -> Result<Grid> {
        let now = chrono::Utc::now();
        let mut key = None;
        for day in [now, now - chrono::Duration::days(1)] {
            let url = format!(
                "{BUCKET}/?list-type=2&prefix=CONUS/{product}/{}/",
                day.format("%Y%m%d")
            );
            let xml = self.get_text(&url, None).await?;
            if let Some(k) = crate::level2::parse_listing_any(&xml)
                .into_iter()
                .filter(|k| k.ends_with(".grib2.gz"))
                .max()
            {
                key = Some(k);
                break;
            }
        }
        let key = key.ok_or_else(|| bad(format!("no recent {product} files")))?;
        let time = file_time(&key).unwrap_or(0);
        let gz = self.get_bytes(&format!("{BUCKET}/{key}"), None).await?;
        tokio::task::spawn_blocking(move || {
            let mut raw = Vec::new();
            flate2::read::GzDecoder::new(&gz[..])
                .read_to_end(&mut raw)
                .map_err(|e| bad(e.to_string()))?;
            decode(&raw, factor, time)
        })
        .await
        .map_err(|e| bad(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny GRIB2 message: a 4×2 grid, 16-bit PNG packing, R = -30,
    /// E = 0, D = 1 (values = (raw - 30) / 10).
    fn sample() -> Vec<u8> {
        let raw: [u16; 8] = [0, 30, 40, 80, 20, 30, 30, 530];
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 4, 2);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Sixteen);
            let mut w = enc.write_header().unwrap();
            let data: Vec<u8> = raw.iter().flat_map(|v| v.to_be_bytes()).collect();
            w.write_image_data(&data).unwrap();
        }
        let mut s3 = vec![0u8; 72];
        s3[4] = 3;
        s3[30..34].copy_from_slice(&4u32.to_be_bytes());
        s3[34..38].copy_from_slice(&2u32.to_be_bytes());
        s3[46..50].copy_from_slice(&40_000_000u32.to_be_bytes());
        s3[50..54].copy_from_slice(&260_000_000u32.to_be_bytes());
        s3[63..67].copy_from_slice(&10_000u32.to_be_bytes());
        s3[67..71].copy_from_slice(&10_000u32.to_be_bytes());
        let mut s5 = vec![0u8; 21];
        s5[4] = 5;
        s5[10] = 41;
        s5[11..15].copy_from_slice(&(-30.0f32).to_be_bytes());
        s5[17..19].copy_from_slice(&1u16.to_be_bytes());
        s5[19] = 16;
        let s6 = vec![0, 0, 0, 6, 6, 255];
        let mut s7 = vec![0u8, 0, 0, 0, 7];
        s7.extend(&png_bytes);
        let mut body = Vec::new();
        for s in [&mut s3, &mut s5, &mut s7] {
            let n = s.len() as u32;
            s[0..4].copy_from_slice(&n.to_be_bytes());
        }
        for s in [&s3, &s5, &s6, &s7] {
            body.extend_from_slice(s);
        }
        let mut g = b"GRIB\0\0\0\x02".to_vec();
        g.extend(((16 + body.len() + 4) as u64).to_be_bytes());
        g.extend(body);
        g.extend(b"7777");
        g
    }

    #[test]
    fn decodes_png_packed_grib2() {
        let g = decode(&sample(), 1, 7).unwrap();
        assert_eq!((g.nx, g.ny, g.time), (4, 2, 7));
        assert!((g.lat0 - 40.0).abs() < 1e-9 && (g.lon0 + 100.0).abs() < 1e-9);
        assert!(g.values[0].is_nan()); // -0.3: no data
        assert_eq!(g.values[1], 0.0);
        assert!((g.values[3] - 5.0).abs() < 1e-5);
        assert!((g.values[7] - 50.0).abs() < 1e-4);
        assert_eq!(g.at(40.0, -99.97), Some(5.0));
        assert_eq!(g.at(41.0, -99.97), None);
    }

    #[test]
    fn reduces_by_the_block_maximum() {
        let g = decode(&sample(), 2, 0).unwrap();
        assert_eq!((g.nx, g.ny), (2, 1));
        assert_eq!(g.values[0], 0.0);
        assert!((g.values[1] - 50.0).abs() < 1e-4);
        assert!((g.lat0 - 39.995).abs() < 1e-9 && (g.lon0 + 99.995).abs() < 1e-9);
    }

    #[test]
    fn reads_file_times() {
        let t = file_time("CONUS/MESH_Max_60min_00.50/20261120/MRMS_MESH_Max_60min_00.50_20261120-093000.grib2.gz");
        assert_eq!(t, Some(1_795_167_000));
    }

    /// A real file: `SS_MRMS_SAMPLE=/path/to/MRMS_….grib2` (gunzipped).
    #[test]
    fn reads_a_real_file_when_available() {
        let Ok(path) = std::env::var("SS_MRMS_SAMPLE") else {
            return;
        };
        let g = decode(&std::fs::read(path).unwrap(), 4, 0).unwrap();
        let [w, s, e, n] = g.bounds();
        let mut v: Vec<f32> = g.values.iter().copied().filter(|v| *v > 0.0).collect();
        v.sort_by(f32::total_cmp);
        let q = |f: f64| v.get((v.len() as f64 * f) as usize).copied();
        println!(
            "quantiles 50/90/99/99.9: {:?}",
            [q(0.5), q(0.9), q(0.99), q(0.999)]
        );
        println!(
            "{}x{} {w:.2},{s:.2},{e:.2},{n:.2} max {:?} cells {}",
            g.nx,
            g.ny,
            g.values
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .fold(0f32, f32::max),
            g.values.iter().filter(|v| **v > 0.0).count()
        );
        assert!(n > 50.0 && s < 25.0 && w < -125.0 && e > -65.0);
    }
}
