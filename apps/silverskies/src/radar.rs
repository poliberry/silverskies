//! A simple radar: basemap, the LibreWXR composite (past frames plus the
//! short nowcast) as a looping animation, alert areas and a pin for the
//! active place. One colour scheme, no layers to manage.

use std::time::{Duration, Instant};

use egui::{CornerRadius, Margin, RichText, Ui};
use ss_core::alerts::colors::class_color;
use ss_core::theme::Palette;
use ss_core::{Alert, RadarFrame, WeatherMaps};
use ss_map::{
    carto_basemap, draw_color_legend, radar_dbz_stops, LayerStack, MapWidget, Marker, MarkerKind,
    MarkerLayer, PolyItem, PolygonLayer, RasterTileLayer, TileCache,
};
use ss_net::librewxr::radar_tile_url;

use crate::theme::{alpha, c32, c32a, RADIUS_SMALL};

/// "Universal Blue" — the same calm default the Pro app starts with.
const COLOR_SCHEME: u8 = 2;
const FRAME_STEP: Duration = Duration::from_millis(550);
/// Hold on the newest observed frame before looping.
const LIVE_HOLD: Duration = Duration::from_millis(1600);

pub struct Radar {
    map: MapWidget,
    frame: usize,
    playing: bool,
    last_step: Instant,
    /// The place the map was last centred on (re-centre when it changes).
    centred_on: Option<(f64, f64)>,
    /// The place the pin marker was last placed for.
    pinned: Option<(f64, f64)>,
    dark: Option<bool>,
    alerts_key: Option<String>,
}

/// Basemap, radar frames, alert areas, then the place pin on top.
fn layer_stack(dark: bool) -> LayerStack {
    let mut layers = LayerStack::new();
    layers.push(carto_basemap(dark));
    layers.push(RasterTileLayer::new("radar", "", 12, |_| String::new()).with_opacity(0.8));
    layers.push(PolygonLayer::new("alerts"));
    layers.push(MarkerLayer::new("pin"));
    layers
}

/// All frames in time order: observed, then nowcast.
pub fn frames(maps: &WeatherMaps) -> Vec<&RadarFrame> {
    maps.radar_past
        .iter()
        .chain(maps.radar_nowcast.iter())
        .collect()
}

impl Radar {
    pub fn new(cache: TileCache) -> Radar {
        let mut map = MapWidget::new(cache);
        map.viewport.zoom = 7.0;
        map.show_coords = false;
        Radar {
            map,
            frame: usize::MAX,
            playing: true,
            last_step: Instant::now(),
            centred_on: None,
            pinned: None,
            dark: None,
            alerts_key: None,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        maps: Option<&WeatherMaps>,
        place: Option<(f64, f64, &str)>,
        alerts: &[Alert],
        tz: Option<&str>,
    ) {
        let dark = crate::theme::is_dark(p);
        if self.dark != Some(dark) {
            // Theme changed (or first frame): rebuild with the matching basemap.
            self.dark = Some(dark);
            self.map.dark = dark;
            self.map.layers = layer_stack(dark);
            self.alerts_key = None;
            self.pinned = None;
            self.frame = usize::MAX;
        }

        if let Some((lat, lon, label)) = place {
            if self.centred_on != Some((lat, lon)) {
                if self.centred_on.is_none() {
                    self.map.viewport.set_center(lon, lat);
                } else {
                    self.map
                        .viewport
                        .fly_to(lon, lat, self.map.viewport.zoom.max(6.0));
                }
                self.centred_on = Some((lat, lon));
            }
            if self.pinned != Some((lat, lon)) {
                self.pinned = Some((lat, lon));
                if let Some(pins) = self.map.layers.get_mut::<MarkerLayer>("pin") {
                    pins.markers = vec![Marker {
                        id: "here".into(),
                        lon,
                        lat,
                        kind: MarkerKind::Pin,
                        color: c32(p.a0),
                        label: Some(label.to_string()),
                    }];
                }
            }
        }

        let key: String = alerts
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>()
            .join("|");
        if self.alerts_key.as_deref() != Some(&key) {
            self.alerts_key = Some(key);
            if let Some(layer) = self.map.layers.get_mut::<PolygonLayer>("alerts") {
                layer.set_items(
                    alerts
                        .iter()
                        .filter_map(|a| {
                            let g = a.geometry.clone()?;
                            let c = class_color(&a.class);
                            Some(PolyItem {
                                id: a.id.clone(),
                                geometry: g,
                                fill: alpha(c, 40),
                                stroke: c32(c),
                                stroke_width: 2.0,
                                pulse: false,
                                label: None,
                            })
                        })
                        .collect(),
                );
            }
        }

        // --- frame animation ---
        let all = maps.map(frames).unwrap_or_default();
        let live_idx = maps
            .map(|m| m.radar_past.len().saturating_sub(1))
            .unwrap_or(0);
        if !all.is_empty() {
            if self.frame >= all.len() {
                self.frame = live_idx;
            }
            if self.playing {
                let hold = if self.frame == live_idx {
                    LIVE_HOLD
                } else {
                    FRAME_STEP
                };
                if self.last_step.elapsed() >= hold {
                    self.frame = (self.frame + 1) % all.len();
                    self.last_step = Instant::now();
                }
                ui.ctx().request_repaint_after(FRAME_STEP);
            }
            if let (Some(m), Some(f)) = (maps, all.get(self.frame)) {
                let host = m.host.clone();
                let path = f.path.clone();
                let key = format!("radar:{path}:{COLOR_SCHEME}");
                if let Some(layer) = self.map.layers.get_mut::<RasterTileLayer>("radar") {
                    if layer.key != key {
                        layer.set_template(key, move |t| {
                            radar_tile_url(
                                &host,
                                &path,
                                u32::from(t.z),
                                t.x,
                                t.y,
                                COLOR_SCHEME,
                                true,
                                true,
                                None,
                                None,
                            )
                        });
                    }
                }
            }
        }

        let resp = self.map.show(ui);
        let rect = resp.rect;

        // --- floating controls ---
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 16.0, rect.bottom() - 64.0),
            egui::vec2((rect.width() - 32.0).min(520.0), 48.0),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        egui::Frame::new()
            .fill(c32(p.surface).gamma_multiply(0.94))
            .corner_radius(CornerRadius::same(RADIUS_SMALL + 4))
            .inner_margin(Margin::symmetric(10, 6))
            .show(&mut child, |ui| {
                ui.set_width(bar.width() - 20.0);
                ui.horizontal_centered(|ui| {
                    let label = if self.playing {
                        "❚❚ Pause"
                    } else {
                        "▶ Play"
                    };
                    if ui.button(label).clicked() {
                        self.playing = !self.playing;
                        if !self.playing {
                            self.frame = live_idx;
                        }
                    }
                    if !all.is_empty() {
                        let mut f = self.frame.min(all.len() - 1);
                        let slider = egui::Slider::new(&mut f, 0..=all.len() - 1).show_value(false);
                        if ui.add_sized([150.0, 20.0], slider).changed() {
                            self.frame = f;
                            self.playing = false;
                        }
                        let t = all[f].time;
                        // Frame time in the place's own zone (falls back to this computer's).
                        let when = chrono::DateTime::from_timestamp(t, 0)
                            .map(|d| match tz.and_then(|z| z.parse::<chrono_tz::Tz>().ok()) {
                                Some(z) => d.with_timezone(&z).format("%H:%M").to_string(),
                                None => d.with_timezone(&chrono::Local).format("%H:%M").to_string(),
                            })
                            .unwrap_or_default();
                        let tag = if f > live_idx {
                            "forecast"
                        } else if f == live_idx {
                            "now"
                        } else {
                            "past"
                        };
                        ui.label(RichText::new(format!("{when} · {tag}")).color(c32a(p.text2)));
                    } else {
                        ui.label(RichText::new("Loading radar…").color(c32a(p.text3)));
                    }
                });
            });

        // Rain-intensity legend, top-right.
        let legend = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 236.0, rect.top() + 14.0),
            egui::vec2(220.0, 56.0),
        );
        let mut lg = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(legend)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        egui::Frame::new()
            .fill(c32(p.surface).gamma_multiply(0.94))
            .corner_radius(CornerRadius::same(RADIUS_SMALL))
            .inner_margin(Margin::symmetric(10, 6))
            .show(&mut lg, |ui| {
                ui.set_width(legend.width() - 20.0);
                draw_color_legend(ui, "Rain intensity (dBZ)", &radar_dbz_stops(COLOR_SCHEME));
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_past_then_nowcast() {
        let f = |t: i64| RadarFrame {
            time: t,
            path: format!("/v2/radar/{t}"),
        };
        let maps = WeatherMaps {
            host: "https://example".into(),
            generated: 0,
            radar_past: vec![f(1), f(2)],
            radar_nowcast: vec![f(3)],
            satellite_infrared: vec![],
            color_schemes: vec![],
        };
        let times: Vec<i64> = frames(&maps).iter().map(|f| f.time).collect();
        assert_eq!(times, vec![1, 2, 3]);
    }
}
