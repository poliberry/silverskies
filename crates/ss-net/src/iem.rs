//! Iowa Environmental Mesonet (free, no key): live NEXRAD storm-cell
//! attributes from every WSR-88D (tornado vortex and mesocyclone signatures,
//! hail, echo tops, storm motion) and NWS local storm reports.

use crate::{parse_value, Net, Result};
use serde_json::Value;
use ss_core::storms::{ReportKind, StormCell, StormReport, Tvs};

pub const STORM_CELLS_URL: &str = "https://mesonet.agron.iastate.edu/geojson/nexrad_attr.geojson";

pub fn storm_reports_url(hours: u32) -> String {
    format!("https://mesonet.agron.iastate.edu/geojson/lsr.geojson?hours={hours}")
}

fn features(v: &Value) -> &[Value] {
    v.get("features")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn point(f: &Value) -> Option<(f64, f64)> {
    let c = f.get("geometry")?.get("coordinates")?.as_array()?;
    Some((c.first()?.as_f64()?, c.get(1)?.as_f64()?))
}

fn num(p: &Value, k: &str) -> f64 {
    match p.get(k) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn text(p: &Value, k: &str) -> String {
    p.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub fn parse_storm_cells(json: &str) -> Result<Vec<StormCell>> {
    let v = parse_value(json)?;
    Ok(features(&v)
        .iter()
        .filter_map(|f| {
            let p = f.get("properties")?;
            let (lon, lat) = point(f)?;
            let tvs = match text(p, "tvs").to_ascii_uppercase().as_str() {
                "TVS" => Tvs::Tvs,
                "ETVS" => Tvs::Elevated,
                _ => Tvs::None,
            };
            let meso = text(p, "meso").parse::<u8>().unwrap_or(0);
            Some(StormCell {
                radar: text(p, "nexrad"),
                id: text(p, "storm_id"),
                lon,
                lat,
                tvs,
                meso,
                posh: num(p, "posh"),
                poh: num(p, "poh"),
                max_hail_in: num(p, "max_size"),
                vil: num(p, "vil"),
                max_dbz: num(p, "max_dbz"),
                max_dbz_height_kft: num(p, "max_dbz_height"),
                top_kft: num(p, "top"),
                motion_from_deg: num(p, "drct"),
                speed_kt: num(p, "sknt"),
                valid: text(p, "valid"),
            })
        })
        .collect())
}

pub fn parse_storm_reports(json: &str) -> Result<Vec<StormReport>> {
    let v = parse_value(json)?;
    let mut out: Vec<StormReport> = features(&v)
        .iter()
        .filter_map(|f| {
            let p = f.get("properties")?;
            let (lon, lat) = point(f)?;
            let event = text(p, "typetext");
            let magnitude = p
                .get("magnitude")
                .and_then(|m| match m {
                    Value::Number(n) => n.as_f64(),
                    Value::String(s) => s.trim().parse().ok(),
                    _ => None,
                })
                .or_else(|| p.get("magf").and_then(Value::as_f64));
            Some(StormReport {
                kind: ReportKind::from_code(&text(p, "type"), &event),
                event,
                magnitude,
                unit: text(p, "unit"),
                place: text(p, "city"),
                state: text(p, "st")
                    .chars()
                    .take(2)
                    .collect::<String>()
                    .to_uppercase(),
                source: text(p, "source"),
                remark: text(p, "remark"),
                valid: text(p, "valid"),
                lon,
                lat,
            })
        })
        .collect();
    out.sort_by(|a, b| b.valid.cmp(&a.valid));
    Ok(out)
}

impl Net {
    /// Every storm cell currently tracked by the US NEXRAD network.
    pub async fn storm_cells(&self) -> Result<Vec<StormCell>> {
        parse_storm_cells(&self.get_text(STORM_CELLS_URL, None).await?)
    }

    /// Local storm reports from the last `hours` hours, newest first.
    pub async fn storm_reports(&self, hours: u32) -> Result<Vec<StormReport>> {
        parse_storm_reports(&self.get_text(&storm_reports_url(hours), None).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELLS: &str = r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","id":0,"properties":{"nexrad":"TLX","storm_id":"K4","azimuth":200,"range":40,"tvs":"TVS","meso":"6","posh":70,"poh":100,"max_size":2.25,"vil":62,"max_dbz":68,"max_dbz_height":24.5,"top":48.0,"drct":235,"sknt":38,"valid":"2026-05-20T21:30:12Z"},"geometry":{"type":"Point","coordinates":[-97.6,35.1]}},
      {"type":"Feature","id":1,"properties":{"nexrad":"BBX","storm_id":"G5","azimuth":200,"range":98,"tvs":"NONE","meso":"NONE","posh":0,"poh":0,"max_size":0.0,"vil":4,"max_dbz":46,"max_dbz_height":4.6,"top":9.0,"drct":237,"sknt":37,"valid":"2026-11-18T11:00:21Z"},"geometry":{"type":"Point","coordinates":[-122.0,38.7]}},
      {"type":"Feature","id":2,"properties":{"nexrad":"X"},"geometry":null}]}"#;

    const LSR: &str = r#"{"type":"FeatureCollection","features":[
      {"id":"0","type":"Feature","properties":{"wfo":"OUN","type":"T","magf":null,"county":"Cleveland","typetext":"TORNADO","state":"OK","remark":"Brief touchdown.","city":"3 S Norman","source":"Storm Chaser","unit":null,"valid":"2026-05-20T21:42:00Z","st":"OK","magnitude":null},"geometry":{"type":"Point","coordinates":[-97.44,35.18]}},
      {"id":"1","type":"Feature","properties":{"wfo":"OUN","type":"H","magf":1.75,"county":"Cleveland","typetext":"HAIL","state":"OK","remark":"","city":"Moore","source":"Trained Spotter","unit":"Inch","valid":"2026-05-20T21:50:00Z","st":"OK","magnitude":"1.75"},"geometry":{"type":"Point","coordinates":[-97.48,35.33]}},
      {"id":"2","type":"Feature","properties":{"wfo":"CYS","type":"N","magf":58.0,"typetext":"NON-TSTM WND GST","city":"3 NW Arlington","source":"Department of Hig","unit":"MPH","valid":"2026-11-18T09:38:00Z","st":"WY","magnitude":"58"},"geometry":{"type":"Point","coordinates":[-106.24,41.62]}}]}"#;

    #[test]
    fn parses_storm_cells() {
        let c = parse_storm_cells(CELLS).unwrap();
        assert_eq!(c.len(), 2, "the geometry-less feature is skipped");
        let k4 = &c[0];
        assert_eq!((k4.radar.as_str(), k4.id.as_str()), ("TLX", "K4"));
        assert_eq!(k4.tvs, Tvs::Tvs);
        assert_eq!(k4.meso, 6);
        assert_eq!(k4.max_hail_in, 2.25);
        assert_eq!(k4.top_kft, 48.0);
        assert_eq!((k4.motion_from_deg, k4.speed_kt), (235.0, 38.0));
        assert_eq!((k4.lon, k4.lat), (-97.6, 35.1));
        assert_eq!(k4.threat(), 4);
        assert_eq!(c[1].tvs, Tvs::None);
        assert_eq!(c[1].meso, 0);
    }

    #[test]
    fn parses_storm_reports_newest_first() {
        let r = parse_storm_reports(LSR).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].valid, "2026-11-18T09:38:00Z");
        let tor = r.iter().find(|x| x.kind == ReportKind::Tornado).unwrap();
        assert_eq!(tor.place, "3 S Norman");
        assert_eq!(tor.magnitude, None);
        assert_eq!(tor.headline(), "Tornado");
        let hail = r.iter().find(|x| x.kind == ReportKind::Hail).unwrap();
        assert_eq!(hail.headline(), "Hail 1.75 in");
        assert_eq!(r[0].kind, ReportKind::Wind);
        assert_eq!(r[0].magnitude, Some(58.0));
    }
}
