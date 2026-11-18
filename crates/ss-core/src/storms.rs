//! Severe-storm data: NEXRAD storm cells (from each radar's storm-cell
//! identification and tracking algorithms) and local storm reports.

use serde::{Deserialize, Serialize};

/// Tornado vortex signature on a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tvs {
    None,
    /// Elevated TVS (aloft only).
    Elevated,
    /// TVS reaching the lowest scans.
    Tvs,
}

/// One storm cell as detected by a NEXRAD radar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        if self.top_kft > 0.0 {
            parts.push(format!("top {:.0}k ft", self.top_kft));
        }
        parts.join(" · ")
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
}
