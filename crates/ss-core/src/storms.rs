//! Severe-storm data: NEXRAD storm cells (from each radar's storm-cell
//! identification and tracking algorithms) and local storm reports.

use serde::{Deserialize, Serialize};

/// Tornado vortex signature on a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Tvs {
    #[default]
    None,
    /// Elevated TVS (aloft only).
    Elevated,
    /// TVS reaching the lowest scans.
    Tvs,
}

/// Where a cell came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CellSource {
    /// A US WSR-88D's storm-cell algorithms (with TVS/meso/hail).
    #[default]
    Nexrad,
    /// Found in the global radar mosaic (reflectivity and motion only).
    Mosaic,
}

/// One storm cell, from a NEXRAD radar or the global radar mosaic.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StormCell {
    /// Detecting radar (e.g. `TLX`).
    pub radar: String,
    /// Cell id within that radar (e.g. `G5`).
    pub id: String,
    pub lon: f64,
    pub lat: f64,
    pub tvs: Tvs,
    /// Mesocyclone strength rank (0 = none).
    pub meso: u8,
    /// Probability of severe hail, %.
    pub posh: f64,
    /// Probability of hail, %.
    pub poh: f64,
    /// Maximum expected hail size, inches.
    pub max_hail_in: f64,
    /// Vertically integrated liquid, kg/m².
    pub vil: f64,
    pub max_dbz: f64,
    /// Height of the max reflectivity, thousands of feet.
    pub max_dbz_height_kft: f64,
    /// Echo top, thousands of feet.
    pub top_kft: f64,
    /// Direction the cell is moving *from*, degrees (radar convention).
    pub motion_from_deg: f64,
    /// Speed, knots.
    pub speed_kt: f64,
    /// Observation time, RFC 3339.
    pub valid: String,
    #[serde(default)]
    pub source: CellSource,
    /// Area of the ≥ 40 dBZ core, km² (mosaic cells; 0 when unknown).
    #[serde(default)]
    pub area_km2: f64,
}

impl StormCell {
    /// Direction of travel (degrees), opposite the "from" convention.
    pub fn heading_deg(&self) -> f64 {
        (self.motion_from_deg + 180.0).rem_euclid(360.0)
    }

    /// Where the cell will be after `minutes` on its current motion.
    pub fn projected(&self, minutes: f64) -> (f64, f64) {
        let km = self.speed_kt * 1.852 * minutes / 60.0;
        crate::geo::destination(self.lon, self.lat, self.heading_deg(), km)
    }

    /// 0 = ordinary … 4 = tornadic: drives colour and sort order.
    pub fn threat(&self) -> u8 {
        if self.tvs == Tvs::Tvs {
            4
        } else if self.tvs == Tvs::Elevated || self.meso >= 5 {
            3
        } else if self.meso > 0 || self.max_hail_in >= 1.0 || self.posh >= 50.0 {
            2
        } else if self.max_dbz >= 50.0 || self.poh >= 50.0 {
            1
        } else {
            0
        }
    }

    /// Minutes until the cell, on its current motion, passes closest to
    /// (`lat`, `lon`), and that closest distance in km; None when it's
    /// moving away or barely moving.
    pub fn closest_approach(&self, lat: f64, lon: f64) -> Option<(f64, f64)> {
        if self.speed_kt < 3.0 {
            return None;
        }
        // Local flat-earth frame around the place, km.
        let kx = 111.32 * lat.to_radians().cos();
        let (x, y) = ((self.lon - lon) * kx, (self.lat - lat) * 110.57);
        let h = self.heading_deg().to_radians();
        let v = self.speed_kt * 1.852 / 60.0; // km per minute
        let (vx, vy) = (h.sin() * v, h.cos() * v);
        let t = -(x * vx + y * vy) / (vx * vx + vy * vy);
        if t <= 0.0 {
            return None;
        }
        let (cx, cy) = (x + vx * t, y + vy * t);
        Some((t, (cx * cx + cy * cy).sqrt()))
    }

    /// `TLX/G5`: the radar and the cell's id there.
    pub fn key(&self) -> String {
        format!("{}/{}", self.radar, self.id)
    }

    /// Short description of what makes the cell notable.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        match self.tvs {
            Tvs::Tvs => parts.push("TVS".to_string()),
            Tvs::Elevated => parts.push("ETVS".to_string()),
            Tvs::None => {}
        }
        if self.meso > 0 {
            parts.push(format!("MESO {}", self.meso));
        }
        if self.max_hail_in > 0.0 {
            parts.push(format!(
                "hail {:.2}\" ({:.0}% svr)",
                self.max_hail_in, self.posh
            ));
        }
        parts.push(format!("{:.0} dBZ", self.max_dbz));
        if self.area_km2 > 0.0 {
            parts.push(format!("{:.0} km² core", self.area_km2));
        }
        if self.top_kft > 0.0 {
            parts.push(format!("top {:.0}k ft", self.top_kft));
        }
        parts.join(" · ")
    }
}

/// A storm expected at a place: passing within [`ARRIVAL_KM`] in the next
/// [`ARRIVAL_MINUTES`], or already there.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrival {
    pub place_id: String,
    pub place: String,
    pub cell: StormCell,
    /// Minutes to the closest approach (0 when it's already overhead).
    pub minutes: f64,
    /// How close it passes, km.
    pub miss_km: f64,
}

/// A cell counts as arriving when its centre passes this close, km.
pub const ARRIVAL_KM: f64 = 10.0;
/// How far ahead arrivals are looked for, minutes.
pub const ARRIVAL_MINUTES: f64 = 60.0;

/// Cells this close together are one storm seen by several radars.
const SAME_STORM_KM: f64 = 8.0;

/// The storms heading for each place, most threatening then soonest first
/// per place; only cells worth knowing about (threat ≥ 1 or ≥ 45 dBZ). A
/// storm several radars track is listed once (its most severe reading).
pub fn arrivals(places: &[crate::types::SavedLocation], cells: &[StormCell]) -> Vec<Arrival> {
    let mut out: Vec<Arrival> = Vec::new();
    for pl in places {
        let first = out.len();
        for c in cells
            .iter()
            .filter(|c| c.threat() >= 1 || c.max_dbz >= 45.0)
        {
            let now_km = crate::geo::haversine_km(pl.lat, pl.lon, c.lat, c.lon);
            let hit = if now_km <= ARRIVAL_KM {
                Some((0.0, now_km))
            } else {
                c.closest_approach(pl.lat, pl.lon)
                    .filter(|(t, miss)| *t <= ARRIVAL_MINUTES && *miss <= ARRIVAL_KM)
            };
            let Some((minutes, miss_km)) = hit else {
                continue;
            };
            let a = Arrival {
                place_id: pl.id.clone(),
                place: pl.label.clone(),
                cell: c.clone(),
                minutes,
                miss_km,
            };
            let twin = out[first..].iter().position(|o| {
                crate::geo::haversine_km(o.cell.lat, o.cell.lon, c.lat, c.lon) <= SAME_STORM_KM
            });
            match twin {
                Some(i) if out[first + i].cell.threat() < c.threat() => out[first + i] = a,
                Some(_) => {}
                None => out.push(a),
            }
        }
    }
    out.sort_by(|a, b| {
        a.place
            .cmp(&b.place)
            .then(b.cell.threat().cmp(&a.cell.threat()))
            .then(a.minutes.total_cmp(&b.minutes))
    });
    out
}

/// "passes overhead" / "passes 4 km away".
pub fn miss_label(km: f64) -> String {
    if km < 1.5 {
        "passes overhead".to_string()
    } else {
        format!("passes {km:.0} km away")
    }
}

/// "now", "~5 min", "~1 h 05 min".
pub fn eta_label(minutes: f64) -> String {
    let m = minutes.round() as i64;
    match m {
        ..=1 => "now".to_string(),
        2..=59 => format!("~{m} min"),
        _ => format!("~{} h {:02} min", m / 60, m % 60),
    }
}

/// A tropical cyclone (GDACS: JTWC / NOAA advisories worldwide).
/// One leg of a cyclone track: (from lon/lat, to lon/lat, storm class
/// label, is forecast).
pub type TrackSegment = ((f64, f64), (f64, f64), String, bool);
/// A wind swath: (label like `120 km/h`, alert colour name, outer ring).
pub type WindSwath = (String, String, Vec<(f64, f64)>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TropicalCyclone {
    pub id: String,
    /// `AKVAN-26`.
    pub name: String,
    /// GDACS alert level: Green, Orange or Red.
    pub alert: String,
    /// `Tropical Storm (maximum wind speed of 83 km/h)`.
    pub category: String,
    pub max_wind_kmh: f64,
    /// Current centre.
    pub lon: f64,
    pub lat: f64,
    pub country: String,
    /// Advisory source (JTWC, NOAA…).
    pub source: String,
    /// Last update, local to GDACS (UTC), `YYYY-MM-DDTHH:MM:SS`.
    pub updated: String,
    /// Track segments: (from, to, storm class label, is forecast).
    pub track: Vec<TrackSegment>,
    /// Wind swaths: (label like `120 km/h`, alert colour name, outer ring).
    pub swaths: Vec<WindSwath>,
    pub report_url: String,
}

impl TropicalCyclone {
    /// Alert colour (GDACS green/orange/red).
    pub fn color(&self) -> [u8; 3] {
        alert_color(&self.alert)
    }
}

/// GDACS alert level → colour.
pub fn alert_color(level: &str) -> [u8; 3] {
    match level.to_ascii_lowercase().as_str() {
        "red" => [0xff, 0x2a, 0x2a],
        "orange" => [0xff, 0x9f, 0x1c],
        _ => [0x2e, 0xd1, 0x5a],
    }
}

/// What a local storm report describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportKind {
    Tornado,
    FunnelCloud,
    Hail,
    Wind,
    Flood,
    Snow,
    Other,
}

impl ReportKind {
    /// From the NWS LSR type code.
    pub fn from_code(code: &str, text: &str) -> ReportKind {
        let t = text.to_ascii_uppercase();
        match code {
            "T" => ReportKind::Tornado,
            "C" => ReportKind::FunnelCloud,
            "H" => ReportKind::Hail,
            "G" | "D" | "M" | "N" | "O" => ReportKind::Wind,
            "F" | "E" | "x" => ReportKind::Flood,
            "S" | "R" | "Z" | "s" => {
                if t.contains("RAIN") {
                    ReportKind::Other
                } else {
                    ReportKind::Snow
                }
            }
            _ if t.contains("TORNADO") => ReportKind::Tornado,
            _ if t.contains("HAIL") => ReportKind::Hail,
            _ if t.contains("WND") || t.contains("WIND") => ReportKind::Wind,
            _ if t.contains("FLOOD") => ReportKind::Flood,
            _ => ReportKind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ReportKind::Tornado => "Tornado",
            ReportKind::FunnelCloud => "Funnel cloud",
            ReportKind::Hail => "Hail",
            ReportKind::Wind => "Wind",
            ReportKind::Flood => "Flood",
            ReportKind::Snow => "Snow/ice",
            ReportKind::Other => "Other",
        }
    }

    /// Map symbol letter.
    pub fn letter(self) -> &'static str {
        match self {
            ReportKind::Tornado => "T",
            ReportKind::FunnelCloud => "C",
            ReportKind::Hail => "H",
            ReportKind::Wind => "W",
            ReportKind::Flood => "F",
            ReportKind::Snow => "S",
            ReportKind::Other => "•",
        }
    }

    /// Conventional storm-report colours (SPC style).
    pub fn color(self) -> [u8; 3] {
        match self {
            ReportKind::Tornado => [0xff, 0x2a, 0x2a],
            ReportKind::FunnelCloud => [0xff, 0x8c, 0x8c],
            ReportKind::Hail => [0x2e, 0xd1, 0x5a],
            ReportKind::Wind => [0x3d, 0x8b, 0xff],
            ReportKind::Flood => [0x14, 0xc8, 0xc8],
            ReportKind::Snow => [0xd8, 0xd8, 0xff],
            ReportKind::Other => [0xb0, 0xb0, 0xb0],
        }
    }
}

/// A local storm report (spotter, emergency manager, public…).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StormReport {
    pub kind: ReportKind,
    /// The NWS event type as written (`TORNADO`, `HAIL`, `TSTM WND GST`…).
    pub event: String,
    pub magnitude: Option<f64>,
    pub unit: String,
    pub place: String,
    pub state: String,
    pub source: String,
    pub remark: String,
    /// Report time, RFC 3339.
    pub valid: String,
    pub lon: f64,
    pub lat: f64,
}

impl StormReport {
    /// `Hail 1.75 in`, `Wind 70 MPH`, `Tornado`.
    pub fn headline(&self) -> String {
        match self.magnitude {
            Some(m) if m > 0.0 => {
                let unit = self.unit.to_ascii_lowercase();
                let unit = if unit == "inch" {
                    "in".to_string()
                } else {
                    unit
                };
                format!("{} {} {}", self.event_title(), trim_num(m), unit)
            }
            _ => self.event_title(),
        }
    }

    fn event_title(&self) -> String {
        let e = self.event.to_ascii_lowercase();
        let mut c = e.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => self.kind.label().to_string(),
        }
    }
}

fn trim_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell() -> StormCell {
        StormCell {
            radar: "TLX".into(),
            id: "G5".into(),
            lon: -97.5,
            lat: 35.2,
            tvs: Tvs::None,
            meso: 0,
            posh: 0.0,
            poh: 0.0,
            max_hail_in: 0.0,
            vil: 10.0,
            max_dbz: 45.0,
            max_dbz_height_kft: 12.0,
            top_kft: 30.0,
            motion_from_deg: 240.0,
            speed_kt: 30.0,
            valid: "2026-11-18T11:00:00Z".into(),
            source: CellSource::Nexrad,
            area_km2: 0.0,
        }
    }

    #[test]
    fn projects_downstream_of_the_from_direction() {
        let c = cell();
        assert_eq!(c.heading_deg(), 60.0);
        let (lon, lat) = c.projected(60.0);
        // Moving toward the ENE: east and a little north.
        assert!(lon > c.lon && lat > c.lat);
        // 30 kt for an hour ≈ 55.6 km.
        let km = crate::geo::haversine_km(c.lat, c.lon, lat, lon);
        assert!((km - 55.56).abs() < 0.5, "{km}");
    }

    #[test]
    fn threat_ranks_tornadic_cells_highest() {
        let mut c = cell();
        assert_eq!(c.threat(), 0);
        c.max_dbz = 55.0;
        assert_eq!(c.threat(), 1);
        c.max_hail_in = 1.25;
        assert_eq!(c.threat(), 2);
        c.meso = 6;
        assert_eq!(c.threat(), 3);
        c.tvs = Tvs::Tvs;
        assert_eq!(c.threat(), 4);
        assert!(c.summary().starts_with("TVS · MESO 6 · hail 1.25\""));
    }

    #[test]
    fn report_kinds_and_headlines() {
        assert_eq!(ReportKind::from_code("T", "TORNADO"), ReportKind::Tornado);
        assert_eq!(ReportKind::from_code("H", "HAIL"), ReportKind::Hail);
        assert_eq!(
            ReportKind::from_code("N", "NON-TSTM WND GST"),
            ReportKind::Wind
        );
        assert_eq!(ReportKind::from_code("R", "HEAVY RAIN"), ReportKind::Other);
        assert_eq!(ReportKind::from_code("?", "TORNADO"), ReportKind::Tornado);
        let r = StormReport {
            kind: ReportKind::Hail,
            event: "HAIL".into(),
            magnitude: Some(1.75),
            unit: "Inch".into(),
            place: "2 N Norman".into(),
            state: "OK".into(),
            source: "Trained Spotter".into(),
            remark: String::new(),
            valid: "2026-11-18T10:00:00Z".into(),
            lon: -97.4,
            lat: 35.3,
        };
        assert_eq!(r.headline(), "Hail 1.75 in");
    }

    fn moving_cell(lon: f64, lat: f64, from: f64, kt: f64, hail: f64) -> StormCell {
        StormCell {
            radar: "TLX".into(),
            id: "A1".into(),
            lon,
            lat,
            max_hail_in: hail,
            max_dbz: 55.0,
            motion_from_deg: from,
            speed_kt: kt,
            ..Default::default()
        }
    }

    #[test]
    fn approaching_cell_has_eta_receding_cell_none() {
        // 30 km due west, moving from the west (heading east) at 30 kt.
        let c = moving_cell(-97.8, 35.2, 270.0, 30.0, 0.0);
        let (mins, miss) = c.closest_approach(35.2, -97.47).unwrap();
        assert!(miss < 2.0, "{miss}");
        // ~30 km at 55.6 km/h ≈ 32 min.
        assert!((mins - 32.4).abs() < 3.0, "{mins}");
        assert!(moving_cell(-97.8, 35.2, 90.0, 30.0, 0.0)
            .closest_approach(35.2, -97.47)
            .is_none());
        assert!(moving_cell(-97.8, 35.2, 270.0, 0.0, 0.0)
            .closest_approach(35.2, -97.47)
            .is_none());
    }

    #[test]
    fn finds_arrivals_at_each_place() {
        let place = |id: &str, lat, lon| crate::types::SavedLocation {
            id: id.into(),
            label: id.into(),
            lat,
            lon,
            accu_weather_location_key: None,
        };
        let places = [place("norman", 35.2, -97.47), place("far", 40.0, -90.0)];
        let cells = [
            moving_cell(-97.8, 35.2, 270.0, 30.0, 1.5), // ~32 min to Norman
            moving_cell(-97.47, 35.25, 0.0, 0.0, 0.0),  // overhead, stationary
            moving_cell(-99.5, 35.2, 270.0, 30.0, 0.0), // 3+ h away
            // The hail storm again, as a neighbouring radar sees it.
            StormCell {
                radar: "VNX".into(),
                ..moving_cell(-97.79, 35.21, 270.0, 30.0, 0.0)
            },
        ];
        let a = arrivals(&places, &cells);
        assert_eq!(a.len(), 2, "{a:?}");
        assert!(a.iter().all(|x| x.place_id == "norman"));
        // Hail cell first (higher threat), then the one overhead.
        assert_eq!(a[0].cell.max_hail_in, 1.5);
        assert_eq!(a[1].minutes, 0.0);
        assert_eq!(eta_label(32.4), "~32 min");
        assert_eq!(eta_label(0.0), "now");
        assert_eq!(eta_label(65.0), "~1 h 05 min");
        assert_eq!(miss_label(0.4), "passes overhead");
        assert_eq!(miss_label(4.2), "passes 4 km away");
    }
}
