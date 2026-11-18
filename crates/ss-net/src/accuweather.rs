//! AccuWeather Core Weather API: location key by coordinates, current
//! conditions, 12-hour and 5-day forecasts, mapped onto the shared
//! [`Weather`] model (AccuWeather icon numbers become WMO weather codes).
//!
//! Requests authenticate with `Authorization: Bearer <key>` as AccuWeather's
//! current developer portal documents; keys from the older portal that only
//! work as an `apikey` query parameter are retried that way. Free keys allow
//! 50 requests a day, so location keys are cached for the session and a
//! place's forecast is reused for [`MIN_REFRESH`].

use std::time::{Duration, Instant};

use crate::{parse_value, Net, NetError, Result};
use serde_json::Value;
use ss_core::types::{CurrentConditions, DailyPoint, HourlyPoint, Weather, WeatherProviderId};

pub const BASE: &str = "https://dataservice.accuweather.com";

/// The API host; `SS_ACCUWEATHER_BASE` overrides it (for testing against a
/// local mock server).
fn base() -> String {
    std::env::var("SS_ACCUWEATHER_BASE")
        .ok()
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| BASE.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// A place's AccuWeather forecast is reused for this long (three requests
/// per refresh against a free key's 50 a day).
pub const MIN_REFRESH: Duration = Duration::from_secs(60 * 60);

/// AccuWeather icon number (1–44) → the closest WMO weather code.
pub fn icon_to_wmo(icon: u8) -> u8 {
    match icon {
        1 | 33 => 0,             // clear
        2 | 34 => 1,             // mostly clear
        3 | 4 | 35 | 36 => 2,    // partly cloudy
        5 | 37 => 45,            // hazy
        6 | 38 => 2,             // mostly cloudy
        7 | 8 => 3,              // cloudy / overcast
        11 => 45,                // fog
        12 => 61,                // showers
        13 | 14 | 39 | 40 => 80, // (partly sunny) showers
        15 => 95,                // thunderstorms
        16 | 17 | 41 | 42 => 95, // (partly sunny) t-storms
        18 => 63,                // rain
        19 => 71,                // flurries
        20 | 21 | 43 => 85,      // (partly sunny) flurries
        22 => 73,                // snow
        23 | 44 => 86,           // mostly cloudy w/ snow
        24 => 67,                // ice
        25 => 77,                // sleet
        26 => 66,                // freezing rain
        29 => 85,                // rain and snow (mixed showers)
        30..=32 => 0,            // hot / cold / windy
        _ => 3,
    }
}

fn path<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().try_fold(v, |cur, k| cur.get(*k))
}

fn num(v: &Value, keys: &[&str]) -> f64 {
    path(v, keys).and_then(Value::as_f64).unwrap_or(0.0)
}

fn text<'a>(v: &'a Value, keys: &[&str]) -> &'a str {
    path(v, keys).and_then(Value::as_str).unwrap_or_default()
}

fn icon(v: &Value, keys: &[&str]) -> u8 {
    icon_to_wmo(num(v, keys).clamp(0.0, 255.0) as u8)
}

/// `2026-11-18T07:06:00-06:00` → `2026-11-18T07:06` (local wall time, as
/// the rest of the app expects).
fn local_minute(iso: &str) -> String {
    iso.get(..16).unwrap_or(iso).to_string()
}

/// `(location key, IANA time zone)` from a geoposition search.
pub fn parse_geoposition(json: &str) -> Result<(String, String)> {
    let v = parse_value(json)?;
    let key = text(&v, &["Key"]);
    if key.is_empty() {
        return Err(NetError::Missing("Key"));
    }
    Ok((key.to_string(), text(&v, &["TimeZone", "Name"]).to_string()))
}

/// Current conditions (request with `details=true`).
pub fn parse_current(json: &str) -> Result<(CurrentConditions, String)> {
    let v = parse_value(json)?;
    let c = v
        .as_array()
        .and_then(|a| a.first())
        .ok_or(NetError::Missing("current conditions"))?;
    let cur = CurrentConditions {
        temperature_c: num(c, &["Temperature", "Metric", "Value"]),
        apparent_temperature_c: num(c, &["RealFeelTemperature", "Metric", "Value"]),
        weather_code: icon(c, &["WeatherIcon"]),
        is_day: path(c, &["IsDayTime"])
            .and_then(Value::as_bool)
            .unwrap_or(true),
        relative_humidity_pct: num(c, &["RelativeHumidity"]),
        wind_speed_kmh: num(c, &["Wind", "Speed", "Metric", "Value"]),
        wind_direction_deg: num(c, &["Wind", "Direction", "Degrees"]),
        precipitation_mm: num(c, &["PrecipitationSummary", "PastHour", "Metric", "Value"]),
        cloud_cover_pct: num(c, &["CloudCover"]),
        uv_index: num(c, &["UVIndex"]),
    };
    Ok((cur, local_minute(text(c, &["LocalObservationDateTime"]))))
}

/// The 12-hour forecast (request with `metric=true`).
pub fn parse_hourly(json: &str) -> Result<Vec<HourlyPoint>> {
    let v = parse_value(json)?;
    let hours = v.as_array().ok_or(NetError::Missing("hourly forecast"))?;
    Ok(hours
        .iter()
        .map(|h| HourlyPoint {
            time: local_minute(text(h, &["DateTime"])),
            temperature_c: num(h, &["Temperature", "Value"]),
            weather_code: icon(h, &["WeatherIcon"]),
            is_day: path(h, &["IsDaylight"])
                .and_then(Value::as_bool)
                .unwrap_or(true),
            precipitation_probability_pct: num(h, &["PrecipitationProbability"]),
        })
        .collect())
}

/// The 5-day forecast (request with `metric=true&details=true`).
pub fn parse_daily(json: &str) -> Result<Vec<DailyPoint>> {
    let v = parse_value(json)?;
    let days = path(&v, &["DailyForecasts"])
        .and_then(Value::as_array)
        .ok_or(NetError::Missing("DailyForecasts"))?;
    Ok(days
        .iter()
        .map(|d| {
            let uv = path(d, &["AirAndPollen"])
                .and_then(Value::as_array)
                .and_then(|a| a.iter().find(|x| text(x, &["Name"]) == "UVIndex"))
                .map(|x| num(x, &["Value"]))
                .unwrap_or(0.0);
            DailyPoint {
                date: text(d, &["Date"]).get(..10).unwrap_or_default().to_string(),
                weather_code: icon(d, &["Day", "Icon"]),
                temp_max_c: num(d, &["Temperature", "Maximum", "Value"]),
                temp_min_c: num(d, &["Temperature", "Minimum", "Value"]),
                precipitation_probability_max_pct: num(d, &["Day", "PrecipitationProbability"])
                    .max(num(d, &["Night", "PrecipitationProbability"])),
                sunrise: local_minute(text(d, &["Sun", "Rise"])),
                sunset: local_minute(text(d, &["Sun", "Set"])),
                uv_index_max: uv,
            }
        })
        .collect())
}

/// Put the forecast together; the current hour (from the observation) leads
/// the hourly list, since AccuWeather's 12 hours start at the next hour.
pub fn assemble(
    current: CurrentConditions,
    observed: &str,
    mut hourly: Vec<HourlyPoint>,
    daily: Vec<DailyPoint>,
    timezone: String,
) -> Weather {
    let now_hour = observed.get(..13).map(|h| format!("{h}:00"));
    if let Some(t) = now_hour {
        if hourly.first().is_none_or(|h| h.time != t) {
            hourly.insert(
                0,
                HourlyPoint {
                    time: t,
                    temperature_c: current.temperature_c,
                    weather_code: current.weather_code,
                    is_day: current.is_day,
                    precipitation_probability_pct: 0.0,
                },
            );
        }
    }
    Weather {
        current,
        hourly,
        daily,
        timezone,
        provider: WeatherProviderId::AccuWeather,
    }
}

/// A readable reason for an AccuWeather HTTP failure.
pub fn status_message(status: u16) -> String {
    match status {
        401 | 403 => "AccuWeather rejected the API key. Check it in Settings.".into(),
        503 | 429 => "AccuWeather's request limit for this API key has been reached \
                      (free keys allow 50 requests a day). Try again later."
            .into(),
        s => format!("AccuWeather returned HTTP {s}."),
    }
}

fn cache_key(lat: f64, lon: f64) -> String {
    format!("{lat:.3},{lon:.3}")
}

impl Net {
    /// GET an AccuWeather URL: Bearer header first, then (for keys from the
    /// older portal) the `apikey` query parameter.
    async fn accu_get(&self, url: &str, key: &str) -> Result<String> {
        let legacy_url = || {
            let sep = if url.contains('?') { '&' } else { '?' };
            format!("{url}{sep}apikey={}", crate::enc(key))
        };
        let known_legacy = self.accu.legacy_keys.lock().unwrap().contains(key);
        if !known_legacy {
            let resp = self.client.get(url).bearer_auth(key).send().await?;
            let status = resp.status().as_u16();
            if resp.status().is_success() {
                return Ok(resp.text().await?);
            }
            if status != 401 && status != 403 {
                return Err(NetError::Service(status_message(status)));
            }
        }
        let resp = self.client.get(legacy_url()).send().await?;
        let status = resp.status().as_u16();
        if resp.status().is_success() {
            self.accu
                .legacy_keys
                .lock()
                .unwrap()
                .insert(key.to_string());
            return Ok(resp.text().await?);
        }
        Err(NetError::Service(status_message(status)))
    }

    /// The AccuWeather forecast for a point (location key cached; whole
    /// forecast reused for [`MIN_REFRESH`]).
    pub async fn accuweather_forecast(&self, lat: f64, lon: f64, key: &str) -> Result<Weather> {
        let ck = cache_key(lat, lon);
        if let Some((at, w)) = self.accu.forecasts.lock().unwrap().get(&ck) {
            if at.elapsed() < MIN_REFRESH {
                return Ok(w.clone());
            }
        }
        let base = base();
        let cached = self.accu.locations.lock().unwrap().get(&ck).cloned();
        let (loc, tz) = match cached {
            Some(v) => v,
            None => {
                let url = format!("{base}/locations/v1/cities/geoposition/search?q={lat},{lon}");
                let v = parse_geoposition(&self.accu_get(&url, key).await?)?;
                self.accu
                    .locations
                    .lock()
                    .unwrap()
                    .insert(ck.clone(), v.clone());
                v
            }
        };
        let cur = self
            .accu_get(
                &format!("{base}/currentconditions/v1/{loc}?details=true"),
                key,
            )
            .await?;
        let hourly = self
            .accu_get(
                &format!("{base}/forecasts/v1/hourly/12hour/{loc}?metric=true"),
                key,
            )
            .await?;
        let daily = self
            .accu_get(
                &format!("{base}/forecasts/v1/daily/5day/{loc}?metric=true&details=true"),
                key,
            )
            .await?;
        let (current, observed) = parse_current(&cur)?;
        let w = assemble(
            current,
            &observed,
            parse_hourly(&hourly)?,
            parse_daily(&daily)?,
            tz,
        );
        self.accu
            .forecasts
            .lock()
            .unwrap()
            .insert(ck, (Instant::now(), w.clone()));
        Ok(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEO: &str = r#"{"Key":"331976","LocalizedName":"Norman","TimeZone":{"Code":"CST","Name":"America/Chicago","GmtOffset":-6.0}}"#;
    const CURRENT: &str = r#"[{"LocalObservationDateTime":"2026-11-18T09:45:00-06:00","WeatherIcon":7,"IsDayTime":true,
        "Temperature":{"Metric":{"Value":8.3,"Unit":"C"},"Imperial":{"Value":47.0,"Unit":"F"}},
        "RealFeelTemperature":{"Metric":{"Value":5.1,"Unit":"C"}},"RelativeHumidity":71,
        "Wind":{"Direction":{"Degrees":180,"Localized":"S"},"Speed":{"Metric":{"Value":14.8,"Unit":"km/h"}}},
        "UVIndex":1,"CloudCover":95,"PrecipitationSummary":{"PastHour":{"Metric":{"Value":0.2,"Unit":"mm"}}}}]"#;
    const HOURLY: &str = r#"[{"DateTime":"2026-11-18T10:00:00-06:00","WeatherIcon":12,"IsDaylight":true,"Temperature":{"Value":9.0,"Unit":"C"},"PrecipitationProbability":40},
        {"DateTime":"2026-11-18T11:00:00-06:00","WeatherIcon":15,"IsDaylight":true,"Temperature":{"Value":10.5,"Unit":"C"},"PrecipitationProbability":60}]"#;
    const DAILY: &str = r#"{"Headline":{},"DailyForecasts":[{"Date":"2026-11-18T07:00:00-06:00",
        "Sun":{"Rise":"2026-11-18T07:06:00-06:00","Set":"2026-11-18T17:22:00-06:00"},
        "Temperature":{"Minimum":{"Value":4.4,"Unit":"C"},"Maximum":{"Value":12.8,"Unit":"C"}},
        "AirAndPollen":[{"Name":"AirQuality","Value":30},{"Name":"UVIndex","Value":3,"Category":"Moderate"}],
        "Day":{"Icon":18,"PrecipitationProbability":70},"Night":{"Icon":7,"PrecipitationProbability":20}}]}"#;

    #[test]
    fn parses_a_full_forecast() {
        let (key, tz) = parse_geoposition(GEO).unwrap();
        assert_eq!((key.as_str(), tz.as_str()), ("331976", "America/Chicago"));
        let (cur, observed) = parse_current(CURRENT).unwrap();
        assert_eq!(cur.temperature_c, 8.3);
        assert_eq!(cur.apparent_temperature_c, 5.1);
        assert_eq!(cur.weather_code, 3);
        assert_eq!(cur.wind_speed_kmh, 14.8);
        assert_eq!(cur.wind_direction_deg, 180.0);
        assert_eq!(cur.precipitation_mm, 0.2);
        assert_eq!(observed, "2026-11-18T09:45");
        let w = assemble(
            cur,
            &observed,
            parse_hourly(HOURLY).unwrap(),
            parse_daily(DAILY).unwrap(),
            tz,
        );
        // The current hour leads, then AccuWeather's hours.
        let times: Vec<_> = w.hourly.iter().map(|h| h.time.as_str()).collect();
        assert_eq!(
            times,
            ["2026-11-18T09:00", "2026-11-18T10:00", "2026-11-18T11:00"]
        );
        assert_eq!(w.hourly[2].weather_code, 95);
        assert_eq!(w.hourly[1].precipitation_probability_pct, 40.0);
        let d = &w.daily[0];
        assert_eq!(d.date, "2026-11-18");
        assert_eq!((d.temp_min_c, d.temp_max_c), (4.4, 12.8));
        assert_eq!(d.sunrise, "2026-11-18T07:06");
        assert_eq!(d.sunset, "2026-11-18T17:22");
        assert_eq!(d.uv_index_max, 3.0);
        assert_eq!(d.precipitation_probability_max_pct, 70.0);
        assert_eq!(d.weather_code, 63);
        assert_eq!(w.provider, WeatherProviderId::AccuWeather);
        assert_eq!(w.timezone, "America/Chicago");
    }

    #[test]
    fn icons_map_to_wmo_codes() {
        assert_eq!(icon_to_wmo(1), 0);
        assert_eq!(icon_to_wmo(2), 1);
        assert_eq!(icon_to_wmo(34), 1);
        assert_eq!(icon_to_wmo(7), 3);
        assert_eq!(icon_to_wmo(11), 45);
        assert_eq!(icon_to_wmo(15), 95);
        assert_eq!(icon_to_wmo(22), 73);
        assert_eq!(icon_to_wmo(26), 66);
        // Every icon maps to a code the app has a label and icon for.
        for i in 0..=50 {
            assert_ne!(
                ss_core::weather::wmo::wmo_label(icon_to_wmo(i)),
                "Unknown",
                "icon {i}"
            );
        }
    }

    #[test]
    fn errors_are_readable() {
        assert!(status_message(401).contains("rejected the API key"));
        assert!(status_message(503).contains("limit"));
        assert!(parse_geoposition("{}").is_err());
        assert!(parse_current("[]").is_err());
    }
}
