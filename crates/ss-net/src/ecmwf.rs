//! ECMWF open data (CC BY 4.0): the IFS 0.25° forecast, 00z and 12z runs
//! out to 10 days. Fields are read one at a time with HTTP range requests
//! using each step's index file.

use serde::Deserialize;

use crate::grib::Field;
use crate::{Net, NetError, Result};

const BASE: &str = "https://data.ecmwf.int/forecasts";

/// Forecast steps (hours) of a 00z/12z run.
pub fn steps() -> Vec<u32> {
    (0..=144).step_by(3).chain((150..=240).step_by(6)).collect()
}

fn run_parts(run: i64) -> (String, String) {
    let t = chrono::DateTime::from_timestamp(run, 0).unwrap_or_default();
    (t.format("%Y%m%d").to_string(), t.format("%H").to_string())
}

fn step_url(run: i64, step: u32, ext: &str) -> String {
    let (d, h) = run_parts(run);
    format!("{BASE}/{d}/{h}z/ifs/0p25/oper/{d}{h}0000-{step}h-oper-fc.{ext}")
}

#[derive(Debug, Deserialize)]
struct IndexLine {
    param: String,
    #[serde(default)]
    levelist: Option<String>,
    #[serde(rename = "_offset")]
    offset: u64,
    #[serde(rename = "_length")]
    length: u64,
}

/// A field in the index: parameter and (for pressure levels) level.
pub fn find_in_index(index: &str, param: &str, level: Option<&str>) -> Option<(u64, u64)> {
    index
        .lines()
        .filter_map(|l| serde_json::from_str::<IndexLine>(l).ok())
        .find(|r| r.param == param && r.levelist.as_deref() == level)
        .map(|r| (r.offset, r.length))
}

impl Net {
    /// The newest 00z/12z run whose last step is out (unix s).
    pub async fn ecmwf_latest_run(&self) -> Result<i64> {
        let now = chrono::Utc::now().timestamp();
        let mut run = now - now.rem_euclid(12 * 3600);
        for _ in 0..4 {
            // The whole run is out when its last step's index is.
            if self
                .get_text(&step_url(run, 240, "index"), None)
                .await
                .is_ok()
            {
                return Ok(run);
            }
            run -= 12 * 3600;
        }
        Err(NetError::Service("no recent ECMWF run found".into()))
    }

    /// One field of a run and step: (param, pressure level or None).
    pub async fn ecmwf_field(
        &self,
        run: i64,
        step: u32,
        param: &str,
        level: Option<&str>,
    ) -> Result<Field> {
        let index = self.get_text(&step_url(run, step, "index"), None).await?;
        let (off, len) = find_in_index(&index, param, level)
            .ok_or_else(|| NetError::Service(format!("ECMWF: no {param} at +{step} h")))?;
        let resp = self
            .client
            .get(step_url(run, step, "grib2"))
            .header(
                reqwest::header::RANGE,
                format!("bytes={off}-{}", off + len - 1),
            )
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(NetError::Status {
                url: step_url(run, step, "grib2"),
                status: resp.status().as_u16(),
            });
        }
        let bytes = resp.bytes().await?.to_vec();
        tokio::task::spawn_blocking(move || crate::grib::decode(&bytes))
            .await
            .map_err(|e| NetError::Parse(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_and_urls() {
        let s = steps();
        assert_eq!(
            (s[0], s[1], s[48], s[49], *s.last().unwrap()),
            (0, 3, 144, 150, 240)
        );
        let run = chrono::NaiveDate::from_ymd_opt(2026, 11, 20)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        assert_eq!(
            step_url(run, 24, "index"),
            "https://data.ecmwf.int/forecasts/20261120/12z/ifs/0p25/oper/20261120120000-24h-oper-fc.index"
        );
    }

    #[test]
    fn finds_fields_in_the_index() {
        let idx = r#"{"param": "2t", "levtype": "sfc", "_offset": 10, "_length": 5}
{"param": "gh", "levelist": "850", "levtype": "pl", "_offset": 20, "_length": 6}
{"param": "gh", "levelist": "500", "levtype": "pl", "_offset": 30, "_length": 7}"#;
        assert_eq!(find_in_index(idx, "2t", None), Some((10, 5)));
        assert_eq!(find_in_index(idx, "gh", Some("500")), Some((30, 7)));
        assert_eq!(find_in_index(idx, "msl", None), None);
    }
}
