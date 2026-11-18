//! GDACS (Global Disaster Alert and Coordination System, UN/EU JRC): free,
//! keyless, worldwide tropical cyclone advisories with tracks, forecast
//! tracks and wind swaths.

use crate::{parse_value, Net, Result};
use serde_json::Value;
use ss_core::storms::TropicalCyclone;

pub const EVENTS_URL: &str =
    "https://www.gdacs.org/gdacsapi/api/events/geteventlist/SEARCH?eventlist=TC&alertlevel=green;orange;red";

fn text(p: &Value, k: &str) -> String {
    p.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn pair(c: &Value) -> Option<(f64, f64)> {
    Some((c.get(0)?.as_f64()?, c.get(1)?.as_f64()?))
}

/// Make a lon/lat path continuous across the antimeridian: each point's
/// longitude is shifted by ±360° to stay within 180° of the previous one, so
/// a storm near 180° is drawn as one shape instead of spanning the globe.
pub fn unwrap_lons(pts: &mut [(f64, f64)]) {
    for i in 1..pts.len() {
        let prev = pts[i - 1].0;
        let mut lon = pts[i].0;
        while lon - prev > 180.0 {
            lon -= 360.0;
        }
        while prev - lon > 180.0 {
            lon += 360.0;
        }
        pts[i].0 = lon;
    }
}

/// Current cyclones from the event list: `(event id, episode id, geometry URL)`.
pub fn current_events(json: &str) -> Result<Vec<(String, String, String)>> {
    let v = parse_value(json)?;
    Ok(v.get("features")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|f| {
            let p = f.get("properties")?;
            let current = matches!(p.get("iscurrent"), Some(Value::String(s)) if s == "true")
                || matches!(p.get("iscurrent"), Some(Value::Bool(true)));
            if !current {
                return None;
            }
            let geo = p.get("url")?.get("geometry")?.as_str()?.to_string();
            let id = p.get("eventid").map(|v| v.to_string()).unwrap_or_default();
            let ep = p
                .get("episodeid")
                .map(|v| v.to_string())
                .unwrap_or_default();
            Some((id, ep, geo))
        })
        .collect())
}

/// One cyclone from its geometry document (centroid, track, swaths).
pub fn parse_cyclone(json: &str) -> Result<Option<TropicalCyclone>> {
    let v = parse_value(json)?;
    let feats = v
        .get("features")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut tc: Option<TropicalCyclone> = None;
    let mut track = Vec::new();
    let mut swaths = Vec::new();
    for f in feats {
        let Some(p) = f.get("properties") else {
            continue;
        };
        let Some(g) = f.get("geometry") else { continue };
        let class = text(p, "Class");
        match g.get("type").and_then(Value::as_str) {
            Some("Point") if class.contains("Centroid") => {
                let Some((lon, lat)) = g.get("coordinates").and_then(pair) else {
                    continue;
                };
                let sev = p.get("severitydata");
                tc = Some(TropicalCyclone {
                    id: p.get("eventid").map(|v| v.to_string()).unwrap_or_default(),
                    name: text(p, "eventname"),
                    alert: text(p, "alertlevel"),
                    category: sev
                        .and_then(|s| s.get("severitytext"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    max_wind_kmh: sev
                        .and_then(|s| s.get("severity"))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    lon,
                    lat,
                    country: text(p, "country"),
                    source: text(p, "source"),
                    updated: text(p, "datemodified"),
                    track: Vec::new(),
                    swaths: Vec::new(),
                    report_url: p
                        .get("url")
                        .and_then(|u| u.get("report"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                });
            }
            Some("LineString") => {
                let mut pts: Vec<(f64, f64)> = g
                    .get("coordinates")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(pair).collect())
                    .unwrap_or_default();
                unwrap_lons(&mut pts);
                let forecast = p.get("forecast").and_then(Value::as_bool).unwrap_or(false);
                for w in pts.windows(2) {
                    track.push((w[0], w[1], text(p, "polygonlabel"), forecast));
                }
            }
            Some("Polygon") if class.starts_with("Poly_") => {
                let mut ring: Vec<(f64, f64)> = g
                    .get("coordinates")
                    .and_then(|c| c.get(0))
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(pair).collect())
                    .unwrap_or_default();
                unwrap_lons(&mut ring);
                if ring.len() >= 3 {
                    let level = class.trim_start_matches("Poly_").to_string();
                    swaths.push((text(p, "polygonlabel"), level, ring));
                }
            }
            _ => {}
        }
    }
    Ok(tc.map(|mut t| {
        // Keep the track and swaths on the same side of 180° as the centre.
        let shift = |lon: f64| -> f64 {
            let mut l = lon;
            while l - t.lon > 180.0 {
                l -= 360.0;
            }
            while t.lon - l > 180.0 {
                l += 360.0;
            }
            l - lon
        };
        for seg in &mut track {
            let d = shift(seg.0 .0);
            seg.0 .0 += d;
            seg.1 .0 += d;
        }
        for (_, _, ring) in &mut swaths {
            if let Some(&(lon0, _)) = ring.first() {
                let d = shift(lon0);
                for pt in ring.iter_mut() {
                    pt.0 += d;
                }
            }
        }
        t.track = track;
        t.swaths = swaths;
        t
    }))
}

impl Net {
    /// Every current tropical cyclone worldwide, with tracks and swaths.
    pub async fn tropical_cyclones(&self) -> Result<Vec<TropicalCyclone>> {
        let events = current_events(&self.get_text(EVENTS_URL, None).await?)?;
        let docs = futures::future::join_all(
            events
                .iter()
                .take(12)
                .map(|(_, _, url)| self.get_text(url, None)),
        )
        .await;
        let mut out = Vec::new();
        for doc in docs.into_iter().flatten() {
            if let Ok(Some(tc)) = parse_cyclone(&doc) {
                out.push(tc);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{"features":[
      {"properties":{"eventid":1001351,"episodeid":7,"iscurrent":"true","url":{"geometry":"https://g/1"}}},
      {"properties":{"eventid":1001348,"episodeid":24,"iscurrent":"false","url":{"geometry":"https://g/2"}}}]}"#;

    const GEO: &str = r#"{"features":[
      {"geometry":{"type":"Polygon","coordinates":[[[84,18],[86,18],[86,20],[84,20],[84,18]]]},"properties":{"Class":"Poly_Orange","polygonlabel":"90 km/h"}},
      {"geometry":{"type":"Point","coordinates":[85.0,19.4]},"properties":{"Class":"Point_Centroid","eventid":1001351,"eventname":"AKVAN-26","alertlevel":"Orange","country":"India","source":"JTWC","datemodified":"2026-11-18T11:05:21",
        "severitydata":{"severity":83.3,"severitytext":"Tropical Storm (maximum wind speed of 83 km/h)"},"url":{"report":"https://www.gdacs.org/report.aspx?eventid=1001351"}}},
      {"geometry":{"type":"LineString","coordinates":[[85.0,19.4],[84.9,21.2]]},"properties":{"Class":"Line_Line_0","polygonlabel":"TD","forecast":true}},
      {"geometry":{"type":"LineString","coordinates":[[86.7,11.3],[87.2,11.6],[87.3,12.0]]},"properties":{"Class":"Line_Line_4","polygonlabel":"TS","forecast":false}}]}"#;

    #[test]
    fn lists_only_current_events() {
        let e = current_events(LIST).unwrap();
        assert_eq!(
            e,
            vec![("1001351".into(), "7".into(), "https://g/1".into())]
        );
    }

    #[test]
    fn parses_cyclone_centre_track_and_swaths() {
        let tc = parse_cyclone(GEO).unwrap().unwrap();
        assert_eq!(tc.name, "AKVAN-26");
        assert_eq!(tc.alert, "Orange");
        assert_eq!((tc.lon, tc.lat), (85.0, 19.4));
        assert_eq!(tc.max_wind_kmh, 83.3);
        assert_eq!(tc.track.len(), 3, "a 3-point line is two segments");
        assert!(tc.track.iter().any(|t| t.3), "has forecast segments");
        assert!(tc.track.iter().any(|t| !t.3), "has past segments");
        assert_eq!(tc.swaths.len(), 1);
        assert_eq!(tc.swaths[0].0, "90 km/h");
        assert_eq!(tc.swaths[0].1, "Orange");
        assert_eq!(tc.color(), [0xff, 0x9f, 0x1c]);
    }

    #[test]
    fn paths_crossing_the_antimeridian_stay_continuous() {
        let mut ring = vec![
            (179.0, -15.0),
            (-179.5, -15.0),
            (-179.0, -16.0),
            (178.5, -16.0),
        ];
        unwrap_lons(&mut ring);
        assert_eq!(
            ring,
            vec![
                (179.0, -15.0),
                (180.5, -15.0),
                (181.0, -16.0),
                (178.5, -16.0)
            ]
        );
        let span = ring.iter().map(|p| p.0).fold(f64::MIN, f64::max)
            - ring.iter().map(|p| p.0).fold(f64::MAX, f64::min);
        assert!(span < 5.0);
    }

    #[test]
    fn no_centroid_means_no_cyclone() {
        assert!(parse_cyclone(r#"{"features":[]}"#).unwrap().is_none());
    }
}
