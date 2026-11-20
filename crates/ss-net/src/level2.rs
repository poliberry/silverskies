//! NEXRAD Level II volume scans from the public NOAA archive on AWS
//! (`unidata-nexrad-level2`, free, no key): find a site's newest volume,
//! download it, and decode its reflectivity sweeps.
//!
//! Format (WSR-88D ICD 2620010): a 24-byte volume header, then records,
//! each a 4-byte size and a bzip2 stream. Decompressed, the records hold
//! messages: a 12-byte channel header, a 16-byte message header, then the
//! body. Message 31 ("generic radial") carries one radial with data blocks
//! (`REF`, `VEL`, …) found through pointers; other messages are fixed
//! 2432-byte records.

use std::io::Read;

use crate::{Net, NetError, Result};
use ss_core::radar3d::{Moment, MomentData, Radial, Sweep, Volume};

pub const BUCKET: &str = "https://unidata-nexrad-level2.s3.amazonaws.com";

/// S3 listing URL for a site's volumes on a UTC day.
pub fn list_url(site: &str, date: chrono::NaiveDate) -> String {
    format!(
        "{BUCKET}/?list-type=2&prefix={}/{site}/",
        date.format("%Y/%m/%d")
    )
}

/// Object keys in an S3 `ListBucketResult`, volume files only (the `_MDM`
/// metadata files are skipped).
pub fn parse_listing(xml: &str) -> Vec<String> {
    xml.split("<Key>")
        .skip(1)
        .filter_map(|s| s.split("</Key>").next())
        .filter(|k| !k.ends_with("_MDM"))
        .map(str::to_string)
        .collect()
}

fn be_u16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
fn be_i16(b: &[u8], i: usize) -> Option<i16> {
    Some(i16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
fn be_u32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn be_f32(b: &[u8], i: usize) -> Option<f32> {
    Some(f32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Concatenate the bzip2 records after the 24-byte volume header.
pub fn decompress(file: &[u8]) -> Result<Vec<u8>> {
    if file.len() < 28 || !file.starts_with(b"AR2V") {
        return Err(NetError::Parse("not a NEXRAD Level II file".into()));
    }
    let mut out = Vec::with_capacity(file.len() * 7);
    let mut pos = 24;
    while pos + 4 <= file.len() {
        let size = i32::from_be_bytes(file[pos..pos + 4].try_into().unwrap_or_default());
        pos += 4;
        let n = size.unsigned_abs() as usize;
        if n == 0 || pos + n > file.len() {
            break;
        }
        bzip2::read::BzDecoder::new(&file[pos..pos + n])
            .read_to_end(&mut out)
            .map_err(|e| NetError::Parse(format!("Level II record: {e}")))?;
        pos += n;
    }
    Ok(out)
}

/// One message-31 radial: (elevation number, elevation°, radial, site
/// lat/lon/height when the volume block is present).
type Parsed = (u8, f32, Radial, Option<(f64, f64, f64)>);

fn parse_m31(b: &[u8]) -> Option<Parsed> {
    let azimuth = be_f32(b, 12)?;
    let elev_num = *b.get(22)?;
    let elevation = be_f32(b, 24)?;
    let count = be_u16(b, 30)? as usize;
    let mut refl = None;
    let mut moments = Vec::new();
    let mut site = None;
    for k in 0..count.min(10) {
        let ptr = be_u32(b, 32 + 4 * k)? as usize;
        let name = b.get(ptr..ptr + 4)?;
        match name {
            b"RVOL" => {
                site = Some((
                    f64::from(be_f32(b, ptr + 8)?),
                    f64::from(be_f32(b, ptr + 12)?),
                    f64::from(be_i16(b, ptr + 16)?) + f64::from(be_u16(b, ptr + 18)?),
                ));
            }
            b"DREF" | b"DVEL" | b"DZDR" | b"DRHO" => {
                let ngates = be_u16(b, ptr + 8)? as usize;
                let first = f32::from(be_i16(b, ptr + 10)?) / 1000.0;
                let spacing = f32::from(be_i16(b, ptr + 12)?) / 1000.0;
                let word = *b.get(ptr + 19)?;
                let scale = be_f32(b, ptr + 20)?;
                let offset = be_f32(b, ptr + 24)?;
                if scale == 0.0 || !(word == 8 || word == 16) {
                    continue;
                }
                let bytes = usize::from(word / 8);
                let raw = b.get(ptr + 28..ptr + 28 + ngates * bytes)?;
                // 0 = below threshold, 1 = range folded.
                let values = raw.chunks_exact(bytes).map(|c| {
                    let r = if bytes == 1 {
                        f32::from(c[0])
                    } else {
                        f32::from(u16::from_be_bytes([c[0], c[1]]))
                    };
                    if r < 2.0 {
                        f32::NAN
                    } else {
                        (r - offset) / scale
                    }
                });
                let moment = match name {
                    b"DVEL" => Moment::Vel,
                    b"DZDR" => Moment::Zdr,
                    b"DRHO" => Moment::Rho,
                    _ => Moment::Ref,
                };
                if moment == Moment::Ref {
                    refl = Some((first, spacing, values.collect::<Vec<f32>>()));
                } else {
                    moments.push(MomentData {
                        moment,
                        first_gate_km: first,
                        gate_km: spacing,
                        values: values
                            .map(|v| if v.is_nan() { 0 } else { moment.encode(v) })
                            .collect(),
                    });
                }
            }
            _ => {}
        }
    }
    let (first, spacing, dbz) = refl?;
    let radial = Radial {
        azimuth_deg: azimuth,
        elevation_deg: elevation,
        first_gate_km: first,
        gate_km: spacing,
        dbz,
        moments,
    };
    Some((elev_num, elevation, radial, site))
}

fn elevations_sum(s: &Sweep) -> f32 {
    s.radials.iter().map(|r| r.elevation_deg).sum()
}

/// Decode a decompressed volume. Walks the messages, re-synchronising on
/// the site id if a record boundary is off, and keeps one reflectivity
/// sweep per elevation number.
pub fn parse_volume(buf: &[u8], site: &str) -> Result<Volume> {
    let id = site.as_bytes();
    let mut sweeps: Vec<(u8, Sweep)> = Vec::new();
    let mut loc = None;
    let mut first_time: Option<(u16, u32)> = None;
    let mut p = 0usize;
    while p + 28 + 64 <= buf.len() {
        let typ = buf[p + 15];
        let size = be_u16(buf, p + 12).unwrap_or(0) as usize;
        if typ == 31 && size > 0 && buf.get(p + 28..p + 32) == Some(id) {
            let body = &buf[p + 28..(p + 12 + size * 2).min(buf.len())];
            if first_time.is_none() {
                first_time = Some((be_u16(body, 8).unwrap_or(0), be_u32(body, 4).unwrap_or(0)));
            }
            if let Some((num, el, radial, s)) = parse_m31(body) {
                if loc.is_none() {
                    loc = s;
                }
                match sweeps.iter_mut().find(|(n, _)| *n == num) {
                    Some((_, sw)) => sw.radials.push(radial),
                    None => sweeps.push((
                        num,
                        Sweep {
                            elevation_deg: el,
                            radials: vec![radial],
                        },
                    )),
                }
            }
            p += 12 + size * 2;
            continue;
        }
        if typ != 31 && (1..=33).contains(&typ) {
            p += 2432;
            continue;
        }
        // Lost: find the next message-31 header (site id 28 bytes in).
        let next = buf[p + 1..]
            .windows(4)
            .enumerate()
            .filter(|(_, w)| *w == id)
            .map(|(i, _)| p + 1 + i)
            .find(|&i| i >= 28 && buf[i - 28 + 15] == 31);
        match next {
            Some(i) => p = i - 28,
            None => break,
        }
    }
    let (lat, lon, height) =
        loc.ok_or_else(|| NetError::Parse("Level II volume has no site location".into()))?;
    if sweeps.is_empty() {
        return Err(NetError::Parse(
            "Level II volume has no reflectivity".into(),
        ));
    }
    // Date: days since 1970-01-01 (day 1 = 1 Jan 1970); ms after midnight UTC.
    let time = first_time
        .and_then(|(days, ms)| {
            let secs = (i64::from(days) - 1) * 86_400 + i64::from(ms / 1000);
            chrono::DateTime::from_timestamp(secs, 0)
        })
        .map(|t| t.to_rfc3339())
        .unwrap_or_default();
    // Each radial carries the antenna's elevation; the first of a sweep can
    // still be settling, so use the sweep's mean.
    let mut sweeps: Vec<Sweep> = sweeps
        .into_iter()
        .map(|(_, mut s)| {
            let n = s.radials.len().max(1) as f32;
            s.elevation_deg = elevations_sum(&s) / n;
            s
        })
        .collect();
    sweeps.sort_by(|a, b| a.elevation_deg.total_cmp(&b.elevation_deg));
    Ok(Volume {
        site: site.to_string(),
        lat,
        lon,
        height_m: height,
        time,
        sweeps,
    })
}

impl Net {
    /// The newest volume scan for `site` (today's, else yesterday's, UTC).
    pub async fn latest_level2(&self, site: &str) -> Result<Volume> {
        let today = chrono::Utc::now().date_naive();
        let mut key = None;
        for date in [today, today.pred_opt().unwrap_or(today)] {
            let keys = parse_listing(&self.get_text(&list_url(site, date), None).await?);
            if let Some(k) = keys.last() {
                key = Some(k.clone());
                break;
            }
        }
        let key = key.ok_or(NetError::Missing("recent volume scan for this radar"))?;
        let bytes = self.get_bytes(&format!("{BUCKET}/{key}"), None).await?;
        let site = site.to_string();
        tokio::task::spawn_blocking(move || parse_volume(&decompress(&bytes)?, &site))
            .await
            .map_err(|e| NetError::Parse(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_volume_keys() {
        let xml = "<ListBucketResult><Contents><Key>2026/11/19/KTLX/KTLX20261119_000636_V06</Key></Contents>\
            <Contents><Key>2026/11/19/KTLX/KTLX20261119_000636_V06_MDM</Key></Contents>\
            <Contents><Key>2026/11/19/KTLX/KTLX20261119_001115_V06</Key></Contents></ListBucketResult>";
        assert_eq!(
            parse_listing(xml),
            vec![
                "2026/11/19/KTLX/KTLX20261119_000636_V06",
                "2026/11/19/KTLX/KTLX20261119_001115_V06"
            ]
        );
        let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        assert!(list_url("KTLX", d).ends_with("prefix=2026/11/09/KTLX/"));
    }

    #[test]
    fn rejects_other_files() {
        assert!(decompress(b"not radar data at all, sorry").is_err());
    }

    /// A real volume: set `SS_LEVEL2_SAMPLE=/path/to/KMUX..._V06`.
    #[test]
    fn decodes_a_real_volume_when_available() {
        let Ok(path) = std::env::var("SS_LEVEL2_SAMPLE") else {
            return;
        };
        let file = std::fs::read(&path).unwrap();
        let site = std::str::from_utf8(&file[20..24]).unwrap().to_string();
        let buf = decompress(&file).unwrap();
        let v = parse_volume(&buf, &site).unwrap();
        eprintln!(
            "{} {} at {:.3},{:.3} {:.0} m: {} sweeps",
            v.site,
            v.time,
            v.lat,
            v.lon,
            v.height_m,
            v.sweeps.len()
        );
        for s in &v.sweeps {
            let gates: usize = s.radials.iter().map(|r| r.dbz.len()).sum();
            let echoes = s
                .radials
                .iter()
                .flat_map(|r| r.dbz.iter())
                .filter(|d| **d >= 20.0)
                .count();
            let max = s
                .radials
                .iter()
                .flat_map(|r| r.dbz.iter())
                .fold(f32::MIN, |a, b| a.max(*b));
            eprintln!(
                "  {:5.2}°  {} radials  {} gates  {} ≥20 dBZ  max {:.1}  gate {:.3} km from {:.3}",
                s.elevation_deg,
                s.radials.len(),
                gates,
                echoes,
                max,
                s.radials[0].gate_km,
                s.radials[0].first_gate_km
            );
            let kinds: Vec<String> = s.radials[0]
                .moments
                .iter()
                .map(|m| {
                    format!(
                        "{:?}({})",
                        m.moment,
                        m.values.iter().filter(|&&b| b > 0).count()
                    )
                })
                .collect();
            eprintln!("         moments: {}", kinds.join(" "));
        }
        let p = ss_core::radar3d::polar_volume(&v, v.lat, v.lon);
        for (m, d) in &p.moments {
            let n = d.iter().filter(|&&b| b > 0).count();
            eprintln!(
                "  polar {m:?}: {n} gates with data over {} tilts",
                p.sweeps.len()
            );
        }
        assert!(v.sweeps.len() >= 5);
        assert!((20.0..60.0).contains(&v.lat) && (-170.0..-60.0).contains(&v.lon));
        assert!(v.sweeps.iter().all(|s| s.radials.len() >= 300));
    }
}
