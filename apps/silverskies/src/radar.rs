//! A simple radar: basemap, the LibreWXR composite (past frames plus the
//! short nowcast) as a looping animation, alert areas and a pin for the
//! active place. One colour scheme, no layers to manage.

use std::sync::Arc;

use egui::{CornerRadius, Margin, Ui};
use ss_core::alerts::colors::class_color;
use ss_core::theme::Palette;
use ss_core::{Alert, RadarFrame, WeatherMaps};
use ss_map::{
    basemap, draw_color_legend, esri_labels, esri_roads, radar_dbz_stops, Basemap, LayerStack,
    LoopClock, LoopFrameSource, MapWidget, Marker, MarkerKind, MarkerLayer, PolyItem, PolygonLayer,
    RadarLoopLayer, TileCache,
};
use ss_net::librewxr::radar_tile_url;

use crate::theme::{alpha, c32, RADIUS_SMALL};

/// "Universal Blue" — the same calm default the Pro app starts with.
const COLOR_SCHEME: u8 = 2;

pub struct Radar {
    map: MapWidget,
    /// Playback: holds each frame, eases into the next, waits for loading
    /// frames, and holds the live frame longer.
    clock: LoopClock,
    /// The newest observed frame's time, to follow it as new frames arrive.
    live_time: Option<i64>,
    /// The place the map was last centred on (re-centre when it changes).
    centred_on: Option<(f64, f64)>,
    /// The place the pin marker was last placed for.
    pinned: Option<(f64, f64)>,
    /// (dark, imagery) the layers were built for.
    built: Option<(bool, bool)>,
    alerts_key: Option<String>,
    /// Satellite imagery instead of the grey map (the Map / Satellite switch).
    pub imagery: bool,
}

/// Basemap (grey map or satellite imagery), radar frames, labels (and
/// roads on imagery), alert areas, then the place pin on top.
fn layer_stack(dark: bool, imagery: bool) -> LayerStack {
    let style = if imagery {
        Basemap::Satellite
    } else {
        Basemap::Map
    };
    let mut layers = LayerStack::new();
    layers.push(basemap(style, dark));
    layers.push(RadarLoopLayer::new("radar", 12, 0.8));
    if imagery {
        layers.push(esri_roads());
    }
    layers.push(esri_labels(style, dark));
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
            clock: LoopClock::default(),
            live_time: None,
            centred_on: None,
            pinned: None,
            built: None,
            alerts_key: None,
            imagery: false,
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
    ) -> bool {
        let mut toggled = false;
        let dark = crate::theme::is_dark(p);
        if self.built != Some((dark, self.imagery)) {
            // Theme or map style changed (or first frame): rebuild the layers.
            self.built = Some((dark, self.imagery));
            self.map.dark = dark;
            self.map.layers = layer_stack(dark, self.imagery);
            self.map.attribution = ss_map::attribution_for(if self.imagery {
                Basemap::Satellite
            } else {
                Basemap::Map
            })
            .to_string();
            self.alerts_key = None;
            self.pinned = None;
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
        // Follow the live edge: when a newer observed frame arrives, go to it
        // (the loop layer crossfades rather than cuts).
        let newest = maps.and_then(|m| m.radar_past.last()).map(|f| f.time);
        if newest != self.live_time {
            if self.live_time.is_none() || !self.clock.playing {
                self.clock.set(live_idx);
            }
            self.live_time = newest;
        }
        if let (Some(m), Some(layer)) = (maps, self.map.layers.get_mut::<RadarLoopLayer>("radar")) {
            let host = m.host.clone();
            layer.set_frames(
                all.iter()
                    .map(|f| {
                        let (host, path) = (host.clone(), f.path.clone());
                        LoopFrameSource {
                            key: format!("radar:{}:{COLOR_SCHEME}", f.path),
                            url: Arc::new(move |t| {
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
                            }),
                        }
                    })
                    .collect(),
            );
            let n = layer.len();
            let next = if n > 0 { (self.clock.idx + 1) % n } else { 0 };
            let dt = ui.input(|i| i.stable_dt).min(0.1);
            layer.pos = self.clock.tick(dt, n, live_idx, layer.ready(next));
            if self.clock.playing && n > 1 {
                ui.ctx().request_repaint();
            }
        }

        let resp = self.map.show(ui);
        let rect = resp.rect;

        // --- Map / Satellite switch (top left, a Material segmented button) ---
        if map_style_switch(ui, rect, p, &mut self.imagery) {
            toggled = true;
        }

        // --- floating controls ---
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 16.0, rect.bottom() - 64.0),
            egui::vec2((rect.width() - 32.0).min(480.0), 48.0),
        );
        // A floating pill: play/pause, scrubber and time, each placed at a
        // fixed position on the bar's centre line so they align exactly.
        let sch = crate::theme::scheme(p);
        ui.painter().rect_filled(
            bar.translate(egui::vec2(0.0, 2.0)),
            CornerRadius::same(255),
            egui::Color32::from_black_alpha(if sch.dark { 80 } else { 28 }),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(255), sch.surface_container_high);
        let cy = bar.center().y;
        let btn = egui::Rect::from_min_size(
            egui::pos2(bar.left() + 6.0, cy - 18.0),
            egui::vec2(100.0, 36.0),
        );
        let label = if self.clock.playing {
            "❚❚  Pause"
        } else {
            "▶  Play"
        };
        // Drawn directly so its size is fixed (a Button would wrap the label).
        let play = ui.interact(btn, ui.scope_id().with("radar_play"), egui::Sense::click());
        let fill = if play.hovered() {
            sch.primary.lerp_to_gamma(sch.on_primary, 0.08)
        } else {
            sch.primary
        };
        ui.painter().rect_filled(btn, CornerRadius::same(255), fill);
        crate::views::text_on_baseline(
            ui.painter(),
            btn.center().x,
            crate::views::baseline_for_center(cy, 14.0),
            egui::Align::Center,
            label,
            crate::theme::medium_font(14.0),
            sch.on_primary,
        );
        let play = play.on_hover_cursor(egui::CursorIcon::PointingHand);
        if play.clicked() {
            self.clock.playing = !self.clock.playing;
            if !self.clock.playing {
                self.clock.set(live_idx);
            }
        }
        if !all.is_empty() {
            let mut f = self.clock.idx.min(all.len() - 1);
            let slider_r = egui::Rect::from_min_size(
                egui::pos2(btn.right() + 14.0, cy - 12.0),
                egui::vec2(170.0, 24.0),
            );
            ui.spacing_mut().slider_width = slider_r.width();
            let slider = egui::Slider::new(&mut f, 0..=all.len() - 1).show_value(false);
            let sr = ui.put(slider_r, slider);
            let text_x = sr.rect.right() + 14.0;
            if sr.changed() {
                self.clock.set(f);
                self.clock.playing = false;
            }
            // Loading progress for the loop, as a thin line under the track.
            if let Some(layer) = self.map.layers.get_mut::<RadarLoopLayer>("radar") {
                let n = layer.len().max(1);
                let loaded = (0..n).map(|i| layer.ready(i)).sum::<f32>() / n as f32;
                if loaded < 0.99 {
                    let track = slider_r.shrink2(egui::vec2(6.0, 0.0));
                    let y = slider_r.bottom() - 1.0;
                    ui.painter().line_segment(
                        [
                            egui::pos2(track.left(), y),
                            egui::pos2(track.left() + track.width() * loaded, y),
                        ],
                        egui::Stroke::new(2.0, sch.tertiary),
                    );
                }
            }
            let t = all[f].time;
            // Frame time in the place's own zone (falls back to this computer's).
            let when = chrono::DateTime::from_timestamp(t, 0)
                .map(|d| match tz.and_then(|z| z.parse::<chrono_tz::Tz>().ok()) {
                    Some(z) => d.with_timezone(&z).format("%H:%M").to_string(),
                    None => d.with_timezone(&chrono::Local).format("%H:%M").to_string(),
                })
                .unwrap_or_default();
            // How fresh the live frame is.
            let age_min = all
                .get(live_idx)
                .map(|lf| (chrono::Utc::now().timestamp() - lf.time).max(0) / 60);
            let tag = if f > live_idx {
                "forecast".to_string()
            } else if f == live_idx {
                match age_min {
                    Some(m) if m >= 1 => format!("live, {m} min ago"),
                    _ => "live".to_string(),
                }
            } else {
                "past".to_string()
            };
            crate::views::text_on_baseline(
                ui.painter(),
                text_x,
                crate::views::baseline_for_center(cy, 14.5),
                egui::Align::Min,
                format!("{when} · {tag}"),
                egui::FontId::proportional(14.5),
                sch.on_surface_variant,
            );
        } else {
            crate::views::text_on_baseline(
                ui.painter(),
                btn.right() + 14.0,
                crate::views::baseline_for_center(cy, 14.5),
                egui::Align::Min,
                "Loading radar…",
                egui::FontId::proportional(14.5),
                sch.on_surface_variant,
            );
        }

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
        toggled
    }
}

/// A two-segment "Map | Satellite" switch in the map's top-left corner.
/// Returns true when it was flipped.
fn map_style_switch(ui: &mut Ui, map: egui::Rect, p: &Palette, imagery: &mut bool) -> bool {
    let s = crate::theme::scheme(p);
    let seg = egui::vec2(96.0, 36.0);
    let outer = egui::Rect::from_min_size(
        map.left_top() + egui::vec2(16.0, 14.0),
        egui::vec2(seg.x * 2.0, seg.y),
    );
    ui.painter().rect_filled(
        outer.translate(egui::vec2(0.0, 2.0)),
        CornerRadius::same(255),
        egui::Color32::from_black_alpha(if s.dark { 80 } else { 28 }),
    );
    ui.painter()
        .rect_filled(outer, CornerRadius::same(255), s.surface_container_high);
    let mut flipped = false;
    for (i, (label, sat)) in [("Map", false), ("Satellite", true)]
        .into_iter()
        .enumerate()
    {
        let r =
            egui::Rect::from_min_size(outer.left_top() + egui::vec2(seg.x * i as f32, 0.0), seg)
                .shrink(3.0);
        let id = ui.scope_id().with(("map_style", i));
        let resp = ui
            .interact(r, id, egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let on = *imagery == sat;
        if on {
            ui.painter()
                .rect_filled(r, CornerRadius::same(255), s.secondary_container);
        } else if resp.hovered() {
            ui.painter().rect_filled(
                r,
                CornerRadius::same(255),
                s.on_surface.gamma_multiply(0.08),
            );
        }
        crate::views::text_on_baseline(
            ui.painter(),
            r.center().x,
            crate::views::baseline_for_center(r.center().y, 14.0),
            egui::Align::Center,
            label,
            crate::theme::medium_font(14.0),
            if on {
                s.on_secondary_container
            } else {
                s.on_surface_variant
            },
        );
        if resp.clicked() && !on {
            *imagery = sat;
            flipped = true;
        }
    }
    flipped
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
