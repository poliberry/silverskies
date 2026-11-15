//! Background network work. Each call spawns a task on the tokio runtime
//! that posts an [`Event`] back to the UI thread and requests a repaint.

use crossbeam_channel::Sender;
use ss_core::alerts::classify::{classify, display_event};
use ss_core::alerts::merge::dedupe_and_sort;
use ss_core::geo::geometry_contains;
use ss_core::{Alert, BBox, Location, Weather, WeatherMaps};
use ss_net::Net;
use tokio::runtime::Handle;

use crate::ai::{self, AiTask};

pub enum Event {
    Weather {
        location_id: String,
        result: Result<Weather, String>,
    },
    Alerts {
        location_id: String,
        result: Result<Vec<Alert>, String>,
    },
    Geocoded {
        query: String,
        results: Vec<Location>,
    },
    IpLocated(Option<Location>),
    ReverseGeocoded {
        location_id: String,
        label: String,
    },
    Maps(Result<WeatherMaps, String>),
    Ai {
        /// Which request this answers (see `App::ai_seq`).
        seq: u64,
        result: Result<String, String>,
    },
}

#[derive(Clone)]
pub struct Worker {
    rt: Handle,
    net: Net,
    /// Separate client for the AI: longer timeout than the weather APIs.
    ai_client: reqwest::Client,
    tx: Sender<Event>,
    ctx: egui::Context,
}

impl Worker {
    pub fn new(rt: Handle, net: Net, tx: Sender<Event>, ctx: egui::Context) -> Worker {
        let ai_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_default();
        Worker {
            rt,
            net,
            ai_client,
            tx,
            ctx,
        }
    }

    fn spawn<F>(&self, f: impl FnOnce(Net, Sender<Event>) -> F)
    where
        F: std::future::Future<Output = Event> + Send + 'static,
    {
        let fut = f(self.net.clone(), self.tx.clone());
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        self.rt.spawn(async move {
            let ev = fut.await;
            let _ = tx.send(ev);
            ctx.request_repaint();
        });
    }

    pub fn forecast(&self, location_id: String, lat: f64, lon: f64) {
        self.spawn(move |net, _| async move {
            let result = net.forecast(lat, lon).await.map_err(|e| e.to_string());
            Event::Weather {
                location_id,
                result,
            }
        });
    }

    /// Official alerts for a point from the WMO / CAP feeds LibreWXR
    /// aggregates worldwide.
    pub fn alerts(&self, location_id: String, lat: f64, lon: f64) {
        self.spawn(move |net, _| async move {
            let result = net
                .librewxr_alerts_bbox(BBox::around(lat, lon, 0.5))
                .await
                .map(|all| alerts_for_point(all, lat, lon))
                .map_err(|e| e.to_string());
            Event::Alerts {
                location_id,
                result,
            }
        });
    }

    pub fn geocode(&self, query: String) {
        self.spawn(move |net, _| async move {
            let results = net.geocode(&query).await;
            Event::Geocoded { query, results }
        });
    }

    pub fn ip_locate(&self) {
        self.spawn(move |net, _| async move { Event::IpLocated(net.ip_geolocate().await) });
    }

    pub fn reverse_geocode(&self, location_id: String, lat: f64, lon: f64) {
        self.spawn(move |net, _| async move {
            let label = net
                .reverse_geocode(lat, lon)
                .await
                .unwrap_or_else(|| "My location".to_string());
            Event::ReverseGeocoded { location_id, label }
        });
    }

    pub fn weather_maps(&self) {
        self.spawn(move |net, _| async move {
            Event::Maps(net.weather_maps().await.map_err(|e| e.to_string()))
        });
    }

    pub fn ai(&self, seq: u64, key: String, model: String, task: AiTask, digest: String) {
        let client = self.ai_client.clone();
        self.spawn(move |_, _| async move {
            let result = ai::run(&client, &key, &model, &task, &digest).await;
            Event::Ai { seq, result }
        });
    }
}

/// Keep alerts whose area covers the point (or that carry no geometry, so
/// can't be ruled out), classified, de-duplicated and most severe first.
pub fn alerts_for_point(all: Vec<Alert>, lat: f64, lon: f64) -> Vec<Alert> {
    let mut v: Vec<Alert> = all
        .into_iter()
        .filter(|a| {
            a.geometry
                .as_ref()
                .is_none_or(|g| geometry_contains(g, lat, lon))
        })
        .map(|mut a| {
            a.class = classify(
                &a.event,
                a.headline.as_deref(),
                a.description.as_deref(),
                a.severity,
            );
            a.display_event = display_event(&a.event, &a.class);
            a
        })
        .collect();
    v = dedupe_and_sort(v);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss_core::{AlertSource, Geometry, Severity};

    fn alert(id: &str, event: &str, geometry: Option<Geometry>) -> Alert {
        Alert {
            id: id.into(),
            source: AlertSource::LibreWxr,
            event: event.into(),
            display_event: event.into(),
            class: String::new(),
            headline: None,
            description: None,
            instruction: None,
            severity: Severity::Moderate,
            area_desc: Some(id.into()),
            onset: None,
            sent: None,
            expires: None,
            issuing_office: None,
            url: None,
            geometry,
            affected_zones: vec![],
            vtec: None,
        }
    }

    #[test]
    fn keeps_only_alerts_covering_the_point() {
        let square = |x0: f64, y0: f64| {
            Geometry::Polygon(vec![vec![
                [x0, y0],
                [x0 + 1.0, y0],
                [x0 + 1.0, y0 + 1.0],
                [x0, y0 + 1.0],
                [x0, y0],
            ]])
        };
        let all = vec![
            alert("in", "Wind Warning", Some(square(10.0, 50.0))),
            alert("out", "Frost Warning", Some(square(20.0, 50.0))),
            alert("nogeom", "Heat Advisory", None),
        ];
        let kept = alerts_for_point(all, 50.5, 10.5);
        let ids: Vec<&str> = kept.iter().map(|a| a.id.as_str()).collect();
        assert!(ids.contains(&"in") && ids.contains(&"nogeom"));
        assert!(!ids.contains(&"out"));
        assert!(kept.iter().all(|a| !a.class.is_empty()));
    }
}
