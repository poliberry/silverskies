//! Silver Skies network layer. All API clients live here; nothing in this crate
//! touches the UI. Each client is a thin async function over a shared
//! `reqwest::Client` and returns `ss_core` types.
//!
//! Every client is split into a pure `parse_*` function (unit tested with
//! inlined fixtures) and a thin async fetcher.
//!
//! Alerts leave this crate *unclassified*: `class` is empty (or, for ECCC, the
//! raw `risk_colour_en` value) and `display_event` equals `event`. The app
//! layer runs the classifier.

pub mod accuweather;
pub mod eccc;
pub mod gdacs;
pub mod iem;
pub mod librewxr;
pub mod mosaic;
pub mod ncep;
pub mod nwr;
pub mod nws;
pub mod openmeteo;
pub mod owm;
pub mod spc;
pub mod willyweather;

use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

pub const USER_AGENT: &str =
    "SilverSkies/2.0 (desktop app; https://github.com/poliberry/silverskies-desktop)";

/// Errors name the host, not the full request URL (which can be hundreds of
/// characters of query string and ends up in panes and logs).
#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("{}", describe_http(.0))]
    Http(#[from] reqwest::Error),
    #[error("HTTP {status}{} from {}", status_hint(*.status), host_of(.url))]
    Status { url: String, status: u16 },
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("missing field: {0}")]
    Missing(&'static str),
    /// A service-specific, already readable message (e.g. a rejected key).
    #[error("{0}")]
    Service(String),
}

pub type Result<T> = std::result::Result<T, NetError>;

/// The host part of a URL (`api.open-meteo.com`), or the input if unparsable.
pub fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    &rest[..end]
}

fn status_hint(status: u16) -> &'static str {
    match status {
        429 => " (rate limited)",
        500..=599 => " (server error)",
        404 => " (not found)",
        401 | 403 => " (not allowed)",
        _ => "",
    }
}

fn describe_http(e: &reqwest::Error) -> String {
    let host = e
        .url()
        .and_then(|u| u.host_str())
        .unwrap_or("the server")
        .to_string();
    if e.is_timeout() {
        format!("network error: {host} timed out")
    } else if e.is_connect() {
        format!("network error: couldn't connect to {host}")
    } else if e.is_decode() || e.is_body() {
        format!("network error: bad response from {host}")
    } else {
        format!("network error talking to {host}")
    }
}

/// AccuWeather caches shared by every clone of a [`Net`]: location keys by
/// coordinates, and recent forecasts (to stay within key quotas).
#[derive(Default)]
pub struct AccuCache {
    pub(crate) locations: std::sync::Mutex<std::collections::HashMap<String, (String, String)>>,
    /// Keys that only work the legacy `?apikey=` way, so later requests
    /// skip the Bearer attempt (which would cost a request each).
    pub(crate) legacy_keys: std::sync::Mutex<std::collections::HashSet<String>>,
    #[allow(clippy::type_complexity)]
    pub(crate) forecasts: std::sync::Mutex<
        std::collections::HashMap<String, (std::time::Instant, ss_core::types::Weather)>,
    >,
}

/// Shared HTTP client plus the configured LibreWXR host.
#[derive(Clone)]
pub struct Net {
    client: reqwest::Client,
    pub libre_wxr_host: String,
    accu: std::sync::Arc<AccuCache>,
}

impl Net {
    pub fn new(libre_wxr_host: &str) -> Result<Net> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Net {
            client,
            libre_wxr_host: libre_wxr_host.trim_end_matches('/').to_string(),
            accu: Default::default(),
        })
    }

    async fn send(&self, url: &str, accept: Option<&str>) -> Result<reqwest::Response> {
        tracing::debug!(url, "GET");
        let mut req = self.client.get(url);
        if let Some(a) = accept {
            req = req.header(reqwest::header::ACCEPT, a);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Status {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }
        Ok(resp)
    }

    pub async fn get_text(&self, url: &str, accept: Option<&str>) -> Result<String> {
        Ok(self.send(url, accept).await?.text().await?)
    }

    pub async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        accept: Option<&str>,
    ) -> Result<T> {
        let text = self.get_text(url, accept).await?;
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn get_bytes(&self, url: &str, accept: Option<&str>) -> Result<Vec<u8>> {
        Ok(self.send(url, accept).await?.bytes().await?.to_vec())
    }

    /// The forecast from the configured provider: AccuWeather when it is
    /// selected and has a key, Open-Meteo otherwise.
    pub async fn weather(
        &self,
        provider: ss_core::types::WeatherProviderId,
        accu_key: Option<&str>,
        lat: f64,
        lon: f64,
    ) -> Result<ss_core::types::Weather> {
        let key = accu_key.map(str::trim).filter(|k| !k.is_empty());
        match (provider, key) {
            (ss_core::types::WeatherProviderId::AccuWeather, Some(k)) => {
                self.accuweather_forecast(lat, lon, k).await
            }
            _ => self.forecast(lat, lon).await,
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers shared by the clients
// ---------------------------------------------------------------------------

pub(crate) const GEO_JSON: &str = "application/geo+json";

pub(crate) fn parse_value(json: &str) -> Result<Value> {
    Ok(serde_json::from_str(json)?)
}

/// Non-empty trimmed string field.
pub(crate) fn jstr(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Number field, tolerating numeric strings.
pub(crate) fn jf64(v: &Value, key: &str) -> Option<f64> {
    match v.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A timestamp that is either a string or unix seconds.
pub(crate) fn time_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => {
            let secs = n.as_i64()?;
            chrono::DateTime::from_timestamp(secs, 0)
                .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        }
        _ => None,
    }
}

pub(crate) fn parse_geometry(v: Option<&Value>) -> Option<ss_core::types::Geometry> {
    let v = v?;
    if v.is_null() {
        return None;
    }
    serde_json::from_value(v.clone()).ok()
}

/// Stable (cross-run) FNV-1a hash, hex encoded.
pub(crate) fn stable_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

pub(crate) fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dphi = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dphi / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    6371.0088 * 2.0 * a.sqrt().asin()
}

pub(crate) fn enc(s: &str) -> String {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_known_distance() {
        // Safford AZ -> Tucson AZ is roughly 150 km.
        let d = haversine_km(32.834, -109.7076, 32.2226, -110.9747);
        assert!((d - 133.0).abs() < 15.0, "{d}");
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(stable_hash("a"), stable_hash("a"));
        assert_ne!(stable_hash("a"), stable_hash("b"));
    }

    #[test]
    fn time_str_handles_unix() {
        assert_eq!(
            time_str(&serde_json::json!(0)).as_deref(),
            Some("1970-01-01T00:00:00Z")
        );
    }

    #[test]
    fn net_trims_host() {
        let n = Net::new("https://x.test/").unwrap();
        assert_eq!(n.libre_wxr_host, "https://x.test");
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn errors_name_the_host_not_the_url() {
        let e = NetError::Status {
            url: "https://api.open-meteo.com/v1/forecast?latitude=1&longitude=2".into(),
            status: 429,
        };
        assert_eq!(
            e.to_string(),
            "HTTP 429 (rate limited) from api.open-meteo.com"
        );
        assert_eq!(host_of("https://a.b:8080/x?y"), "a.b:8080");
        assert_eq!(host_of("not a url"), "not a url");
    }
}
