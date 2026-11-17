//! Google weather icon (V1 set) names and the WMO code mapping.
//!
//! The icons themselves (light and dark variants) are bundled by the
//! `ss-assets` crate; this module only decides which one a code shows.

/// Every icon name [`v1_icon`] can return.
pub const V1_ICONS: [&str; 22] = [
    "clear_day",
    "clear_night",
    "partly_cloudy_day",
    "partly_cloudy_night",
    "mostly_clear_night",
    "cloudy",
    "haze_fog",
    "drizzle",
    "icy",
    "rain_showers",
    "heavy_rain",
    "flurries",
    "snow_showers",
    "heavy_snow",
    "scattered_rain_showers_day",
    "scattered_rain_showers_night",
    "scattered_snow_showers_day",
    "scattered_snow_showers_night",
    "thunderstorms_day",
    "thunderstorms_night",
    "strong_thunderstorms",
    "not_available",
];

/// Map a WMO weather code (and day/night) to a V1 icon name.
pub fn v1_icon(code: u8, is_day: bool) -> &'static str {
    let dn = |day: &'static str, night: &'static str| if is_day { day } else { night };
    match code {
        0 => dn("clear_day", "clear_night"),
        // Mainly clear: V1 has no "mostly clear" daytime icon.
        1 => dn("partly_cloudy_day", "mostly_clear_night"),
        2 => dn("partly_cloudy_day", "partly_cloudy_night"),
        3 => "cloudy",
        45 | 48 => "haze_fog",
        51 | 53 | 55 => "drizzle",
        56 | 57 | 66 | 67 => "icy",
        61 | 63 | 81 => "rain_showers",
        65 | 82 => "heavy_rain",
        71 | 77 => "flurries",
        73 => "snow_showers",
        75 | 86 => "heavy_snow",
        80 => dn("scattered_rain_showers_day", "scattered_rain_showers_night"),
        85 => dn("scattered_snow_showers_day", "scattered_snow_showers_night"),
        95 => dn("thunderstorms_day", "thunderstorms_night"),
        96 | 99 => "strong_thunderstorms",
        _ => "not_available",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_night_variants() {
        assert_eq!(v1_icon(0, true), "clear_day");
        assert_eq!(v1_icon(0, false), "clear_night");
        assert_eq!(v1_icon(1, false), "mostly_clear_night");
        assert_eq!(v1_icon(80, false), "scattered_rain_showers_night");
        assert_eq!(v1_icon(95, true), "thunderstorms_day");
    }

    #[test]
    fn fixed_codes() {
        for (codes, name) in [
            (&[3u8][..], "cloudy"),
            (&[45, 48], "haze_fog"),
            (&[51, 53, 55], "drizzle"),
            (&[56, 57, 66, 67], "icy"),
            (&[61, 63, 81], "rain_showers"),
            (&[65, 82], "heavy_rain"),
            (&[71, 77], "flurries"),
            (&[75, 86], "heavy_snow"),
            (&[96, 99], "strong_thunderstorms"),
        ] {
            for &c in codes {
                assert_eq!(v1_icon(c, true), name, "code {c}");
                assert_eq!(v1_icon(c, false), name, "code {c}");
            }
        }
        assert_eq!(v1_icon(4, true), "not_available");
    }

    #[test]
    fn every_mapped_name_is_listed() {
        for code in 0..=255u8 {
            for day in [true, false] {
                assert!(V1_ICONS.contains(&v1_icon(code, day)));
            }
        }
    }
}
