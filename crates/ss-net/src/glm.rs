//! Lightning from the GOES Geostationary Lightning Mapper (GLM): GOES-19
//! (East: the Americas and Atlantic) and GOES-18 (West: the Pacific), from
//! NOAA's open buckets on AWS. Each Level 2 file holds ~20 s of flashes
//! (netCDF-4), read with a pure-Rust decoder.

use std::collections::HashSet;

use crate::{Net, NetError, Result};

/// One lightning flash.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flash {
    pub lat: f64,
    pub lon: f64,
    /// Unix seconds (start of the 20 s file).
    pub time: i64,
}

/// The satellites' buckets.
pub const BUCKETS: [&str; 2] = ["noaa-goes19", "noaa-goes18"];

fn list_url(bucket: &str, hour: chrono::DateTime<chrono::Utc>) -> String {
    format!(
        "https://{bucket}.s3.amazonaws.com/?list-type=2&prefix=GLM-L2-LCFA/{}/",
        hour.format("%Y/%j/%H")
    )
}

/// Start time of a GLM file from its name (`…_s20263240919000_e…`).
pub fn file_time(key: &str) -> Option<i64> {
    let s = key.split("_s").nth(1)?.get(0..13)?;
    let year: i32 = s.get(0..4)?.parse().ok()?;
    let doy: u32 = s.get(4..7)?.parse().ok()?;
    let (h, m, sec): (u32, u32, u32) = (
        s.get(7..9)?.parse().ok()?,
        s.get(9..11)?.parse().ok()?,
        s.get(11..13)?.parse().ok()?,
    );
    let d = chrono::NaiveDate::from_yo_opt(year, doy)?;
    Some(d.and_hms_opt(h, m, sec)?.and_utc().timestamp())
}

/// Good-quality flashes in a GLM Level 2 file.
pub fn parse_flashes(nc: &[u8], time: i64) -> Result<Vec<Flash>> {
    let bad = |e: netcdf_reader::Error| NetError::Parse(format!("GLM: {e}"));
    let f = netcdf_reader::NcFile::from_bytes(nc).map_err(bad)?;
    let lat = f.read_variable_unpacked("flash_lat").map_err(bad)?;
    let lon = f.read_variable_unpacked("flash_lon").map_err(bad)?;
    let quality = f.read_variable_as_f64("flash_quality_flag").ok();
    Ok(lat
        .iter()
        .zip(lon.iter())
        .enumerate()
        .filter(|(i, _)| {
            quality
                .as_ref()
                .is_none_or(|q| q.get(*i).is_none_or(|&v| v == 0.0))
        })
        .filter(|(_, (la, lo))| la.is_finite() && lo.is_finite())
        .map(|(_, (&la, &lo))| Flash {
            lat: la,
            lon: lo,
            time,
        })
        .collect())
}

/// The flash nearest to (`lat`, `lon`) within `max_km`, newer than
/// `since`: (distance km, bearing deg, the flash).
pub fn nearest(
    flashes: &[Flash],
    lat: f64,
    lon: f64,
    max_km: f64,
    since: i64,
) -> Option<(f64, f64, Flash)> {
    flashes
        .iter()
        .filter(|f| f.time >= since)
        .map(|f| (ss_core::geo::haversine_km(lat, lon, f.lat, f.lon), f))
        .filter(|(d, _)| *d <= max_km)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(d, f)| (d, ss_core::geo::bearing_deg(lat, lon, f.lat, f.lon), *f))
}

/// Roughly where the GOES lightning mappers see (the Americas, Atlantic,
/// eastern Pacific).
pub fn glm_covers(lat: f64, lon: f64) -> bool {
    lat.abs() <= 54.0 && (-175.0..=-15.0).contains(&lon)
}

/// Flashes from an Xweather `/lightning/closest` response.
pub fn parse_xweather(json: &str) -> Result<Vec<Flash>> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| NetError::Parse(format!("Xweather: {e}")))?;
    if v.get("success").and_then(|s| s.as_bool()) == Some(false) {
        let err = &v["error"];
        // "No data" is a success with nothing nearby.
        if err["code"].as_str() == Some("warn_no_data") {
            return Ok(Vec::new());
        }
        let msg = err["description"]
            .as_str()
            .or_else(|| err["code"].as_str())
            .unwrap_or("request refused");
        return Err(NetError::Service(format!("Xweather: {msg}")));
    }
    Ok(v["response"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    Some(Flash {
                        lat: r["loc"]["lat"].as_f64()?,
                        lon: r["loc"]["long"].as_f64()?,
                        time: r["ob"]["timestamp"].as_i64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

impl Net {
    /// Strikes within 100 km of a point in the last 5 minutes from Xweather
    /// (Vaisala GLD360, worldwide). Each call costs 10 accesses.
    pub async fn xweather_lightning(
        &self,
        client_id: &str,
        client_secret: &str,
        lat: f64,
        lon: f64,
    ) -> Result<Vec<Flash>> {
        let enc = |s: &str| {
            percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
        };
        let url = format!(
            "https://data.api.xweather.com/lightning/closest?p={lat:.4},{lon:.4}&radius=100km&limit=1000&client_id={}&client_secret={}",
            enc(client_id),
            enc(client_secret)
        );
        // Errors come back as JSON with a 4xx status: read the body either way.
        let body = self.get_text_any_status(&url).await?;
        parse_xweather(&body)
    }

    /// One GLM file's flashes.
    async fn glm_file(&self, bucket: &str, key: String, time: i64) -> (String, Result<Vec<Flash>>) {
        let url = format!("https://{bucket}.s3.amazonaws.com/{key}");
        let r = match self.get_bytes(&url, None).await {
            Ok(b) => tokio::task::spawn_blocking(move || parse_flashes(&b, time))
                .await
                .map_err(|e| NetError::Parse(e.to_string()))
                .and_then(|r| r),
            Err(e) => Err(e),
        };
        (key, r)
    }

    /// Flashes from GLM files newer than `since` (unix s) that aren't in
    /// `known`, from both satellites. Returns them with the keys read.
    pub async fn glm_flashes(
        &self,
        since: i64,
        known: &HashSet<String>,
    ) -> Result<(Vec<Flash>, Vec<String>)> {
        let now = chrono::Utc::now();
        let hours = [now - chrono::Duration::hours(1), now];
        let mut keys = Vec::new();
        for bucket in BUCKETS {
            for h in hours {
                if h.timestamp() + 3600 < since {
                    continue;
                }
                let Ok(xml) = self.get_text(&list_url(bucket, h), None).await else {
                    continue;
                };
                for k in crate::level2::parse_listing_any(&xml) {
                    let t = file_time(&k).unwrap_or(0);
                    if t >= since && !known.contains(&k) {
                        keys.push((bucket, k, t));
                    }
                }
            }
        }
        if keys.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        use futures::StreamExt;
        // Built up front (futures are lazy): mapping inside the stream
        // trips the compiler's Send inference.
        let futs: Vec<_> = keys
            .into_iter()
            .map(|(bucket, k, t)| self.glm_file(bucket, k, t))
            .collect();
        let got: Vec<_> = futures::stream::iter(futs)
            .buffer_unordered(24)
            .collect()
            .await;
        let mut flashes = Vec::new();
        let mut read = Vec::new();
        for (k, r) in got {
            match r {
                Ok(f) => {
                    flashes.extend(f);
                    read.push(k);
                }
                Err(e) => tracing::debug!("GLM {k}: {e}"),
            }
        }
        Ok((flashes, read))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_file_times() {
        let k = "GLM-L2-LCFA/2026/324/09/OR_GLM-L2-LCFA_G19_s20263240919000_e20263240919200_c20263240919217.nc";
        let t = file_time(k).unwrap();
        // Day 324 of 2026 is 20 November.
        let want = chrono::NaiveDate::from_ymd_opt(2026, 11, 20)
            .unwrap()
            .and_hms_opt(9, 19, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        assert_eq!(t, want);
        assert_eq!(file_time("nonsense"), None);
    }

    #[test]
    fn finds_the_nearest_flash() {
        let f = |lat, lon, time| Flash { lat, lon, time };
        let all = [
            f(38.0, -121.0, 100),
            f(38.1, -121.8, 100),
            f(38.0, -121.81, 10),
        ];
        let (d, b, n) = nearest(&all, 38.0, -121.8, 50.0, 50).unwrap();
        assert_eq!(n, all[1]);
        assert!((d - 11.1).abs() < 0.3 && !(5.0..=355.0).contains(&b));
        assert!(nearest(&all, 0.0, 0.0, 50.0, 0).is_none());
    }

    #[test]
    fn reads_xweather_strikes() {
        let ok = r#"{"success":true,"error":null,"response":[
            {"loc":{"lat":32.9001,"long":-87.3311},"ob":{"timestamp":1748359934,"dateTimeISO":"2025-05-27T15:32:14+00:00","pulse":{"type":"cg","peakamp":-7000}}},
            {"loc":{"lat":33.1,"long":-87.0},"ob":{"timestamp":1748359940}}]}"#;
        let f = parse_xweather(ok).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(
            f[0],
            Flash {
                lat: 32.9001,
                lon: -87.3311,
                time: 1748359934
            }
        );
        let none = r#"{"success":true,"error":{"code":"warn_no_data","description":"No data"},"response":[]}"#;
        assert!(parse_xweather(none).unwrap().is_empty());
        let bad = r#"{"success":false,"error":{"code":"invalid_client","description":"The client provided is invalid."},"response":[]}"#;
        assert!(parse_xweather(bad)
            .unwrap_err()
            .to_string()
            .contains("client provided is invalid"));
        assert!(glm_covers(35.0, -97.0) && !glm_covers(52.0, 5.0) && !glm_covers(-33.9, 151.2));
    }

    /// A real file: set `SS_GLM_SAMPLE=/path/to/OR_GLM-L2-LCFA_….nc`.
    #[test]
    fn reads_a_real_file_when_available() {
        let Ok(path) = std::env::var("SS_GLM_SAMPLE") else {
            return;
        };
        let f = parse_flashes(&std::fs::read(path).unwrap(), 0).unwrap();
        assert!(!f.is_empty());
        assert!(f
            .iter()
            .all(|x| x.lat.abs() <= 90.0 && x.lon.abs() <= 180.0));
    }
}
