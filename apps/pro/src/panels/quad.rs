//! FOUR PANEL: four linked maps of one radar's scan, a different product
//! in each (reflectivity, velocity, CC and ZDR by default), all at the same
//! tilt. Panning or zooming one moves them all; the pointer is mirrored as
//! a cross in the others. Works for NEXRAD and European (OPERA) radars:
//! the selected single-site radar, or else the nearest one to the place.

use std::time::{Duration, Instant};

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, RichText, Stroke, Ui};
use ss_core::radar3d::L2Product;
use ss_core::PanelKind;
use ss_map::{GeoImageLayer, MapWidget, TileCache};

use super::l2map::L2Map;
use super::{placeholder, Panel, PanelCx};
use crate::theme::{c32, c32a};

/// Radars this close to the place are used when none is selected, km.
const NEAR_KM: f64 = 230.0;

pub struct QuadPanel {
    maps: Vec<MapWidget>,
    l2: Vec<L2Map>,
    products: [L2Product; 4],
    /// The radar the maps were last centred on.
    centred: Option<String>,
    dark: Option<bool>,
    /// Where the pointer is (lon, lat), to mirror in the other panes.
    hover: Option<(usize, (f64, f64))>,
}

impl QuadPanel {
    pub fn new(
        client: reqwest::Client,
        rt: tokio::runtime::Handle,
        tile_dir: Option<std::path::PathBuf>,
        ctx: egui::Context,
    ) -> Self {
        let maps = (0..4)
            .map(|_| {
                let cache =
                    TileCache::new(client.clone(), rt.clone(), tile_dir.clone(), ctx.clone());
                let mut m = MapWidget::new(cache);
                m.show_attribution = false;
                m.show_coords = false;
                m.show_stats = false;
                m.layers.push(ss_map::basemap(ss_map::Basemap::Map, true));
                m.layers.push(GeoImageLayer::new("l2"));
                m.layers
                    .push(ss_map::esri_labels(ss_map::Basemap::Map, true));
                m
            })
            .collect();
        Self {
            maps,
            l2: (0..4).map(|_| L2Map::default()).collect(),
            products: [
                L2Product::Reflectivity,
                L2Product::Velocity,
                L2Product::Cc,
                L2Product::Zdr,
            ],
            centred: None,
            dark: None,
            hover: None,
        }
    }

    /// The radar to show: the selected site, or the nearest to the place
    /// that isn't known to be offline (the nearest of all if every one is).
    fn site(cx: &PanelCx) -> Option<String> {
        let sh = &*cx.shared;
        if let Some(s) = &sh.config.radar.nexrad_site {
            return Some(s.to_ascii_uppercase());
        }
        let loc = cx.location.as_ref()?;
        let mut near: Vec<(f64, String)> = sh
            .radar_stations
            .iter()
            .map(|s| {
                (
                    ss_core::geo::haversine_km(loc.lat, loc.lon, s.lat, s.lon),
                    s.id.to_ascii_uppercase(),
                )
            })
            .filter(|(d, _)| *d <= NEAR_KM)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.iter()
            .find(|(_, id)| !sh.level2_errors.contains_key(id))
            .or(near.first())
            .map(|(_, id)| id.clone())
    }
}

/// A product's colour range for the legend, and how to read it.
pub fn legend_range(product: L2Product) -> (f32, f32, String) {
    let (lo, hi) = match product {
        L2Product::Reflectivity => (5.0, 75.0),
        L2Product::Cc => (0.5, 1.02),
        L2Product::Zdr => (-2.0, 6.0),
        _ => product.moment().range(),
    };
    let text = match product {
        L2Product::Reflectivity => format!("{lo:.0}…{hi:.0} dBZ"),
        L2Product::Velocity | L2Product::StormRelative => {
            format!("{lo:.0}…{hi:.0} m/s, green toward")
        }
        L2Product::Cc => format!("{lo:.2}…{hi:.2}"),
        L2Product::Zdr => format!("{lo:.0}…{hi:.0} dB"),
    };
    (lo, hi, text)
}

fn legend(painter: &egui::Painter, at: egui::Pos2, product: L2Product) {
    let (lo, hi, text) = legend_range(product);
    let (w, h) = (90.0, 8.0);
    let bg = Rect::from_min_size(at - vec2(4.0, 4.0), vec2(w + 8.0, h + 22.0));
    painter.rect_filled(bg, 3.0, Color32::from_black_alpha(170));
    for k in 0..30 {
        let v = lo + (hi - lo) * (k as f32 + 0.5) / 30.0;
        let c = product.color(v);
        let x0 = at.x + w * k as f32 / 30.0;
        painter.rect_filled(
            Rect::from_min_max(pos2(x0, at.y), pos2(x0 + w / 30.0 + 0.5, at.y + h)),
            0.0,
            Color32::from_rgb(c[0], c[1], c[2]),
        );
    }
    painter.text(
        at + vec2(0.0, h + 3.0),
        Align2::LEFT_TOP,
        text,
        FontId::monospace(9.5),
        Color32::from_white_alpha(210),
    );
}

impl Panel for QuadPanel {
    fn kind(&self) -> PanelKind {
        PanelKind::Quad
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut PanelCx) {
        let p = cx.palette;
        let dark = crate::theme::is_dark(&p);
        if self.dark != Some(dark) {
            self.dark = Some(dark);
            for m in &mut self.maps {
                m.layers.replace(
                    "basemap",
                    Box::new(ss_map::basemap(ss_map::Basemap::Map, dark)),
                );
                m.layers.replace(
                    "labels",
                    Box::new(ss_map::esri_labels(ss_map::Basemap::Map, dark)),
                );
            }
        }
        let Some(site) = Self::site(cx) else {
            placeholder(
                ui,
                &p,
                "No single-site radar here: choose one with :site, or a place within 230 km of a NEXRAD or European radar.",
            );
            return;
        };
        // Fetch the scan (shared with the radar pane's Level II view).
        {
            let sh = &mut *cx.shared;
            let due = sh
                .level2_requested
                .get(&site)
                .is_none_or(|t| t.elapsed() > Duration::from_secs(240));
            if due {
                sh.level2_requested.insert(site.clone(), Instant::now());
                cx.tasks.level2(&site);
            }
        }
        let scan = cx.shared.level2.get(&site).cloned();
        let tilt = cx.shared.config.radar.level2_tilt;

        // Header: radar, tilt, time.
        let station = cx
            .shared
            .radar_stations
            .iter()
            .find(|s| s.id.eq_ignore_ascii_case(&site))
            .map(|s| s.name.clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{site} · {station}"))
                    .monospace()
                    .color(c32(p.text)),
            );
            let n = scan.as_ref().map_or(1, |s| s.polar.sweeps.len().max(1));
            let shown = tilt.min(n - 1);
            if ui.small_button("◀").on_hover_text("Lower tilt").clicked() && shown > 0 {
                cx.shared.config.radar.level2_tilt = shown - 1;
                cx.shared.dirty_config = true;
            }
            let elev = self.l2.iter().find_map(|l| l.elevation);
            ui.label(
                RichText::new(match elev {
                    Some(e) => format!("{e:.1}°"),
                    None => "–".into(),
                })
                .monospace()
                .color(c32(p.a0)),
            );
            if ui.small_button("▶").on_hover_text("Higher tilt").clicked() && shown + 1 < n {
                cx.shared.config.radar.level2_tilt = shown + 1;
                cx.shared.dirty_config = true;
            }
            if let Some(s) = &scan {
                let time = chrono::DateTime::parse_from_rfc3339(&s.time)
                    .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
                    .unwrap_or_default();
                ui.label(RichText::new(time).monospace().color(c32a(p.text3)));
            } else {
                let text = match cx.shared.level2_errors.get(&site) {
                    Some(e) => format!("scan: {e}"),
                    None => "loading scan…".into(),
                };
                ui.label(RichText::new(text).monospace().color(c32a(p.text3)));
            }
            if self.l2.iter().any(L2Map::busy) {
                ui.spinner();
            }
            let credit = if ss_core::opera::is_opera(&site) {
                "EUMETNET OPERA (CC BY 4.0)"
            } else {
                "NOAA NEXRAD"
            };
            ui.label(RichText::new(credit).small().color(c32a(p.text3)))
                .on_hover_text("Maps: Esri, HERE, Garmin, © OpenStreetMap contributors");
        });

        // Centre on the radar whenever it changes.
        if self.centred.as_deref() != Some(site.as_str()) {
            if let Some(s) = &scan {
                for m in &mut self.maps {
                    m.viewport.set_center(s.lon, s.lat);
                    m.viewport.zoom = 7.0;
                }
                self.centred = Some(site.clone());
                for l in &mut self.l2 {
                    l.clear();
                }
            }
        }

        // Render each product.
        let motion = {
            let sh = &*cx.shared;
            let cells = if ss_core::opera::is_opera(&site) {
                &sh.mosaic_cells
            } else {
                &sh.storm_cells
            };
            super::radar3d::motion_for_site(cells, &site)
        };
        if let Some(s) = &scan {
            for i in 0..4 {
                if let Some(done) = self.l2[i].update(s, self.products[i], tilt, motion) {
                    if let Some(l) = self.maps[i].layers.get_mut::<GeoImageLayer>("l2") {
                        l.image = done.map(|img| {
                            let tex = ui.ctx().load_texture(
                                format!("quad-{i}"),
                                img.image,
                                egui::TextureOptions::LINEAR,
                            );
                            (tex, img.bounds)
                        });
                    }
                }
            }
            if self.l2.iter().any(L2Map::busy) {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }

        // The 2×2 grid.
        let area = ui.available_rect_before_wrap().intersect(ui.clip_rect());
        let gap = 3.0;
        let (w, h) = ((area.width() - gap) / 2.0, (area.height() - gap) / 2.0);
        let mut moved: Option<usize> = None;
        let mut hover = None;
        let loc = cx.location.clone();
        for i in 0..4 {
            let (col, row) = ((i % 2) as f32, (i / 2) as f32);
            let r = Rect::from_min_size(
                area.min + vec2(col * (w + gap), row * (h + gap)),
                vec2(w, h),
            );
            let resp = ui
                .scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                    ui.set_clip_rect(r);
                    self.maps[i].show(ui)
                })
                .inner;
            if resp.viewport_changed {
                moved = Some(i);
            }
            if let Some(h) = resp.hovered {
                hover = Some((i, h));
            }
            let painter = ui.painter_at(r);
            // The place.
            if let Some(l) = &loc {
                let at = self.maps[i].viewport.screen_from_lonlat(l.lon, l.lat, r);
                painter.circle_stroke(at, 5.0, Stroke::new(1.5, Color32::WHITE));
            }
            // The pointer, mirrored from another pane.
            if let Some((j, (lon, lat))) = self.hover {
                if j != i {
                    let at = self.maps[i].viewport.screen_from_lonlat(lon, lat, r);
                    let s = Stroke::new(1.2, Color32::WHITE);
                    painter.line_segment([at - vec2(8.0, 0.0), at + vec2(8.0, 0.0)], s);
                    painter.line_segment([at - vec2(0.0, 8.0), at + vec2(0.0, 8.0)], s);
                }
            }
            // Product menu (top-left) and legend (bottom-left).
            let menu_rect = Rect::from_min_size(r.min + vec2(6.0, 6.0), vec2(70.0, 20.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(menu_rect), |ui| {
                egui::ComboBox::from_id_salt(("quad-product", i))
                    .width(64.0)
                    .selected_text(RichText::new(self.products[i].short()).monospace().small())
                    .show_ui(ui, |ui| {
                        for prod in L2Product::ALL {
                            ui.selectable_value(&mut self.products[i], prod, prod.label());
                        }
                    });
            });
            legend(
                &painter,
                pos2(r.left() + 8.0, r.bottom() - 30.0),
                self.products[i],
            );
        }
        self.hover = hover;
        if let Some(i) = moved {
            let (lon, lat, z) = (
                self.maps[i].viewport.center_lon,
                self.maps[i].viewport.center_lat,
                self.maps[i].viewport.zoom,
            );
            for (j, m) in self.maps.iter_mut().enumerate() {
                if j != i {
                    m.viewport.set_center(lon, lat);
                    m.viewport.zoom = z;
                }
            }
            ui.ctx().request_repaint();
        }
        ui.allocate_rect(area, egui::Sense::hover());
    }
}
