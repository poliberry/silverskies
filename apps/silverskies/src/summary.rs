//! Plain-language forecast summaries and quick answers, written locally from
//! the forecast data — no network or AI needed. When AI is enabled these
//! are still shown first and the AI's take is added underneath.

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use ss_core::weather::units::{fmt_temp, fmt_time, fmt_wind};
use ss_core::weather::wmo::wmo_label;
use ss_core::{Alert, DailyPoint, HourlyPoint, TimeFormat, UnitPref, Weather};

use crate::search::{DayRef, Topic};

/// WMO codes that mean something is falling from the sky.
pub fn is_wet_code(code: u8) -> bool {
    matches!(code, 51..=67 | 80..=82 | 95..=99)
}

pub fn is_snow_code(code: u8) -> bool {
    matches!(code, 71..=77 | 85 | 86)
}

/// Index of the hourly point for `now` (the last hour not after it).
pub fn current_hour_index(hourly: &[HourlyPoint], now: NaiveDateTime) -> usize {
    hourly
        .iter()
        .rposition(|h| parse_local(&h.time).is_some_and(|t| t <= now))
        .unwrap_or(0)
}

fn parse_local(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").ok()
}

/// The daily entry a day reference points at.
pub fn day_index(daily: &[DailyPoint], day: DayRef) -> Option<usize> {
    match day {
        DayRef::Today => (!daily.is_empty()).then_some(0),
        DayRef::Tomorrow => (daily.len() > 1).then_some(1),
        DayRef::Weekday(n) => daily.iter().position(|d| {
            NaiveDate::parse_from_str(&d.date, "%Y-%m-%d")
                .is_ok_and(|x| x.weekday().num_days_from_monday() == n)
        }),
    }
}

/// `Today`, `Tomorrow` or the weekday name for a daily index.
pub fn day_name(daily: &[DailyPoint], i: usize) -> String {
    match i {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        _ => daily
            .get(i)
            .and_then(|d| NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").ok())
            .map(|d| d.format("%A").to_string())
            .unwrap_or_else(|| "That day".into()),
    }
}

fn deg(c: f64, u: UnitPref) -> String {
    // "72°" — the unit is obvious from context in running text.
    let s = fmt_temp(c, u);
    s.trim_end_matches(['F', 'C']).to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub headline: String,
    pub points: Vec<String>,
}

/// A short, friendly summary of the forecast.
pub fn summarize(
    w: &Weather,
    alerts: &[Alert],
    units: UnitPref,
    tf: TimeFormat,
    now: NaiveDateTime,
) -> Summary {
    let c = &w.current;
    let mut headline = format!(
        "{} and {} right now",
        wmo_label(c.weather_code),
        deg(c.temperature_c, units)
    );
    if let Some(d) = w.daily.first() {
        headline.push_str(&format!(
            ", heading for a high of {} and a low of {}.",
            deg(d.temp_max_c, units),
            deg(d.temp_min_c, units)
        ));
    } else {
        headline.push('.');
    }

    let mut points = Vec::new();
    if let Some(a) = alerts.first() {
        let more = alerts.len() - 1;
        points.push(if more == 0 {
            format!("{} is in effect.", a.display_event)
        } else {
            format!(
                "{} and {more} other alert{} in effect.",
                a.display_event,
                if more == 1 { " is" } else { "s are" }
            )
        });
    }

    points.push(precip_outlook(w, tf, now));

    if (c.apparent_temperature_c - c.temperature_c).abs() >= 3.0 {
        points.push(format!(
            "It feels like {} because of the {}.",
            deg(c.apparent_temperature_c, units),
            if c.apparent_temperature_c > c.temperature_c {
                "humidity"
            } else {
                "wind"
            }
        ));
    }
    if c.wind_speed_kmh >= 30.0 {
        points.push(format!(
            "Breezy, with winds around {}.",
            fmt_wind(c.wind_speed_kmh, units)
        ));
    }
    if let (Some(t), Some(m)) = (w.daily.first(), w.daily.get(1)) {
        let diff = m.temp_max_c - t.temp_max_c;
        if diff.abs() >= 4.0 {
            points.push(format!(
                "Tomorrow turns {} (high {}).",
                if diff > 0.0 {
                    "noticeably warmer"
                } else {
                    "noticeably cooler"
                },
                deg(m.temp_max_c, units)
            ));
        }
    }
    if let Some(t) = w.daily.first() {
        if t.uv_index_max >= 6.0 && c.is_day {
            points.push(format!(
                "UV peaks around {:.0} today — sunscreen is a good idea.",
                t.uv_index_max
            ));
        }
    }
    if let Some((i, d)) = w
        .daily
        .iter()
        .enumerate()
        .skip(1)
        .take(6)
        .filter(|(_, d)| d.precipitation_probability_max_pct >= 60.0)
        .max_by(|a, b| {
            a.1.precipitation_probability_max_pct
                .total_cmp(&b.1.precipitation_probability_max_pct)
        })
    {
        points.push(format!(
            "Wettest day ahead: {} ({}% chance of {}).",
            day_name(&w.daily, i),
            d.precipitation_probability_max_pct.round(),
            if is_snow_code(d.weather_code) {
                "snow"
            } else {
                "rain"
            }
        ));
    }
    Summary { headline, points }
}

/// When the next rain starts (or the current rain stops), within 12 hours.
fn precip_outlook(w: &Weather, tf: TimeFormat, now: NaiveDateTime) -> String {
    let i = current_hour_index(&w.hourly, now);
    let next: Vec<&HourlyPoint> = w.hourly.iter().skip(i).take(13).collect();
    let wet_now = is_wet_code(w.current.weather_code) || is_snow_code(w.current.weather_code);
    let kind = if is_snow_code(w.current.weather_code) {
        "Snow"
    } else {
        "Rain"
    };
    if wet_now {
        return match next
            .iter()
            .skip(1)
            .find(|h| h.precipitation_probability_pct < 30.0)
        {
            Some(h) => format!("{kind} should ease around {}.", fmt_time(&h.time, tf)),
            None => format!("{kind} looks set to continue for a while."),
        };
    }
    match next
        .iter()
        .skip(1)
        .find(|h| h.precipitation_probability_pct >= 50.0)
    {
        Some(h) => format!(
            "{} likely around {} ({}% chance).",
            if is_snow_code(h.weather_code) {
                "Snow"
            } else {
                "Rain"
            },
            fmt_time(&h.time, tf),
            h.precipitation_probability_pct.round()
        ),
        None => "No rain expected in the next 12 hours.".into(),
    }
}

/// A one-or-two sentence answer to a weather question about `day`.
pub fn answer(
    w: &Weather,
    topic: Topic,
    day: Option<DayRef>,
    units: UnitPref,
    tf: TimeFormat,
    now: NaiveDateTime,
) -> String {
    let di = day.and_then(|d| day_index(&w.daily, d));
    let is_today = di.is_none_or(|i| i == 0);
    let Some(d) = w.daily.get(di.unwrap_or(0)) else {
        return "No forecast data yet.".into();
    };
    let when = day_name(&w.daily, di.unwrap_or(0));
    let pop = d.precipitation_probability_max_pct.round();
    let c = &w.current;
    match topic {
        Topic::Rain | Topic::Umbrella => {
            if is_today && is_wet_code(c.weather_code) {
                let base = format!(
                    "Yes — it's {} right now.",
                    wmo_label(c.weather_code).to_lowercase()
                );
                if topic == Topic::Umbrella {
                    return format!("{base} Take an umbrella.");
                }
                return format!("{base} {}", precip_outlook(w, tf, now));
            }
            let likely = pop >= 50.0;
            let verdict = match (topic, likely, pop >= 25.0) {
                (Topic::Umbrella, true, _) => "Yes, bring an umbrella.",
                (Topic::Umbrella, false, true) => "Maybe — a compact umbrella wouldn't hurt.",
                (Topic::Umbrella, false, false) => "Probably not.",
                (_, true, _) => "Rain is likely.",
                (_, false, true) => "There's a chance of rain.",
                _ => "Rain isn't expected.",
            };
            let mut s = format!("{verdict} {when}: {pop}% chance of precipitation.");
            if is_today {
                s.push(' ');
                s.push_str(&precip_outlook(w, tf, now));
            }
            s
        }
        Topic::Snow => {
            if is_today && is_snow_code(c.weather_code) {
                return format!(
                    "Yes — {} right now.",
                    wmo_label(c.weather_code).to_lowercase()
                );
            }
            let cold = d.temp_min_c <= 1.0;
            if is_snow_code(d.weather_code) {
                format!(
                    "Snow is in the forecast for {} ({pop}% chance).",
                    when.to_lowercase()
                )
            } else if cold && pop >= 40.0 {
                format!(
                    "Possibly — it's cold enough and there's a {pop}% chance of precipitation {}.",
                    when.to_lowercase()
                )
            } else {
                format!("Snow isn't expected {}.", when.to_lowercase())
            }
        }
        Topic::Storm => {
            let stormy = (95..=99).contains(&d.weather_code)
                || (is_today && (95..=99).contains(&c.weather_code));
            if stormy {
                format!(
                    "Yes — thunderstorms are possible {}. Keep an eye on alerts.",
                    when.to_lowercase()
                )
            } else {
                format!("No thunderstorms in the forecast {}.", when.to_lowercase())
            }
        }
        Topic::Wind => {
            if is_today {
                let v = c.wind_speed_kmh;
                let feel = if v >= 50.0 {
                    "Very windy"
                } else if v >= 30.0 {
                    "Breezy"
                } else if v >= 15.0 {
                    "A light breeze"
                } else {
                    "Calm"
                };
                format!("{feel} right now — {}.", fmt_wind(v, units))
            } else {
                format!("{when} wind data isn't in the daily forecast; check closer to the day.")
            }
        }
        Topic::Hot | Topic::Cold | Topic::Jacket => {
            let hi = deg(d.temp_max_c, units);
            let lo = deg(d.temp_min_c, units);
            let verdict = match topic {
                Topic::Hot if d.temp_max_c >= 30.0 => "Yes, it's a hot one.",
                Topic::Hot if d.temp_max_c >= 24.0 => "Warm, but not scorching.",
                Topic::Hot => "Not really.",
                Topic::Cold if d.temp_min_c <= 0.0 => "Yes — expect freezing temperatures.",
                Topic::Cold if d.temp_max_c <= 10.0 => "Yes, it'll be chilly.",
                Topic::Cold => "Not especially.",
                _ if d.temp_max_c <= 12.0 => "Yes, wear a warm jacket.",
                _ if d.temp_min_c <= 15.0 => "A light jacket for the morning and evening.",
                _ => "You can probably leave it at home.",
            };
            format!("{verdict} {when}: high {hi}, low {lo}.")
        }
        Topic::General => format!(
            "{when}: {}, high {}, low {}, {pop}% chance of precipitation.",
            wmo_label(d.weather_code),
            deg(d.temp_max_c, units),
            deg(d.temp_min_c, units)
        ),
    }
}

#[cfg(test)]
pub mod fixtures {
    use ss_core::{CurrentConditions, DailyPoint, HourlyPoint, Weather, WeatherProviderId};

    /// A Monday 2026-08-10 forecast: sunny now, rain from 15:00.
    pub fn weather() -> Weather {
        let hourly = (0..48)
            .map(|h| HourlyPoint {
                time: format!("2026-08-{:02}T{:02}:00", 10 + h / 24, h % 24),
                temperature_c: 20.0 + (h % 24) as f64 / 2.0,
                weather_code: if (15..18).contains(&(h % 24)) { 61 } else { 1 },
                is_day: (6..20).contains(&(h % 24)),
                precipitation_probability_pct: if (15..18).contains(&(h % 24)) {
                    70.0
                } else {
                    5.0
                },
            })
            .collect();
        let daily = (0..7)
            .map(|d| DailyPoint {
                date: format!("2026-08-{:02}", 10 + d),
                weather_code: if d == 3 { 63 } else { 2 },
                temp_max_c: if d == 1 { 31.0 } else { 25.0 },
                temp_min_c: 14.0,
                precipitation_probability_max_pct: if d == 3 {
                    85.0
                } else {
                    70.0 * f64::from(u8::from(d == 0))
                },
                sunrise: format!("2026-08-{:02}T06:10", 10 + d),
                sunset: format!("2026-08-{:02}T20:05", 10 + d),
                uv_index_max: 7.0,
            })
            .collect();
        Weather {
            current: CurrentConditions {
                temperature_c: 22.0,
                apparent_temperature_c: 22.5,
                weather_code: 1,
                is_day: true,
                relative_humidity_pct: 55.0,
                wind_speed_kmh: 12.0,
                wind_direction_deg: 200.0,
                precipitation_mm: 0.0,
                cloud_cover_pct: 20.0,
                uv_index: 5.0,
            },
            hourly,
            daily,
            timezone: "UTC".into(),
            provider: WeatherProviderId::OpenMeteo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::weather;
    use super::*;

    fn now() -> NaiveDateTime {
        NaiveDateTime::parse_from_str("2026-08-10T10:30", "%Y-%m-%dT%H:%M").unwrap()
    }

    #[test]
    fn hour_index_tracks_now() {
        let w = weather();
        assert_eq!(current_hour_index(&w.hourly, now()), 10);
    }

    #[test]
    fn day_refs_resolve() {
        let w = weather();
        assert_eq!(day_index(&w.daily, DayRef::Today), Some(0));
        assert_eq!(day_index(&w.daily, DayRef::Tomorrow), Some(1));
        // 2026-08-13 is a Thursday.
        assert_eq!(day_index(&w.daily, DayRef::Weekday(3)), Some(3));
        assert_eq!(day_name(&w.daily, 3), "Thursday");
    }

    #[test]
    fn summary_mentions_rain_warmth_and_wettest_day() {
        let w = weather();
        let s = summarize(&w, &[], UnitPref::C, TimeFormat::H24, now());
        assert!(
            s.headline.starts_with("Mainly Clear and 22°"),
            "{}",
            s.headline
        );
        let all = s.points.join(" ");
        assert!(all.contains("Rain likely around 15:00"), "{all}");
        assert!(all.contains("noticeably warmer"), "{all}");
        assert!(all.contains("Wettest day ahead: Thursday"), "{all}");
        assert!(all.contains("UV peaks"), "{all}");
    }

    #[test]
    fn answers_rain_and_umbrella() {
        let w = weather();
        let a = answer(
            &w,
            Topic::Umbrella,
            Some(DayRef::Today),
            UnitPref::C,
            TimeFormat::H24,
            now(),
        );
        assert!(a.starts_with("Yes, bring an umbrella."), "{a}");
        let a = answer(
            &w,
            Topic::Rain,
            Some(DayRef::Tomorrow),
            UnitPref::C,
            TimeFormat::H24,
            now(),
        );
        assert!(a.starts_with("Rain isn't expected."), "{a}");
        let a = answer(
            &w,
            Topic::Hot,
            Some(DayRef::Tomorrow),
            UnitPref::C,
            TimeFormat::H24,
            now(),
        );
        assert!(a.starts_with("Yes, it's a hot one."), "{a}");
    }
}
