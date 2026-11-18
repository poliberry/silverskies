//! Compact plain-text rendering of a forecast (and its alerts) for prompts.

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use ss_core::weather::units::{fmt_temp, fmt_time, fmt_wind};
use ss_core::weather::wmo::wmo_label;
use ss_core::{Alert, TimeFormat, UnitPref, Weather};

/// "Now" in the forecast's own time zone (hourly times are local, no offset).
pub fn local_now(w: &Weather) -> NaiveDateTime {
    match w.timezone.parse::<chrono_tz::Tz>() {
        Ok(tz) => chrono::Utc::now().with_timezone(&tz).naive_local(),
        Err(_) => chrono::Local::now().naive_local(),
    }
}

fn hour_index(w: &Weather, now: NaiveDateTime) -> usize {
    w.hourly
        .iter()
        .rposition(|h| {
            NaiveDateTime::parse_from_str(&h.time, "%Y-%m-%dT%H:%M").is_ok_and(|t| t <= now)
        })
        .unwrap_or(0)
}

fn day_name(w: &Weather, i: usize) -> String {
    match i {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        _ => w
            .daily
            .get(i)
            .and_then(|d| NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").ok())
            .map(|d| format!("{} {}", d.format("%A"), d.day()))
            .unwrap_or_else(|| "Later".into()),
    }
}

/// The forecast for `place` as prompt text. `now` is local time at the place.
pub fn forecast_digest(
    w: &Weather,
    place: &str,
    alerts: &[Alert],
    units: UnitPref,
    tf: TimeFormat,
    now: NaiveDateTime,
) -> String {
    let c = &w.current;
    let mut s = format!(
        "Place: {place}\nLocal time: {}\nNow: {}, {} (feels {}), humidity {:.0}%, wind {}, UV {:.0}, cloud {:.0}%\n",
        now.format("%A %Y-%m-%d %H:%M"),
        wmo_label(c.weather_code),
        fmt_temp(c.temperature_c, units),
        fmt_temp(c.apparent_temperature_c, units),
        c.relative_humidity_pct,
        fmt_wind(c.wind_speed_kmh, units),
        c.uv_index,
        c.cloud_cover_pct,
    );
    s.push_str("Next 12 hours:\n");
    let i = hour_index(w, now);
    for h in w.hourly.iter().skip(i).take(12) {
        s.push_str(&format!(
            "  {}: {}, {}, {:.0}% precip\n",
            fmt_time(&h.time, tf),
            wmo_label(h.weather_code),
            fmt_temp(h.temperature_c, units),
            h.precipitation_probability_pct
        ));
    }
    s.push_str("Daily:\n");
    for (i, d) in w.daily.iter().enumerate().take(7) {
        s.push_str(&format!(
            "  {}: {}, high {}, low {}, {:.0}% precip, UV {:.0}, sunrise {}, sunset {}\n",
            day_name(w, i),
            wmo_label(d.weather_code),
            fmt_temp(d.temp_max_c, units),
            fmt_temp(d.temp_min_c, units),
            d.precipitation_probability_max_pct,
            d.uv_index_max,
            fmt_time(&d.sunrise, tf),
            fmt_time(&d.sunset, tf),
        ));
    }
    if alerts.is_empty() {
        s.push_str("Active official alerts: none\n");
    } else {
        s.push_str("Active official alerts:\n");
        for a in alerts.iter().take(8) {
            s.push_str(&format!(
                "  {} ({}){}{}\n",
                a.display_event,
                a.severity.label(),
                a.headline
                    .as_deref()
                    .map(|h| format!(": {h}"))
                    .unwrap_or_default(),
                a.expires
                    .as_deref()
                    .map(|e| format!(" — until {e}"))
                    .unwrap_or_default(),
            ));
        }
    }
    s
}

/// The system prompt for a weather assistant in `app`, grounded in
/// `context` (one or more forecast digests).
pub fn assistant_system_prompt(app: &str, context: &str) -> String {
    format!(
        "You are the weather assistant inside {app}, a desktop weather app. Answer questions \
about weather and the forecasts below in a warm, plain voice, briefly (a few sentences unless \
asked for more), in plain text without markdown headings. Use the forecast data when it is \
relevant and never invent numbers, alerts or places that are not in it; say so when the data \
cannot answer. General weather and climate questions are fine too. You can make mistakes: \
for anything safety-critical (severe storms, floods, fire, heat, travel or outdoor plans in \
dangerous conditions), tell the user to check official warnings from their national weather \
service and follow local authorities.\n\n<forecasts>\n{context}</forecasts>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss_core::{CurrentConditions, DailyPoint, HourlyPoint, WeatherProviderId};

    fn weather() -> Weather {
        Weather {
            current: CurrentConditions {
                temperature_c: 20.0,
                apparent_temperature_c: 19.0,
                weather_code: 2,
                is_day: true,
                relative_humidity_pct: 50.0,
                wind_speed_kmh: 10.0,
                wind_direction_deg: 90.0,
                precipitation_mm: 0.0,
                cloud_cover_pct: 40.0,
                uv_index: 3.0,
            },
            hourly: (0..24)
                .map(|h| HourlyPoint {
                    time: format!("2026-08-10T{h:02}:00"),
                    temperature_c: 15.0 + h as f64 / 2.0,
                    weather_code: 1,
                    is_day: true,
                    precipitation_probability_pct: 10.0,
                })
                .collect(),
            daily: (0..7)
                .map(|d| DailyPoint {
                    date: format!("2026-08-{:02}", 10 + d),
                    weather_code: 3,
                    temp_max_c: 25.0,
                    temp_min_c: 12.0,
                    precipitation_probability_max_pct: 20.0,
                    sunrise: format!("2026-08-{:02}T06:00", 10 + d),
                    sunset: format!("2026-08-{:02}T20:00", 10 + d),
                    uv_index_max: 6.0,
                })
                .collect(),
            timezone: "UTC".into(),
            provider: WeatherProviderId::OpenMeteo,
        }
    }

    #[test]
    fn digest_has_now_hours_days_and_alerts() {
        let now = NaiveDateTime::parse_from_str("2026-08-10T09:30", "%Y-%m-%dT%H:%M").unwrap();
        let d = forecast_digest(
            &weather(),
            "Testville",
            &[],
            UnitPref::C,
            TimeFormat::H24,
            now,
        );
        assert!(d.starts_with("Place: Testville"));
        assert!(d.contains("Now: Partly Cloudy, 20°C"));
        assert!(d.contains("  09:00:"), "{d}");
        assert!(d.contains("Tomorrow:"));
        assert!(d.contains("Active official alerts: none"));
        assert_eq!(d.lines().filter(|l| l.starts_with("  ")).count(), 12 + 7);
    }

    #[test]
    fn system_prompt_carries_context_and_safety_note() {
        let p = assistant_system_prompt("Silver Skies", "Place: X\n");
        assert!(p.contains("Silver Skies"));
        assert!(p.contains("<forecasts>\nPlace: X\n</forecasts>"));
        assert!(p.contains("make mistakes"));
        assert!(p.contains("local authorities"));
    }
}
