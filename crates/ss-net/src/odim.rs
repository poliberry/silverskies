//! European single-site radar volumes from EUMETNET OPERA's Open Radar Data
//! (CC BY 4.0): the last 24 h of scans from ~150 radars in ~20 countries,
//! in ODIM HDF5, read with a pure-Rust HDF5 decoder into the same
//! [`Volume`] as NEXRAD Level II, so every Level II view works in Europe.
//!
//! Countries send either whole volumes (`PVOL`) or one tilt per file
//! (`SCAN`), and some one quantity per file; the fields of every file of
//! the newest scan are merged by elevation.

use hdf5_reader::group::Group;
use hdf5_reader::{Dataset, Hdf5File};
use ss_core::radar3d::{Moment, MomentData, Radial, Sweep, Volume};

use crate::{Net, NetError, Result};

/// The 24 h cache (the documented backup store, which is the one being
/// filled; the primary endpoint is tried first).
const STORES: [&str; 2] = [
    "https://s3.waw3-1.cloudferro.com/openradar-24h",
    "https://object-store.os-api.cci1.ecmwf.int/openradar-24h-backup",
];

/// Files of one scan arrive over a few minutes; anything this much older
/// than the newest file belongs to the previous scan.
const SCAN_WINDOW_MIN: i64 = 6;

fn bad(m: impl std::fmt::Display) -> NetError {
    NetError::Parse(format!("ODIM: {m}"))
}

/// One quantity on one tilt, decoded to physical values (NaN = no data).
#[derive(Debug, Clone)]
pub struct Field {
    pub quantity: String,
    pub elevation_deg: f32,
    pub nrays: usize,
    pub nbins: usize,
    pub first_gate_km: f32,
    pub gate_km: f32,
    /// `nrays × nbins`, ray 0 centred half a ray east of north.
    pub values: Vec<f32>,
}

/// Site position and time from a file's root.
#[derive(Debug, Clone, Default)]
pub struct Header {
    pub lat: f64,
    pub lon: f64,
    pub height_m: f64,
    /// `YYYYMMDDTHHMMSSZ`-ish as RFC 3339.
    pub time: String,
}

fn attr_f64(g: &Group, name: &str) -> Option<f64> {
    g.attribute(name).ok()?.read_as_f64().ok()
}

fn attr_str(g: &Group, name: &str) -> Option<String> {
    let a = g.attribute(name).ok()?;
    a.read_string()
        .ok()
        .or_else(|| a.read_strings().ok().and_then(|v| v.into_iter().next()))
        .map(|s| s.trim_end_matches('\0').trim().to_string())
}

/// A `what`/`where`/`how` attribute, looked up from the innermost group
/// outwards (ODIM lets lower levels override, and upper ones supply).
fn find_f64(groups: &[&Group], sub: &str, name: &str) -> Option<f64> {
    groups
        .iter()
        .find_map(|g| g.group(sub).ok().and_then(|w| attr_f64(&w, name)))
}

fn find_str(groups: &[&Group], sub: &str, name: &str) -> Option<String> {
    groups
        .iter()
        .find_map(|g| g.group(sub).ok().and_then(|w| attr_str(&w, name)))
}

/// A dataset of any integer or float type, as f64.
fn read_numbers(d: &Dataset) -> Result<Vec<f64>> {
    macro_rules! try_as {
        ($($t:ty),*) => {
            $(
                if let Ok(a) = d.read_array::<$t>() {
                    return Ok(a.iter().map(|&v| v as f64).collect());
                }
            )*
        };
    }
    try_as!(u8, u16, i8, i16, f32, f64, u32, i32);
    Err(bad("unsupported data type"))
}

/// Decode one ODIM file: its header and every quantity on every tilt.
pub fn parse(bytes: &[u8]) -> Result<(Header, Vec<Field>)> {
    let f = Hdf5File::from_bytes(bytes).map_err(bad)?;
    let root = f.root_group().map_err(bad)?;
    let header = {
        let w = root.group("where").map_err(bad)?;
        let what = root.group("what").map_err(bad)?;
        let date = attr_str(&what, "date").unwrap_or_default();
        let time = attr_str(&what, "time").unwrap_or_default();
        Header {
            lat: attr_f64(&w, "lat").ok_or_else(|| bad("no latitude"))?,
            lon: attr_f64(&w, "lon").ok_or_else(|| bad("no longitude"))?,
            height_m: attr_f64(&w, "height").unwrap_or(0.0),
            time: rfc3339(&date, &time).unwrap_or_default(),
        }
    };
    let mut fields = Vec::new();
    let (groups, _) = root.member_names().map_err(bad)?;
    let mut sets: Vec<String> = groups
        .into_iter()
        .filter(|n| n.starts_with("dataset"))
        .collect();
    sets.sort_by_key(|n| n[7..].parse::<u32>().unwrap_or(0));
    for name in sets {
        let ds = root.group(&name).map_err(bad)?;
        // Only polar scans.
        if find_str(&[&ds, &root], "what", "product")
            .is_some_and(|p| !p.eq_ignore_ascii_case("SCAN"))
        {
            continue;
        }
        let Some(elev) = find_f64(&[&ds, &root], "where", "elangle") else {
            continue;
        };
        let (dgroups, _) = ds.member_names().map_err(bad)?;
        for dn in dgroups
            .iter()
            .filter(|n| n.starts_with("data") && !n.starts_with("dataset"))
        {
            let dg = ds.group(dn).map_err(bad)?;
            let levels = [&dg, &ds, &root];
            let Some(quantity) = find_str(&levels, "what", "quantity") else {
                continue;
            };
            let Ok(d) = dg.dataset("data") else {
                continue;
            };
            let shape = d.shape().to_vec();
            if shape.len() != 2 {
                continue;
            }
            let (nrays, nbins) = (shape[0] as usize, shape[1] as usize);
            let gain = find_f64(&levels, "what", "gain").unwrap_or(1.0);
            let offset = find_f64(&levels, "what", "offset").unwrap_or(0.0);
            let nodata = find_f64(&levels, "what", "nodata");
            let undetect = find_f64(&levels, "what", "undetect");
            let rstart = find_f64(&[&dg, &ds, &root], "where", "rstart").unwrap_or(0.0);
            let rscale = find_f64(&[&dg, &ds, &root], "where", "rscale").unwrap_or(1000.0);
            let raw = read_numbers(&d)?;
            let values = raw
                .iter()
                .map(|&r| {
                    if Some(r) == nodata || Some(r) == undetect {
                        f32::NAN
                    } else {
                        (offset + gain * r) as f32
                    }
                })
                .collect();
            fields.push(Field {
                quantity: quantity.to_ascii_uppercase(),
                elevation_deg: elev as f32,
                nrays,
                nbins,
                first_gate_km: rstart as f32 + (rscale as f32 / 2000.0),
                gate_km: rscale as f32 / 1000.0,
                values,
            });
        }
    }
    Ok((header, fields))
}

fn rfc3339(date: &str, time: &str) -> Option<String> {
    let t = chrono::NaiveDateTime::parse_from_str(&format!("{date}{time}"), "%Y%m%d%H%M%S").ok()?;
    Some(t.and_utc().to_rfc3339())
}

/// The moment a quantity feeds, and whether it's reflectivity.
fn moment_of(q: &str) -> Option<Option<Moment>> {
    match q {
        "DBZH" | "TH" | "DBZ" => Some(None),
        "VRADH" | "VRAD" | "VRADDH" => Some(Some(Moment::Vel)),
        "ZDR" => Some(Some(Moment::Zdr)),
        "RHOHV" => Some(Some(Moment::Rho)),
        _ => None,
    }
}

/// Reflectivity sources, best first (DBZH is clutter-filtered, TH isn't).
fn ref_rank(q: &str) -> u8 {
    match q {
        "DBZH" => 0,
        "DBZ" => 1,
        _ => 2,
    }
}

/// Merge the fields of a scan's files into a volume: one sweep per
/// elevation, radials laid out by the reflectivity field (or, without
/// one, the first field), other moments resampled onto them by azimuth.
pub fn volume(site: &str, header: &Header, fields: &[Field]) -> Volume {
    let mut elevs: Vec<f32> = Vec::new();
    for f in fields {
        if moment_of(&f.quantity).is_some()
            && !elevs.iter().any(|e| (e - f.elevation_deg).abs() < 0.05)
        {
            elevs.push(f.elevation_deg);
        }
    }
    elevs.sort_by(f32::total_cmp);
    let mut sweeps = Vec::new();
    for e in elevs {
        let at: Vec<&Field> = fields
            .iter()
            .filter(|f| (f.elevation_deg - e).abs() < 0.05 && moment_of(&f.quantity).is_some())
            .collect();
        let refl = at
            .iter()
            .filter(|f| moment_of(&f.quantity) == Some(None))
            .min_by_key(|f| ref_rank(&f.quantity));
        let Some(base) = refl.or(at.first()) else {
            continue;
        };
        let has_ref = refl.is_some();
        let mut moments: Vec<(Moment, &Field)> = Vec::new();
        for f in &at {
            if let Some(Some(m)) = moment_of(&f.quantity) {
                // DK sends VRAD and VRADH: keep the first of each moment.
                if !moments.iter().any(|(mm, _)| *mm == m) {
                    moments.push((m, f));
                }
            }
        }
        let radials = (0..base.nrays)
            .map(|i| {
                let az = (i as f32 + 0.5) * 360.0 / base.nrays as f32;
                let dbz = if has_ref {
                    base.values[i * base.nbins..(i + 1) * base.nbins].to_vec()
                } else {
                    vec![f32::NAN; base.nbins]
                };
                let moments = moments
                    .iter()
                    .map(|(m, f)| {
                        let j = ((az / 360.0 * f.nrays as f32) as usize).min(f.nrays - 1);
                        MomentData {
                            moment: *m,
                            first_gate_km: f.first_gate_km,
                            gate_km: f.gate_km,
                            values: f.values[j * f.nbins..(j + 1) * f.nbins]
                                .iter()
                                .map(|v| if v.is_finite() { m.encode(*v) } else { 0 })
                                .collect(),
                        }
                    })
                    .collect();
                Radial {
                    azimuth_deg: az,
                    elevation_deg: e,
                    first_gate_km: base.first_gate_km,
                    gate_km: base.gate_km,
                    dbz,
                    moments,
                }
            })
            .collect();
        sweeps.push(Sweep {
            elevation_deg: e,
            radials,
        });
    }
    Volume {
        site: site.to_string(),
        lat: header.lat,
        lon: header.lon,
        height_m: header.height_m,
        time: header.time.clone(),
        sweeps,
    }
}

/// Scan time from a key (`…/deess@20261120T1235@5.5@DBZH.h5`), unix s.
pub fn key_time(key: &str) -> Option<i64> {
    let t = key.rsplit('/').next()?.split('@').nth(1)?;
    let t = chrono::NaiveDateTime::parse_from_str(t, "%Y%m%dT%H%M").ok()?;
    Some(t.and_utc().timestamp())
}

/// Which quantities a key holds (`DBZH_TH_VRADH`).
fn key_quantities(key: &str) -> Vec<String> {
    key.rsplit('@')
        .next()
        .unwrap_or("")
        .trim_end_matches(".h5")
        .split('_')
        .map(str::to_string)
        .collect()
}

/// The files that make up the newest complete-enough scan: everything
/// within [`SCAN_WINDOW_MIN`] of the newest file, skipping files with
/// nothing the app shows (and, for whole volumes, only the newest of each
/// quantity set).
pub fn newest_scan(keys: &[String]) -> Vec<String> {
    let useful = |k: &String| key_quantities(k).iter().any(|q| moment_of(q).is_some());
    let mut ks: Vec<(i64, &String)> = keys
        .iter()
        .filter(|k| k.ends_with(".h5") && useful(k))
        .filter_map(|k| Some((key_time(k)?, k)))
        .collect();
    ks.sort();
    let Some(&(newest, _)) = ks.last() else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // Newest first; a key's elevation + quantity list identifies it.
    for (t, k) in ks.iter().rev() {
        if newest - t > SCAN_WINDOW_MIN * 60 {
            break;
        }
        let name = k.rsplit('/').next().unwrap_or(k);
        let id: Vec<&str> = name.split('@').skip(2).collect();
        if seen.insert(id.join("@")) {
            out.push((*k).clone());
        }
    }
    out
}

impl Net {
    async fn odim_list(&self, prefix: &str, after: &str) -> Result<(usize, Vec<String>)> {
        let mut last = Err(NetError::Missing("radar data store"));
        for (i, store) in STORES.iter().enumerate() {
            let url = format!("{store}/?list-type=2&prefix={prefix}&start-after={after}");
            match self.get_text(&url, None).await {
                Ok(xml) => {
                    let keys = crate::level2::parse_listing_any(&xml);
                    if !keys.is_empty() {
                        return Ok((i, keys));
                    }
                    last = Ok((i, keys));
                }
                Err(e) => last = Err(e),
            }
        }
        last
    }

    /// The newest scan of an OPERA radar (ODIM code, e.g. `deess`).
    pub async fn latest_odim(&self, code: &str) -> Result<Volume> {
        let code = code.to_ascii_lowercase();
        let country = code
            .get(0..2)
            .ok_or_else(|| bad("bad radar code"))?
            .to_ascii_uppercase();
        let now = chrono::Utc::now();
        let mut found = None;
        // Today's folder, then (just after midnight) yesterday's.
        for day in [now, now - chrono::Duration::hours(2)] {
            let d = day.format("%Y/%m/%d");
            let after = (now - chrono::Duration::minutes(45)).format("%Y%m%dT%H%M");
            for kind in ["PVOL", "SCAN"] {
                let prefix = format!("{d}/{country}/{code}/{kind}/");
                let (store, keys) = self
                    .odim_list(&prefix, &format!("{prefix}{code}@{after}"))
                    .await?;
                if !keys.is_empty() {
                    found = Some((store, keys));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let (store, keys) = found.ok_or(NetError::Missing(
            "a recent scan from this radar (it may be offline)",
        ))?;
        let scan = newest_scan(&keys);
        if scan.is_empty() {
            return Err(NetError::Missing("a recent scan from this radar"));
        }
        let futs: Vec<_> = scan
            .iter()
            .map(|k| self.get_bytes_owned(format!("{}/{k}", STORES[store])))
            .collect();
        let files = futures::future::join_all(futs).await;
        let code2 = code.clone();
        tokio::task::spawn_blocking(move || {
            let mut header = None;
            let mut fields = Vec::new();
            for f in files.into_iter().flatten() {
                match parse(&f) {
                    Ok((h, fs)) => {
                        header.get_or_insert(h);
                        fields.extend(fs);
                    }
                    Err(e) => tracing::debug!("ODIM {code2}: {e}"),
                }
            }
            let header = header.ok_or_else(|| bad("no readable files"))?;
            let v = volume(&code2, &header, &fields);
            if v.sweeps.is_empty() {
                return Err(bad("no reflectivity or velocity in the scan"));
            }
            Ok(v)
        })
        .await
        .map_err(bad)?
    }

    async fn get_bytes_owned(&self, url: String) -> Result<Vec<u8>> {
        self.get_bytes(&url, None).await.map(|b| b.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_newest_scan() {
        let k = |s: &str| format!("2026/11/20/DE/deess/SCAN/{s}");
        let keys = vec![
            k("deess@20261120T1220@0.5@DBZH.h5"),
            k("deess@20261120T1230@0.5@DBZH.h5"),
            k("deess@20261120T1230@0.5@VRADH.h5"),
            k("deess@20261120T1231@1.5@DBZH.h5"),
            k("deess@20261120T1231@1.5@KDP.h5"),
            k("deess@20261120T1235@0.5@DBZH.h5"),
        ];
        let s = newest_scan(&keys);
        // 12:35 0.5 DBZH, then 12:31 1.5 DBZH, 12:30 0.5 VRADH; the older
        // 0.5 DBZH and KDP are skipped.
        assert_eq!(s.len(), 3, "{s:?}");
        assert!(s[0].contains("T1235@0.5@DBZH"));
        assert!(s.iter().any(|x| x.contains("T1230@0.5@VRADH")));
        assert!(!s.iter().any(|x| x.contains("KDP") || x.contains("T1220")));
        assert_eq!(
            key_time(&keys[0]),
            Some(
                chrono::NaiveDate::from_ymd_opt(2026, 11, 20)
                    .unwrap()
                    .and_hms_opt(12, 20, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp()
            )
        );
    }

    #[test]
    fn merges_fields_into_sweeps() {
        let f = |q: &str, e: f32, nrays: usize, v: f32| Field {
            quantity: q.into(),
            elevation_deg: e,
            nrays,
            nbins: 4,
            first_gate_km: 0.5,
            gate_km: 1.0,
            values: vec![v; nrays * 4],
        };
        let fields = vec![
            f("TH", 0.5, 360, 30.0),
            f("DBZH", 0.5, 360, 20.0),
            f("VRADH", 0.5, 720, -10.0),
            f("VRADH", 1.5, 360, 5.0),
            f("KDP", 0.5, 360, 1.0),
        ];
        let v = volume("deess", &Header::default(), &fields);
        assert_eq!(v.sweeps.len(), 2);
        let r = &v.sweeps[0].radials[10];
        assert_eq!(v.sweeps[0].radials.len(), 360);
        assert_eq!(r.dbz[0], 20.0); // DBZH preferred over TH
        assert_eq!(r.moments.len(), 1);
        assert!((Moment::Vel.decode(r.moments[0].values[0]).unwrap() + 10.0).abs() < 0.6);
        // A velocity-only tilt still makes a sweep (no reflectivity).
        assert!(v.sweeps[1].radials[0].dbz.iter().all(|d| d.is_nan()));
    }

    /// Real files: `SS_ODIM_SAMPLES=/dir` of `.h5` files of one scan.
    #[test]
    fn reads_real_files_when_available() {
        let Ok(dir) = std::env::var("SS_ODIM_SAMPLES") else {
            return;
        };
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        paths.sort();
        for p in paths
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "h5"))
        {
            let t = std::time::Instant::now();
            let (h, fields) = parse(&std::fs::read(p).unwrap()).unwrap();
            let v = volume("x", &h, &fields);
            println!(
                "{}: {:.3},{:.3} {} | {} fields {:?} | {} sweeps, {:?}",
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .chars()
                    .take(40)
                    .collect::<String>(),
                h.lat,
                h.lon,
                h.time,
                fields.len(),
                fields
                    .iter()
                    .map(|f| format!("{}@{}:{}x{}", f.quantity, f.elevation_deg, f.nrays, f.nbins))
                    .take(4)
                    .collect::<Vec<_>>(),
                v.sweeps.len(),
                t.elapsed()
            );
            assert!(h.lat > 30.0 && h.lat < 75.0);
        }
    }
}
