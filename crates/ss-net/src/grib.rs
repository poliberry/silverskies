//! A small GRIB2 reader for regular lat/lon fields: simple packing (5.0),
//! PNG (5.41) and CCSDS/AEC (5.42, what ECMWF's open data uses, decoded
//! with the pure-Rust AEC decoder in `hdf5-reader`).

use crate::{NetError, Result};

/// One field on a regular lat/lon grid, rows north to south.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub ni: usize,
    pub nj: usize,
    /// First (north-west) point.
    pub lat0: f64,
    pub lon0: f64,
    pub dlat: f64,
    pub dlon: f64,
    /// Row-major; NaN where the bitmap says no data.
    pub values: Vec<f32>,
}

impl Field {
    /// The value at a point, interpolated between the four surrounding grid
    /// points (longitude wraps; NaN neighbours fall back to the nearest).
    pub fn bilinear(&self, lat: f64, lon: f64) -> Option<f32> {
        let fy = (self.lat0 - lat) / self.dlat;
        if fy < 0.0 || fy > (self.nj - 1) as f64 {
            return None;
        }
        let fx = (lon - self.lon0).rem_euclid(360.0) / self.dlon;
        let (y0, x0) = (fy.floor() as usize, fx.floor() as usize);
        let (ty, tx) = ((fy - y0 as f64) as f32, (fx - x0 as f64) as f32);
        let y1 = (y0 + 1).min(self.nj - 1);
        let ni = self.ni.max(1);
        let (xa, xb) = (x0 % ni, (x0 + 1) % ni);
        let v = |y: usize, x: usize| self.values[y * ni + x];
        let (a, b, c, d) = (v(y0, xa), v(y0, xb), v(y1, xa), v(y1, xb));
        if !(a.is_finite() && b.is_finite() && c.is_finite() && d.is_finite()) {
            return self.at(lat, lon);
        }
        let top = a + (b - a) * tx;
        let bot = c + (d - c) * tx;
        Some(top + (bot - top) * ty)
    }

    /// The value nearest a point (longitude wraps).
    pub fn at(&self, lat: f64, lon: f64) -> Option<f32> {
        let r = ((self.lat0 - lat) / self.dlat).round();
        if r < 0.0 || r >= self.nj as f64 {
            return None;
        }
        let c = ((lon - self.lon0).rem_euclid(360.0) / self.dlon).round() as usize % self.ni.max(1);
        let v = self.values[r as usize * self.ni + c];
        v.is_finite().then_some(v)
    }
}

fn bad(m: impl std::fmt::Display) -> NetError {
    NetError::Parse(format!("GRIB: {m}"))
}

fn u32_at(b: &[u8], i: usize) -> Result<u32> {
    b.get(i..i + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| bad("truncated"))
}

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

/// Unpack `n` big-endian `bits`-wide integers from a bit stream.
fn unpack_bits(data: &[u8], bits: u32, n: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(n);
    let (mut acc, mut have, mut pos) = (0u64, 0u32, 0usize);
    for _ in 0..n {
        while have < bits {
            acc = (acc << 8) | u64::from(*data.get(pos).unwrap_or(&0));
            pos += 1;
            have += 8;
        }
        have -= bits;
        out.push(((acc >> have) & ((1u64 << bits) - 1)) as u32);
    }
    out
}

/// Decode the first message of a GRIB2 file.
pub fn decode(grib: &[u8]) -> Result<Field> {
    if grib.get(0..4) != Some(b"GRIB") || grib.get(7) != Some(&2) {
        return Err(bad("not GRIB2"));
    }
    let mut i = 16;
    let (mut ni, mut nj) = (0usize, 0usize);
    let (mut la1, mut lo1, mut di, mut dj, mut scan) = (0.0, 0.0, 0.0, 0.0, 0u8);
    let (mut tmpl, mut r, mut e, mut d, mut bits) = (u16::MAX, 0f32, 0i32, 0i32, 0u32);
    let (mut ccsds_flags, mut block, mut rsi) = (0u8, 0u32, 0u32);
    let mut npoints = 0usize;
    let mut bitmap: Option<&[u8]> = None;
    let mut data: Option<&[u8]> = None;
    while i + 5 <= grib.len() {
        if grib.get(i..i + 4) == Some(b"7777") {
            break;
        }
        let len = u32_at(grib, i)? as usize;
        if len < 5 {
            return Err(bad("bad section"));
        }
        let sec = grib
            .get(i..i + len)
            .ok_or_else(|| bad("truncated section"))?;
        match sec[4] {
            3 => {
                let t = u16::from_be_bytes([sec[12], sec[13]]);
                if t != 0 || sec.len() < 72 {
                    return Err(bad(format!("grid template {t}")));
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
                npoints = u32_at(sec, 5)? as usize;
                tmpl = u16::from_be_bytes([sec[9], sec[10]]);
                if sec.len() < 21 {
                    return Err(bad("short packing section"));
                }
                r = f32::from_be_bytes([sec[11], sec[12], sec[13], sec[14]]);
                e = signed16(&sec[15..17]);
                d = signed16(&sec[17..19]);
                bits = u32::from(sec[19]);
                if tmpl == 42 && sec.len() >= 25 {
                    ccsds_flags = sec[21];
                    block = u32::from(sec[22]);
                    rsi = u32::from(u16::from_be_bytes([sec[23], sec[24]]));
                }
            }
            6 => match sec[5] {
                255 => {}
                0 => bitmap = Some(&sec[6..]),
                o => return Err(bad(format!("bitmap indicator {o}"))),
            },
            7 => data = Some(&sec[5..]),
            _ => {}
        }
        i += len;
    }
    let data = data.ok_or_else(|| bad("no data section"))?;
    if ni == 0 || nj == 0 {
        return Err(bad("no grid"));
    }
    if scan & 0b1110_0000 != 0 {
        return Err(bad(format!("scanning mode {scan:#x}")));
    }
    let (two_e, ten_d) = (2f32.powi(e), 10f32.powi(-d));
    // The packed values (only where the bitmap has data).
    let raw: Vec<u32> = if bits == 0 {
        vec![0; npoints]
    } else {
        match tmpl {
            0 => unpack_bits(data, bits, npoints),
            41 => {
                let img = image::load_from_memory(data).map_err(|e| bad(e.to_string()))?;
                let bpp = (bits as usize).div_ceil(8);
                let bytes: Vec<u8> = match bpp {
                    1 => img.to_luma8().into_raw(),
                    2 => img
                        .to_luma16()
                        .into_raw()
                        .iter()
                        .flat_map(|v| v.to_be_bytes())
                        .collect(),
                    _ => img.to_rgba8().into_raw(),
                };
                let step = bytes.len() / npoints.max(1);
                bytes
                    .chunks_exact(step.max(1))
                    .take(npoints)
                    .map(|c| c.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b)))
                    .collect()
            }
            42 => {
                // libaec flags: 1 signed, 2 three-byte, 4 MSB first,
                // 8 preprocess. HDF5's szip options: 16 MSB, 32 preprocess.
                let mut options = 0u32;
                if ccsds_flags & 4 != 0 {
                    options |= 16;
                }
                if ccsds_flags & 8 != 0 {
                    options |= 32;
                }
                let bytes_per = match bits {
                    1..=8 => 1usize,
                    9..=16 => 2,
                    _ => 4,
                };
                let out_len = npoints * bytes_per;
                let mut framed = (out_len as u32).to_le_bytes().to_vec();
                framed.extend_from_slice(data);
                let bytes = hdf5_reader::filters::szip::decompress_with_limit(
                    &framed,
                    &[options, block, bits, rsi * block],
                    out_len,
                )
                .map_err(|e| bad(e.to_string()))?;
                let msb = ccsds_flags & 4 != 0;
                bytes
                    .chunks_exact(bytes_per)
                    .map(|c| {
                        if msb {
                            c.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b))
                        } else {
                            c.iter().rev().fold(0u32, |a, &b| (a << 8) | u32::from(b))
                        }
                    })
                    .collect()
            }
            t => return Err(bad(format!("packing template {t}"))),
        }
    };
    let n = ni * nj;
    let mut values = vec![f32::NAN; n];
    let mut k = 0usize;
    for (idx, v) in values.iter_mut().enumerate() {
        let present = bitmap.is_none_or(|bm| {
            bm.get(idx / 8)
                .is_some_and(|b| b & (0x80 >> (idx % 8)) != 0)
        });
        if present {
            if let Some(&x) = raw.get(k) {
                *v = (r + x as f32 * two_e) * ten_d;
            }
            k += 1;
        }
    }
    Ok(Field {
        ni,
        nj,
        lat0: la1,
        lon0: if lo1 > 180.0 { lo1 - 360.0 } else { lo1 },
        dlat: dj,
        dlon: di,
        values,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_between_points() {
        let f = Field {
            ni: 4,
            nj: 2,
            lat0: 10.0,
            lon0: 0.0,
            dlat: 1.0,
            dlon: 1.0,
            values: vec![0.0, 10.0, 20.0, 30.0, 0.0, 10.0, 20.0, 30.0],
        };
        assert!((f.bilinear(9.5, 0.5).unwrap() - 5.0).abs() < 1e-5);
        // Wraps: between the last column (30) and the first (0).
        assert!((f.bilinear(10.0, 3.5).unwrap() - 15.0).abs() < 1e-5);
        assert_eq!(f.bilinear(20.0, 0.0), None);
    }

    #[test]
    fn unpacks_bit_streams() {
        // 3-bit values 1, 2, 7, 0 → 001 010 111 000 → 0x2B 0x80.
        assert_eq!(unpack_bits(&[0x2B, 0x80], 3, 4), vec![1, 2, 7, 0]);
    }

    /// Real ECMWF messages: `SS_GRIB_SAMPLES=/dir` of `.grib2` files.
    #[test]
    fn reads_real_files_when_available() {
        let Ok(dir) = std::env::var("SS_GRIB_SAMPLES") else {
            return;
        };
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let t = std::time::Instant::now();
            let f = decode(&std::fs::read(e.path()).unwrap()).unwrap();
            let fin: Vec<f32> = f.values.iter().copied().filter(|v| v.is_finite()).collect();
            let (lo, hi) = fin
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
            println!(
                "{:?}: {}x{} lat0 {} lon0 {} min {lo} max {hi} london {:?} in {:?}",
                e.file_name(),
                f.ni,
                f.nj,
                f.lat0,
                f.lon0,
                f.at(51.5, -0.1),
                t.elapsed()
            );
        }
    }
}
