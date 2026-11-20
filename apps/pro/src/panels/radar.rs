use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{Local, Utc};
use egui::{vec2, Align, Align2, Color32, FontId, Layout, RichText, Ui};
use ss_core::alerts::classify::{storm_tier, StormTier};
use ss_core::alerts::colors::effective_color;
use ss_core::command::{Command, RadarCmd};
use ss_core::theme::{brightness, parse_color};
use ss_core::{
    BBox, Geometry, PanelKind, RadarProduct, RadarSettings, SpcOutlookFeature, TimeFormat,
    WeatherMaps,
};
use ss_map::viewport::world_px_at;
use ss_map::{
    draw_color_legend, esri_basemap, esri_hillshade, radar_dbz_stops, Barb, LayerStack,
    LoopFrameSource, MapWidget, Marker, MarkerKind, MarkerLayer, PolyItem, PolygonLayer,
    RadarLoopLayer, RangeRingLayer, RasterTileLayer, StormLayer, StormMark, StormShape, TileCache,
    TileId, WindBarbLayer, WmsImageLayer,
};
use ss_net::librewxr::{radar_tile_url, satellite_tile_url};
use ss_net::ncep;
use ss_net::nws::is_live;
use ss_net::owm::{self, OwmLayer};

use super::{tracks, Panel, PanelCx};
use crate::state::LogLevel;
use crate::theme::{c32, c32a};
use std::sync::Arc;

/// Max native zoom of the LibreWXR radar / satellite tiles.
const TILE_MAX_ZOOM: u8 = 12;
const OWM_MAX_ZOOM: u8 = 10;
const MIN_SITE_ZOOM: f64 = 5.0;
const BARB_MIN_ZOOM: f64 = 6.0;
/// Below this zoom the viewport spans too much area to auto-load alerts.
const VIEWPORT_ALERTS_MIN_ZOOM: f64 = 4.0;
const FRAME_SECS: f64 = 0.8;
const BAR_H: f32 = 26.0;
const LAYERS_H: f32 = 26.0;
const LEGEND_H: f32 = 36.0;
const LAYERS_GAP: f32 = 4.0;
const LIGHTNING_NEAR_KM: f64 = 100.0;
const LIGHTNING_NEAR_S: i64 = 15 * 60;

/// One frame of the composite radar loop.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopFrame {
    pub time: i64,
    pub path: String,
    pub nowcast: bool,
}

/// Past frames followed by (optionally) nowcast frames.
pub fn loop_frames(maps: &WeatherMaps, include_nowcast: bool) -> Vec<LoopFrame> {
    let mut out: Vec<LoopFrame> = maps
        .radar_past
        .iter()
        .map(|f| LoopFrame {
            time: f.time,
            path: f.path.clone(),
            nowcast: false,
        })
        .collect();
    if include_nowcast {
        out.extend(maps.radar_nowcast.iter().map(|f| LoopFrame {
            time: f.time,
            path: f.path.clone(),
            nowcast: true,
        }));
    }
    out
}

/// Index of the last observed (non-nowcast) frame, 0 if there is none.
pub fn latest_observed_index(frames: &[LoopFrame]) -> usize {
    frames.iter().rposition(|f| !f.nowcast).unwrap_or(0)
}

/// Loop playback state (egui-free).
pub struct Playback {
    pub idx: usize,
    pub playing: bool,
    pub tracking_live: bool,
    /// Time of the previous tick (NaN before the first one while playing).
    last_advance: f64,
    prev_latest_time: Option<i64>,
    /// Shared loop timing: hold, ease into the next frame, wait for loading.
    clock: ss_map::LoopClock,
    /// The fluid playhead for the radar loop layer (`idx` plus the crossfade).
    pub pos: f32,
}

impl Default for Playback {
    fn default() -> Self {
        Self::new()
    }
}

impl Playback {
    pub fn new() -> Self {
        Self {
            idx: 0,
            playing: false,
            tracking_live: true,
            last_advance: f64::NAN,
            prev_latest_time: None,
            clock: ss_map::LoopClock::with_timing(FRAME_SECS as f32, 0.3, FRAME_SECS as f32, 2.0),
            pos: 0.0,
        }
    }

    /// Call every frame with the current loop and time in seconds.
    #[cfg(test)]
    pub fn tick(&mut self, frames: &[LoopFrame], now: f64) {
        self.tick_with(frames, now, 1.0);
    }

    /// Like [`tick`](Self::tick), with how loaded the next frame is (0..=1),
    /// so playback waits for it instead of showing it half-drawn.
    pub fn tick_with(&mut self, frames: &[LoopFrame], now: f64, next_ready: f32) {
        if frames.is_empty() {
            return;
        }
        let latest = latest_observed_index(frames);
        let latest_time = frames.get(latest).filter(|f| !f.nowcast).map(|f| f.time);
        if self.tracking_live && latest_time != self.prev_latest_time {
            self.idx = latest;
        }
        self.prev_latest_time = latest_time;
        self.idx = self.idx.min(frames.len() - 1);
        if self.clock.idx != self.idx {
            self.clock.set(self.idx);
        }
        if self.playing {
            let dt = if self.last_advance.is_nan() {
                0.0
            } else {
                (now - self.last_advance).max(0.0)
            };
            self.last_advance = now;
            self.clock.playing = true;
            self.pos = self.clock.tick(dt as f32, frames.len(), latest, next_ready);
            self.idx = self.clock.idx;
        } else {
            self.clock.playing = false;
            self.pos = self.idx as f32;
        }
    }

    pub fn step(&mut self, frames: &[LoopFrame], delta: i32) {
        self.tracking_live = false;
        if frames.is_empty() {
            return;
        }
        let n = frames.len() as i64;
        self.idx = (self.idx as i64 + i64::from(delta)).rem_euclid(n) as usize;
    }

    pub fn set_playing(&mut self, on: bool) {
        self.playing = on;
        if on {
            self.tracking_live = false;
            self.last_advance = f64::NAN;
        }
    }

    pub fn go_live(&mut self, frames: &[LoopFrame]) {
        self.idx = latest_observed_index(frames);
        self.tracking_live = true;
        self.playing = false;
    }

    pub fn scrub(&mut self, frames: &[LoopFrame], idx: usize) {
        self.tracking_live = false;
        self.idx = idx.min(frames.len().saturating_sub(1));
    }

    pub fn is_live(&self, frames: &[LoopFrame]) -> bool {
        !frames.is_empty()
            && self.idx == latest_observed_index(frames)
            && frames.get(self.idx).is_some_and(|f| !f.nowcast)
    }
}

/// Arrow / cell overlay style: light strokes on a dark palette, dark on a light one.
fn overlay_style(on: bool, dark: bool) -> Option<&'static str> {
    on.then_some(if dark { "light" } else { "dark" })
}

/// Tile-cache key for the composite radar layer; changes with every URL-affecting input.
pub fn radar_layer_key(path: &str, rs: &RadarSettings, dark: bool) -> String {
    format!(
        "radar:{path}:{}:{}{}:{}:{}",
        rs.color_scheme,
        u8::from(rs.smooth),
        u8::from(rs.snow),
        overlay_style(rs.show_arrows, dark).unwrap_or("-"),
        overlay_style(rs.show_cells, dark).unwrap_or("-"),
    )
}

/// SPC Day-1 outlook polygons, lowest risk first so higher risk draws on top.
pub fn spc_items(features: &[SpcOutlookFeature]) -> Vec<PolyItem> {
    let mut sorted: Vec<&SpcOutlookFeature> = features.iter().collect();
    sorted.sort_by_key(|f| f.level);
    sorted
        .into_iter()
        .map(|f| {
            let (fill, stroke) = (parse_color(&f.fill), parse_color(&f.stroke));
            PolyItem {
                id: format!("spc:{}:{}", f.code, f.level),
                geometry: f.geometry.clone(),
                fill: match fill {
                    Some([r, g, b, _]) => Color32::from_rgba_unmultiplied(r, g, b, 48),
                    None => Color32::from_rgba_unmultiplied(128, 128, 128, 48),
                },
                stroke: match stroke {
                    Some([r, g, b, _]) => Color32::from_rgb(r, g, b),
                    None => Color32::GRAY,
                },
                stroke_width: 1.5,
                pulse: false,
                label: Some(format!("SPC Day 1 outlook: {}", f.name)),
            }
        })
        .collect()
}

/// Is a new METAR wind request due for the view `cur`? `last` is the previous
/// request's bbox and time; `settled` means the view has been still for a moment.
pub fn wind_request_due(last: Option<(&BBox, f64)>, now: f64, cur: &BBox, settled: bool) -> bool {
    bbox_request_due(last, now, cur, settled, 300.0)
}

/// Generalised form of `wind_request_due`: a request is also due once the last one is
/// `max_age_secs` old.
pub fn bbox_request_due(
    last: Option<(&BBox, f64)>,
    now: f64,
    cur: &BBox,
    settled: bool,
    max_age_secs: f64,
) -> bool {
    let Some((prev, t)) = last else {
        return true;
    };
    if now - t >= max_age_secs {
        return true;
    }
    if !settled {
        return false;
    }
    let (w, h) = (cur.east - cur.west, cur.north - cur.south);
    let dx = ((cur.west + cur.east) - (prev.west + prev.east)).abs() / 2.0;
    let dy = ((cur.south + cur.north) - (prev.south + prev.north)).abs() / 2.0;
    if dx > 0.3 * w || dy > 0.3 * h {
        return true;
    }
    // A zoom change of one level or more means the width halved or doubled.
    let ratio = w / (prev.east - prev.west).max(1e-9);
    ratio <= 0.5 + 1e-9 || ratio >= 2.0 - 1e-9
}

pub struct RadarPanel {
    map: MapWidget,
    playback: Playback,
    dark: bool,
    positioned: bool,
    alerts_sig: u64,
    locations_sig: u64,
    stations_sig: u64,
    spc_sig: u64,
    md_sig: u64,
    barbs_sig: u64,
    /// Signature of the storm data the storm layers were built from.
    storms_sig: u64,
    nexrad_cfg: Option<(String, RadarProduct)>,
    /// Level II products drawn here.
    l2: super::l2map::L2Map,
    /// The cross section under the drawn line, and its length (km).
    xs: super::l2map::XsWorker,
    xs_tex: Option<(egui::TextureHandle, f32)>,
    /// Which hail/rotation track each layer's texture was made from.
    track_keys: std::collections::HashMap<ss_net::mrms::TrackKind, Option<usize>>,
    last_wind_req: Option<(BBox, f64)>,
    last_alert_req: Option<(BBox, f64)>,
    view_sig: (u64, u64, u64),
    /// (imagery, dark) the basemap and labels were built for.
    basemap_sig: Option<(bool, bool)>,
    last_view_change: f64,
}

/// Zoom level that fits `bbox` inside a viewport of `size` pixels.
pub fn fit_zoom(bbox: &BBox, size: egui::Vec2) -> f64 {
    let (x0, y0) = world_px_at(bbox.west, bbox.north, 0.0);
    let (x1, y1) = world_px_at(bbox.east, bbox.south, 0.0);
    let dx = (x1 - x0).abs().max(1e-6);
    let dy = (y1 - y0).abs().max(1e-6);
    let zx = (f64::from(size.x.max(32.0)) * 0.7 / dx).log2();
    let zy = (f64::from(size.y.max(32.0)) * 0.7 / dy).log2();
    zx.min(zy).clamp(2.0, 12.0)
}

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

fn placeholder_raster(name: &str, max_zoom: u8, opacity: f32) -> RasterTileLayer {
    let mut l = RasterTileLayer::new(name, "", max_zoom, |_| String::new()).with_opacity(opacity);
    l.visible = false;
    l
}

fn nexrad_layer(site: &str, product: RadarProduct) -> WmsImageLayer {
    let site_lc = site.to_ascii_lowercase();
    WmsImageLayer::new(
        "nexrad",
        format!("nexrad:{site_lc}:{}", product.id()),
        120.0,
        move |bbox, w, h| ncep::wms_url(&site_lc, product, bbox, w, h),
    )
    .with_opacity(0.85)
}

/// Point a raster layer at a (possibly new) template; the URL closure is only
/// installed when the cache key changed.
fn sync_raster(
    layers: &mut LayerStack,
    name: &str,
    visible: bool,
    opacity: f32,
    key: Option<String>,
    f: impl Fn(TileId) -> String + Send + 'static,
) {
    if let Some(l) = layers.get_mut::<RasterTileLayer>(name) {
        l.visible = visible;
        l.opacity = opacity;
        if let (true, Some(key)) = (visible, key) {
            if l.key != key {
                l.set_template(key, f);
            }
        }
    }
}

impl RadarPanel {
    pub fn new(
        client: reqwest::Client,
        rt: tokio::runtime::Handle,
        tile_dir: Option<PathBuf>,
        ctx: egui::Context,
    ) -> Self {
        let cache = TileCache::new(client, rt, tile_dir, ctx);
        let mut map = MapWidget::new(cache);
        let l = &mut map.layers;
        l.push(esri_basemap(true));
        l.push(esri_hillshade(0.32));
        l.push(placeholder_raster("satellite", TILE_MAX_ZOOM, 0.75));
        l.push(RadarLoopLayer::new("radar", TILE_MAX_ZOOM, 0.7));
        // Roads (imagery only) and place labels sit above the radar.
        let mut roads = ss_map::esri_roads();
        roads.visible = false;
        l.push(roads);
        l.push(ss_map::esri_labels(ss_map::Basemap::Map, true));
        l.push(placeholder_raster("owm-wind", OWM_MAX_ZOOM, 0.55));
        l.push(placeholder_raster("owm-temp", OWM_MAX_ZOOM, 0.55));
        l.push(placeholder_raster("owm-precip", OWM_MAX_ZOOM, 0.55));
        let mut nx = nexrad_layer("kxxx", RadarProduct::SrBref);
        nx.visible = false;
        l.push(nx);
        let mut l2 = ss_map::GeoImageLayer::new("level2");
        l2.visible = false;
        l.push(l2);
        let mut ring =
            RangeRingLayer::new("range-ring", 0.0, 0.0, ncep::RANGE_RING_M, Color32::GRAY);
        ring.visible = false;
        l.push(ring);
        l.push(PolygonLayer::new("spc-outlook"));
        l.push(PolygonLayer::new("md"));
        l.push(PolygonLayer::new("alerts"));
        l.push(WindBarbLayer::new("barbs", Color32::GRAY));
        // MRMS hail swaths and rotation tracks (US).
        for name in ["swath-track", "hail-track", "rotation-track"] {
            let mut t = ss_map::GeoImageLayer::new(name);
            t.visible = false;
            l.push(t);
        }
        // Meteosat's lightning (Europe, Africa) as an image; GOES flashes
        // as sparks on top.
        let mut mtg =
            WmsImageLayer::new("lightning-mtg", "eumetsat:mtg_li_afa", 300.0, |b, w, h| {
                format!(
                "https://view.eumetsat.int/geoserver/wms?service=WMS&version=1.3.0&request=GetMap\
                 &layers=mtg_fd:li_afa&styles=&crs=EPSG:3857&bbox={},{},{},{}&width={w}&height={h}\
                 &format=image/png&transparent=true",
                b.0, b.1, b.2, b.3
            )
            })
            .with_opacity(0.9);
        mtg.visible = false;
        l.push(mtg);
        let mut ltg = ss_map::LightningLayer::new("lightning");
        ltg.visible = false;
        l.push(ltg);
        l.push(PolygonLayer::new("tc-swaths"));
        l.push(StormLayer::new("storm-reports"));
        l.push(StormLayer::new("storm-cells"));
        l.push(StormLayer::new("tropical"));
        l.push(MarkerLayer::new("locations"));
        l.push(MarkerLayer::new("radar-sites"));
        Self {
            map,
            playback: Playback::new(),
            dark: true,
            positioned: false,
            alerts_sig: 0,
            locations_sig: 0,
            stations_sig: 0,
            spc_sig: 0,
            md_sig: 0,
            barbs_sig: 0,
            storms_sig: 0,
            nexrad_cfg: None,
            l2: Default::default(),
            xs: Default::default(),
            xs_tex: None,
            track_keys: Default::default(),
            last_wind_req: None,
            last_alert_req: None,
            view_sig: (0, 0, 0),
            basemap_sig: None,
            last_view_change: 0.0,
        }
    }

    fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
        self.map.dark = dark;
    }

    /// Swap the basemap and labels when the style or the theme changes.
    fn sync_basemap(&mut self, imagery: bool) {
        let want = (imagery, self.dark);
        if self.basemap_sig == Some(want) {
            return;
        }
        self.basemap_sig = Some(want);
        let style = if imagery {
            ss_map::Basemap::Satellite
        } else {
            ss_map::Basemap::Map
        };
        self.map
            .layers
            .replace("basemap", Box::new(ss_map::basemap(style, self.dark)));
        self.map
            .layers
            .replace("labels", Box::new(ss_map::esri_labels(style, self.dark)));
        self.map.attribution = ss_map::attribution_for(style).to_string();
    }
}

fn overlay_text(
    painter: &egui::Painter,
    pos: egui::Pos2,
    align: Align2,
    text: &str,
    color: Color32,
) {
    let font = FontId::monospace(11.0);
    painter.text(
        pos + vec2(1.0, 1.0),
        align,
        text,
        font.clone(),
        Color32::from_black_alpha(200),
    );
    painter.text(pos, align, text, font, color);
}

/// Shadowed overlay text with its baseline at `y`.
fn overlay_text_on_baseline(
    painter: &egui::Painter,
    x: f32,
    y: f32,
    align: egui::Align,
    text: &str,
    color: Color32,
) {
    use ss_assets::text::text_on_baseline;
    let font = FontId::monospace(11.0);
    text_on_baseline(
        painter,
        x + 1.0,
        y + 1.0,
        align,
        text,
        font.clone(),
        Color32::from_black_alpha(200),
    );
    text_on_baseline(painter, x, y, align, text, font, color);
}

/// Map marks for NEXRAD storm cells: a threat symbol and, for moving cells,
/// a track projected 15/30/45/60 minutes ahead.
pub fn storm_cell_marks(cells: &[ss_core::storms::StormCell]) -> Vec<StormMark> {
    use ss_core::storms::Tvs;
    let mut marks: Vec<StormMark> = cells
        .iter()
        .map(|c| {
            let shape = if c.tvs != Tvs::None {
                StormShape::Tornado
            } else if c.meso > 0 {
                StormShape::Meso
            } else if c.max_hail_in >= 1.0 || c.posh >= 50.0 {
                StormShape::Hail
            } else if c.max_dbz >= 50.0 {
                StormShape::Strong
            } else {
                StormShape::Cell
            };
            let track = if c.speed_kt >= 3.0 {
                [15.0, 30.0, 45.0, 60.0]
                    .iter()
                    .map(|m| c.projected(*m))
                    .collect()
            } else {
                Vec::new()
            };
            let motion = if c.speed_kt >= 3.0 {
                format!(
                    "moving {} at {:.0} kt (track ticks every 15 min)",
                    ss_core::weather::units::compass_8(c.heading_deg()),
                    c.speed_kt
                )
            } else {
                "nearly stationary".to_string()
            };
            let tooltip = if c.source == ss_core::storms::CellSource::Mosaic {
                format!(
                    "Radar-mosaic cell {} — {}\n{motion}\nReflectivity and motion only (no TVS/meso/hail outside NEXRAD)",
                    c.id,
                    c.summary(),
                )
            } else {
                format!(
                    "{}/{} — {}\n{motion}\nVIL {:.0} kg/m² · max {:.0} dBZ at {:.0}k ft",
                    c.radar,
                    c.id,
                    c.summary(),
                    c.vil,
                    c.max_dbz,
                    c.max_dbz_height_kft,
                )
            };
            StormMark {
                id: format!("{}/{}", c.radar, c.id),
                lon: c.lon,
                lat: c.lat,
                color: c32(super::storms::threat_color(c.threat())),
                shape,
                track,
                segments: Vec::new(),
                tooltip,
            }
        })
        .collect();
    // Most threatening drawn last (on top).
    marks.sort_by_key(|m| match m.shape {
        StormShape::Cell => 0,
        StormShape::Strong => 1,
        StormShape::Hail => 2,
        StormShape::Meso => 3,
        _ => 4,
    });
    marks
}

/// Map marks for tropical cyclones: centre symbol, past track (solid) and
/// forecast track (dashed).
pub fn tropical_marks(tcs: &[ss_core::storms::TropicalCyclone]) -> Vec<StormMark> {
    tcs.iter()
        .map(|t| {
            let tooltip = format!(
                "{} — {} alert\n{}\n{} · source {} · updated {} UTC\nsolid: past track · dashed: forecast",
                t.name,
                t.alert,
                t.category,
                if t.country.is_empty() { "open ocean" } else { &t.country },
                t.source,
                t.updated.replace('T', " "),
            );
            StormMark {
                id: format!("tc{}", t.id),
                lon: t.lon,
                lat: t.lat,
                color: c32(t.color()),
                shape: StormShape::Cyclone,
                track: Vec::new(),
                segments: t.track.iter().map(|(a, b, _, fc)| (*a, *b, *fc)).collect(),
                tooltip,
            }
        })
        .collect()
}

/// Wind-swath polygons for tropical cyclones (GDACS 60/90/120 km/h areas).
pub fn tropical_swaths(tcs: &[ss_core::storms::TropicalCyclone]) -> Vec<PolyItem> {
    let mut items = Vec::new();
    for t in tcs {
        for (i, (label, level, ring)) in t.swaths.iter().enumerate() {
            let rgb = ss_core::storms::alert_color(level);
            items.push(PolyItem {
                id: format!("tc{}-{i}", t.id),
                geometry: ss_core::Geometry::Polygon(vec![ring
                    .iter()
                    .map(|(lo, la)| [*lo, *la])
                    .collect()]),
                fill: Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], 26),
                stroke: Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], 150),
                stroke_width: 1.0,
                pulse: false,
                label: Some(format!("{} — winds ≥ {label}", t.name)),
            });
        }
    }
    items
}

/// Map marks for local storm reports (newest drawn on top).
pub fn storm_report_marks(reports: &[ss_core::storms::StormReport]) -> Vec<StormMark> {
    let mut marks: Vec<StormMark> = reports
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let when = chrono::DateTime::parse_from_rfc3339(&r.valid)
                .map(|d| {
                    d.with_timezone(&chrono::Local)
                        .format("%a %H:%M")
                        .to_string()
                })
                .unwrap_or_default();
            let mut tip = format!(
                "{} — {}, {}\n{} · {}",
                r.headline(),
                r.place,
                r.state,
                when,
                r.source
            );
            if !r.remark.is_empty() {
                tip.push('\n');
                tip.push_str(&r.remark.chars().take(200).collect::<String>());
            }
            StormMark {
                id: format!("lsr{i}"),
                lon: r.lon,
                lat: r.lat,
                color: c32(r.kind.color()),
                shape: StormShape::Report(r.kind.letter()),
                track: Vec::new(),
                segments: Vec::new(),
                tooltip: tip,
            }
        })
        .collect();
    marks.reverse();
    marks
}

/// Topmost visible polygon under `pos` in the named layer: `(id, label)`.
fn hit_polygon(
    map: &mut MapWidget,
    name: &str,
    pos: egui::Pos2,
    rect: egui::Rect,
) -> Option<(String, Option<String>)> {
    let vp = &map.viewport;
    let layer = map.layers.get_mut::<PolygonLayer>(name)?;
    if !layer.visible {
        return None;
    }
    layer
        .hit_test(pos, vp, rect)
        .first()
        .map(|it| (it.id.clone(), it.label.clone()))
}

fn legend_stops(rs: &RadarSettings) -> Option<Vec<(Color32, &'static str)>> {
    let dbz = match (&rs.nexrad_site, rs.level2) {
        (None, _) => true,
        (Some(_), Some(p)) => p == ss_core::radar3d::L2Product::Reflectivity,
        (Some(_), None) => rs.nexrad_product == RadarProduct::SrBref,
    };
    (rs.legend && dbz).then(|| radar_dbz_stops(rs.color_scheme))
}

/// Color schemes for the layers row: the live index when loaded, else the
/// built-in list. Pure (no egui) so it can be unit tested.
pub fn scheme_options(maps: Option<&WeatherMaps>) -> Vec<(u8, String)> {
    match maps {
        Some(m) if !m.color_schemes.is_empty() => m
            .color_schemes
            .iter()
            .map(|s| (s.id, s.name.clone()))
            .collect(),
        _ => ss_net::librewxr::default_color_schemes()
            .into_iter()
            .map(|s| (s.id, s.name))
            .collect(),
    }
}

/// Layer toggle buttons for the floating box: `(label, active, command)`.
/// Pure (no egui) so it can be unit tested.
pub fn layer_toggles(rs: &RadarSettings) -> Vec<(&'static str, bool, Command)> {
    vec![
        (
            "IMG",
            rs.imagery,
            Command::Layer {
                name: "imagery".to_string(),
                on: Some(!rs.imagery),
            },
        ),
        (
            "SAT",
            rs.satellite,
            Command::Layer {
                name: "satellite".to_string(),
                on: Some(!rs.satellite),
            },
        ),
        (
            "PLY",
            rs.show_polygons,
            Command::Layer {
                name: "alerts".to_string(),
                on: Some(!rs.show_polygons),
            },
        ),
        (
            "ARW",
            rs.show_arrows,
            Command::Layer {
                name: "arrows".to_string(),
                on: Some(!rs.show_arrows),
            },
        ),
        (
            "CEL",
            rs.show_cells,
            Command::Layer {
                name: "cells".to_string(),
                on: Some(!rs.show_cells),
            },
        ),
        (
            "STN",
            rs.show_stations,
            Command::Layer {
                name: "sites".to_string(),
                on: Some(!rs.show_stations),
            },
        ),
        (
            "BRB",
            rs.wind_barbs,
            Command::Layer {
                name: "barbs".to_string(),
                on: Some(!rs.wind_barbs),
            },
        ),
        (
            "HAL",
            rs.hail_track,
            Command::Layer {
                name: "hail".to_string(),
                on: Some(!rs.hail_track),
            },
        ),
        (
            "ROT",
            rs.rotation_track,
            Command::Layer {
                name: "rotation".to_string(),
                on: Some(!rs.rotation_track),
            },
        ),
        (
            "LTG",
            rs.lightning,
            Command::Layer {
                name: "lightning".to_string(),
                on: Some(!rs.lightning),
            },
        ),
    ]
}

impl Panel for RadarPanel {
    fn kind(&self) -> PanelKind {
        PanelKind::Radar
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut PanelCx) {
        let p = cx.palette;

        let dark = brightness(p.bg) < 128.0;
        if dark != self.dark {
            self.set_dark(dark);
        }
        self.map.units = match cx.shared.config.units {
            ss_core::UnitPref::F => ss_map::Units::Imperial,
            ss_core::UnitPref::C => ss_map::Units::Metric,
        };

        let rs = cx.shared.config.radar.clone();
        let now = ui.input(|i| i.time);
        let frames = cx
            .shared
            .weather_maps
            .as_ref()
            .map(|m| loop_frames(m, rs.nowcast))
            .unwrap_or_default();
        if let Some(on) = cx.req.radar_play.take() {
            self.playback.set_playing(on);
        }
        if let Some(d) = cx.req.radar_step.take() {
            self.playback.step(&frames, d);
        }
        if std::mem::take(&mut cx.req.radar_live) {
            self.playback.go_live(&frames);
        }
        let next_ready = self
            .map
            .layers
            .get_mut::<RadarLoopLayer>("radar")
            .map(|l| l.ready((self.playback.idx + 1) % frames.len().max(1)))
            .unwrap_or(1.0);
        self.playback.tick_with(&frames, now, next_ready);
        if rs.nexrad_site.is_none() && self.playback.playing {
            // Smooth crossfades need every frame while playing.
            ui.ctx().request_repaint();
        }

        self.sync_basemap(rs.imagery);
        self.sync_layers(cx, &rs, &frames, ui.ctx());
        self.sync_level2(cx, &rs, ui.ctx());
        self.handle_requests(cx);

        let stops = legend_stops(&rs);
        let spacing = ui.spacing().item_spacing.y;
        let mut below = BAR_H + spacing + LAYERS_H + spacing;
        if stops.is_some() {
            below += LEGEND_H + spacing;
        }
        let w = ui.available_width();
        let slot = cx.overlay;
        let overlay = slot.is_some();
        let map_h = if overlay {
            ui.available_height()
        } else {
            (ui.available_height() - below).max(64.0)
        };
        let r = ui.allocate_ui(vec2(w, map_h), |ui| self.map.show(ui)).inner;
        self.cross_section_window(ui.ctx(), ui.scope_id().with("xsect"), &rs, &cx.palette);

        if let Some(bar) = slot {
            // In the strip under the function keys: one row.
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("radar_controls_body")
                    .max_rect(bar)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            child.set_clip_rect(bar);
            child.spacing_mut().item_spacing.x = 6.0;
            // Layers and legend take what they need; playback (with its
            // time slider) gets the rest.
            let n_icons = layer_toggles(&rs).len() as f32 + 1.0;
            let layers_w = 112.0 + n_icons * 24.0;
            let legend_w = if stops.is_some() { 300.0 } else { 0.0 };
            let ctrl_w = (bar.width() - layers_w - legend_w - 40.0).max(300.0);
            child.allocate_ui_with_layout(
                vec2(ctrl_w, BAR_H),
                Layout::left_to_right(Align::Center),
                |ui| self.control_bar(ui, cx, &rs, &frames),
            );
            child.separator();
            child.allocate_ui_with_layout(
                vec2(layers_w, LAYERS_H),
                Layout::left_to_right(Align::Center),
                |ui| self.layers_bar_inner(ui, cx, &rs, false),
            );
            if let Some(s) = &stops {
                child.separator();
                ss_map::draw_color_legend_inline(&mut child, "dBZ", s);
            }
        } else {
            ui.allocate_ui_with_layout(
                vec2(w, BAR_H),
                Layout::left_to_right(Align::Center),
                |ui| self.control_bar(ui, cx, &rs, &frames),
            );
            ui.add_space(LAYERS_GAP);
            ui.allocate_ui_with_layout(
                vec2(w, LAYERS_H),
                Layout::left_to_right(Align::Center),
                |ui| self.layers_bar(ui, cx, &rs),
            );
            if let Some(s) = &stops {
                ui.add_space(LAYERS_GAP);
                draw_color_legend(ui, "dBZ", s);
            }
        }

        // Overlay: active location + zoom, AQI badge.
        let label = cx
            .location
            .as_ref()
            .map(|l| l.label.clone())
            .unwrap_or_else(|| "no location".to_string());
        let painter = ui.painter_at(r.rect);
        let text = format!("{label}  z{:.1}", self.map.viewport.zoom);
        // Same baseline as the "RADR <GO>" tag text along the top edge
        // (the tag is 10.5 pt monospace, 3 pt below the viewport top).
        let tag = painter.layout_no_wrap("R".into(), FontId::monospace(10.5), c32(p.a0));
        let base = r.rect.top() + 3.0 + ss_assets::text::galley_baseline(&tag);
        let (x, align) = if overlay {
            (r.rect.center().x, egui::Align::Center)
        } else {
            (r.rect.left() + 8.0, egui::Align::Min)
        };
        overlay_text_on_baseline(&painter, x, base, align, &text, c32(p.a0));
        if rs.aqi_badge {
            let key_blank = cx
                .shared
                .config
                .open_weather_map_api_key
                .as_deref()
                .is_none_or(|k| k.trim().is_empty());
            let text = if key_blank {
                "AQI: no API key".to_string()
            } else {
                match cx.shared.air_quality.get(cx.loc_id()) {
                    Some(a) => format!("AQI {} {}", a.aqi, a.label()),
                    None => "AQI: …".to_string(),
                }
            };
            let (aqi_pos, aqi_align) = if overlay {
                (r.rect.center_top() + vec2(0.0, 24.0), Align2::CENTER_TOP)
            } else {
                (r.rect.right_top() + vec2(-8.0, 6.0), Align2::RIGHT_TOP)
            };
            overlay_text(&painter, aqi_pos, aqi_align, &text, c32(p.a0));
        }
        // Badges for the tracked place: the nearest storm heading there,
        // and the nearest lightning.
        let mut badges: Vec<(String, Color32)> = Vec::new();
        if let Some(loc) = cx.location.as_ref() {
            if let Some(a) = cx.shared.arrivals.iter().find(|a| a.place_id == loc.id) {
                let t = a.cell.threat();
                let what = match t {
                    4 => "TORNADIC STORM",
                    3 => "ROTATING STORM",
                    2 => "SEVERE STORM",
                    _ => "STORM",
                };
                let when = ss_core::storms::eta_label(a.minutes);
                let text = if a.minutes < 2.0 {
                    format!("{what} OVERHEAD · {}", a.cell.key())
                } else {
                    format!("{what} {when} · {}", ss_core::storms::miss_label(a.miss_km))
                };
                badges.push((text, c32(super::storms::threat_color(t))));
            }
            if rs.lightning {
                let now_s = chrono::Utc::now().timestamp();
                let near = ss_net::glm::nearest(
                    &cx.shared.lightning,
                    loc.lat,
                    loc.lon,
                    LIGHTNING_NEAR_KM,
                    now_s - LIGHTNING_NEAR_S,
                );
                if let Some((d, b, f)) = near {
                    let text = format!(
                        "LTG {:.0} km {} · {} min",
                        d,
                        ss_core::weather::units::compass_8(b),
                        (now_s - f.time).max(0) / 60
                    );
                    let c = if d < 16.0 { c32(p.danger) } else { c32(p.a0) };
                    badges.push((text, c));
                }
            }
        }
        let mut dy = if rs.aqi_badge { 18.0 } else { 0.0 };
        for (text, c) in badges {
            let (pos, align) = if overlay {
                (
                    r.rect.center_top() + vec2(0.0, 24.0 + dy),
                    Align2::CENTER_TOP,
                )
            } else {
                (r.rect.right_top() + vec2(-8.0, 6.0 + dy), Align2::RIGHT_TOP)
            };
            let g = painter.layout_no_wrap(text, FontId::monospace(11.0), c);
            let at = align.anchor_size(pos, g.size()).expand2(vec2(5.0, 2.0));
            painter.rect_filled(at, 3.0, Color32::from_black_alpha(170));
            painter.galley(at.min + vec2(5.0, 2.0), g, c);
            dy += at.height() + 3.0;
        }
        self.map.layers.set_visible(
            "radar-sites",
            rs.show_stations && self.map.viewport.zoom >= MIN_SITE_ZOOM,
        );

        if rs.wind_barbs {
            self.request_winds(ui, cx, r.rect, now);
        }
        self.request_viewport_alerts(ui, cx, r.rect, now);

        // Hover tooltip: storm cells and reports first, then alerts, MD and
        // the SPC outlook.
        if let Some(h) = r.response.hover_pos() {
            let vp = self.map.viewport.clone();
            let storm_tip = ["tropical", "storm-cells", "storm-reports"]
                .iter()
                .find_map(|n| {
                    self.map
                        .layers
                        .get_mut::<StormLayer>(n)
                        .and_then(|l| l.hit_test(h, 10.0, &vp, r.rect))
                        .map(|m| m.tooltip.clone())
                });
            let tip = storm_tip.or_else(|| {
                ["alerts", "md", "spc-outlook", "tc-swaths"]
                    .iter()
                    .find_map(|n| hit_polygon(&mut self.map, n, h, r.rect))
                    .and_then(|(_, label)| label)
            });
            // Hail and rotation under the pointer.
            let (lon, lat) = vp.lonlat_from_screen(h, r.rect);
            let tracks: Vec<String> = cx
                .shared
                .tracks
                .values()
                .filter_map(|(_, t)| t.as_ref())
                .filter_map(|t| {
                    let v = t.grid.at(lat, lon)?;
                    (v >= tracks::threshold(t.kind)).then(|| {
                        format!(
                            "{} · last {}",
                            tracks::describe(t.kind, v),
                            ss_core::types::track_period_label(t.minutes)
                        )
                    })
                })
                .collect();
            let tip = match (tip, tracks.is_empty()) {
                (t, true) => t,
                (Some(t), false) => Some(format!("{t}\n{}", tracks.join("\n"))),
                (None, false) => Some(tracks.join("\n")),
            };
            if let Some(t) = tip {
                r.response.clone().on_hover_text_at_pointer(t);
            }
        }
        // Click: alert selection, or log an MD.
        if r.clicked.is_some() {
            if let Some(cp) = r.response.interact_pointer_pos() {
                if let Some((id, _)) = hit_polygon(&mut self.map, "alerts", cp, r.rect) {
                    cx.req.selected_alert_id = Some(id);
                } else if let Some((id, _)) = hit_polygon(&mut self.map, "md", cp, r.rect) {
                    let msg = cx.shared.mcd_alerts.iter().find(|a| a.id == id).map(|a| {
                        let head = a
                            .headline
                            .clone()
                            .unwrap_or_else(|| a.display_event.clone());
                        let lines: Vec<&str> = a
                            .description
                            .as_deref()
                            .unwrap_or("")
                            .lines()
                            .map(str::trim)
                            .filter(|l| !l.is_empty())
                            .take(3)
                            .collect();
                        if lines.is_empty() {
                            head
                        } else {
                            format!("{head}: {}", lines.join(" / "))
                        }
                    });
                    if let Some(m) = msg {
                        cx.shared.log(LogLevel::Info, m);
                    }
                }
            }
        }

        if r.selection_changed {
            cx.shared.selection_bbox = self.map.selected_bbox;
            if let Some(b) = self.map.selected_bbox {
                cx.shared.log(
                    LogLevel::Info,
                    format!(
                        "area selected: {:.2},{:.2} to {:.2},{:.2}; fetching alerts…",
                        b.south, b.west, b.north, b.east
                    ),
                );
                cx.tasks.area_alerts(b);
            }
        }
    }
}

/// A square icon button; `active` draws it in the accent colour.
fn icon_button(
    ui: &mut Ui,
    p: &ss_core::theme::Palette,
    glyph: &str,
    active: Option<bool>,
    tip: &str,
) -> egui::Response {
    let color = match active {
        Some(true) => c32(p.a0),
        Some(false) => c32a(p.text3),
        None => c32(p.text),
    };
    // Bare icons: amber when on, grey when off.
    let b = egui::Button::new(
        RichText::new(glyph)
            .font(crate::theme::icon(17.0))
            .color(color),
    )
    .frame(false)
    .min_size(vec2(22.0, 22.0));
    let r = ui.add(b);
    r.on_hover_text(tip)
}

/// Icon and description for a layer toggle from [`layer_toggles`].
pub fn layer_icon(label: &str) -> (&'static str, &'static str) {
    use ss_assets::symbols as i;
    match label {
        "IMG" => (i::PUBLIC, "Satellite imagery map"),
        "SAT" => (i::SATELLITE_ALT, "IR satellite"),
        "PLY" => (i::POLYLINE, "Alert polygons"),
        "ARW" => (i::NAVIGATION, "Storm motion arrows"),
        "CEL" => (i::RADIO_BUTTON_CHECKED, "Storm cell markers"),
        "STN" => (i::CELL_TOWER, "Radar sites"),
        "BRB" => (i::AIR, "Wind barbs"),
        "LTG" => (i::BOLT, "Lightning"),
        "HAL" => (
            i::WEATHER_HAIL,
            "Hail swaths (US): largest hail estimated by radar",
        ),
        "ROT" => (i::CYCLONE, "Rotation tracks (US): paths of rotating storms"),
        _ => (i::LAYERS, "Layer"),
    }
}

fn frame_time_label(time: i64, fmt: TimeFormat) -> String {
    let Some(utc) = chrono::DateTime::from_timestamp(time, 0) else {
        return "--:--".to_string();
    };
    let local = utc.with_timezone(&Local);
    match fmt {
        TimeFormat::H12 => local.format("%I:%M %p").to_string(),
        TimeFormat::H24 => local.format("%H:%M").to_string(),
    }
}

impl RadarPanel {
    /// Fetch the site's Level II scan and redraw the chosen tilt when
    /// anything changes.
    fn sync_level2(&mut self, cx: &mut PanelCx, rs: &RadarSettings, ctx: &egui::Context) {
        let want = rs
            .nexrad_site
            .as_ref()
            .map(|s| s.to_ascii_uppercase())
            .zip(rs.level2);
        let Some((site, product)) = want else {
            if let Some(l) = self.map.layers.get_mut::<ss_map::GeoImageLayer>("level2") {
                l.image = None;
                l.visible = false;
            }
            self.l2.clear();
            self.xs.clear();
            self.xs_tex = None;
            return;
        };
        let sh = &mut *cx.shared;
        let due = sh
            .level2_requested
            .get(&site)
            .is_none_or(|t| t.elapsed() > Duration::from_secs(240));
        if due {
            sh.level2_requested.insert(site.clone(), Instant::now());
            cx.tasks.level2(&site);
        }
        let Some(scan) = sh.level2.get(&site).cloned() else {
            return;
        };
        let cells = if ss_core::opera::is_opera(&site) {
            &sh.mosaic_cells
        } else {
            &sh.storm_cells
        };
        let motion = super::radar3d::motion_for_site(cells, &site);
        if let Some(line) = self.map.line {
            if let Some(x) = self.xs.update(&scan, product, line, motion) {
                self.xs_tex = x.map(|x| {
                    let tex = ctx.load_texture("xsect", x.image, egui::TextureOptions::LINEAR);
                    (tex, x.length_km)
                });
            }
            if self.xs.busy() {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        let done = self.l2.update(&scan, product, rs.level2_tilt, motion);
        if self.l2.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if let Some(l) = self.map.layers.get_mut::<ss_map::GeoImageLayer>("level2") {
            if let Some(img) = done {
                l.image = img.map(|img| {
                    let tex =
                        ctx.load_texture("level2-map", img.image, egui::TextureOptions::LINEAR);
                    (tex, img.bounds)
                });
            }
            l.visible = true;
            l.opacity = 1.0;
        }
    }

    /// The cross section under the drawn line, in a window over the radar.
    fn cross_section_window(
        &mut self,
        ctx: &egui::Context,
        id: egui::Id,
        rs: &RadarSettings,
        p: &ss_core::theme::Palette,
    ) {
        let (Some((tex, len)), Some(product)) = (&self.xs_tex, rs.level2) else {
            return;
        };
        if self.map.line.is_none() {
            return;
        }
        let (tex_id, len) = (tex.id(), *len);
        let mut open = true;
        egui::Window::new(format!("Cross section · {}", product.label()))
            .id(id)
            .open(&mut open)
            .default_size(vec2(640.0, 300.0))
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                let avail = ui.available_size();
                let (rect, _) = ui.allocate_exact_size(
                    vec2(avail.x.max(300.0), (avail.y - 6.0).max(160.0)),
                    egui::Sense::hover(),
                );
                let plot = egui::Rect::from_min_max(
                    rect.min + vec2(34.0, 4.0),
                    rect.max - vec2(6.0, 18.0),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(plot, 0.0, Color32::from_rgb(10, 12, 18));
                painter.image(
                    tex_id,
                    plot,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                let font = egui::FontId::monospace(10.0);
                let faint = c32a(p.text3);
                let top = super::l2map::XS_TOP_KM;
                for km in (0..=top as i32).step_by(4) {
                    let y = plot.bottom() - plot.height() * km as f32 / top;
                    painter.hline(
                        plot.x_range(),
                        y,
                        egui::Stroke::new(0.5, Color32::from_white_alpha(28)),
                    );
                    painter.text(
                        egui::pos2(plot.left() - 4.0, y),
                        egui::Align2::RIGHT_CENTER,
                        format!("{km}"),
                        font.clone(),
                        faint,
                    );
                }
                painter.text(
                    egui::pos2(rect.left(), plot.top()),
                    egui::Align2::LEFT_TOP,
                    "km",
                    font.clone(),
                    faint,
                );
                let step = if len > 150.0 {
                    50.0
                } else if len > 60.0 {
                    20.0
                } else {
                    10.0
                };
                let mut d = 0.0;
                while d <= len + 0.1 {
                    let x = plot.left() + plot.width() * d / len.max(0.1);
                    painter.text(
                        egui::pos2(x, plot.bottom() + 2.0),
                        egui::Align2::CENTER_TOP,
                        format!("{d:.0}"),
                        font.clone(),
                        faint,
                    );
                    d += step;
                }
                let gold = Color32::from_rgb(0xff, 0xd0, 0x40);
                painter.text(
                    plot.left_top() + vec2(4.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    "A",
                    font.clone(),
                    gold,
                );
                painter.text(
                    plot.right_top() + vec2(-4.0, 2.0),
                    egui::Align2::RIGHT_TOP,
                    "B",
                    font.clone(),
                    gold,
                );
                painter.text(
                    egui::pos2(plot.right(), plot.bottom() + 2.0),
                    egui::Align2::RIGHT_TOP,
                    format!("{len:.0} km"),
                    font,
                    faint,
                );
            });
        if !open {
            self.map.line = None;
            self.xs.clear();
            self.xs_tex = None;
        }
    }

    /// The NEXRAD product menu (Level II products decoded here, or the
    /// NWS images) and, for Level II, the tilt.
    fn product_controls(&mut self, ui: &mut Ui, cx: &mut PanelCx, rs: &RadarSettings, site: &str) {
        use ss_core::radar3d::L2Product;
        let p = cx.palette;
        let current = match rs.level2 {
            Some(l) => format!("Level II {}", l.short()),
            None => rs.nexrad_product.label().to_string(),
        };
        let (mut pick_l2, mut pick_l3) = (None, None);
        egui::ComboBox::from_id_salt("nexrad_product")
            .width(150.0)
            .selected_text(current)
            .show_ui(ui, |ui| {
                ui.label(
                    RichText::new("Level II · every tilt")
                        .small()
                        .color(c32a(p.text3)),
                );
                for prod in L2Product::ALL {
                    if ui
                        .selectable_label(rs.level2 == Some(prod), prod.label())
                        .clicked()
                    {
                        pick_l2 = Some(prod);
                    }
                }
                // The NWS's ready-made images exist for US radars only.
                if ss_core::opera::is_opera(site) {
                    return;
                }
                ui.separator();
                ui.label(
                    RichText::new("NWS images · lowest tilt")
                        .small()
                        .color(c32a(p.text3)),
                );
                for prod in RadarProduct::ALL {
                    let on = rs.level2.is_none() && rs.nexrad_product == prod;
                    if ui.selectable_label(on, prod.label()).clicked() {
                        pick_l3 = Some(prod);
                    }
                }
            });
        if let Some(l2) = pick_l2 {
            cx.shared.config.radar.level2 = Some(l2);
            cx.shared.dirty_config = true;
        }
        if let Some(l3) = pick_l3 {
            cx.shared.config.radar.level2 = None;
            cx.shared.dirty_config = true;
            cx.emit.push(Command::Product(l3));
        }
        let Some(product) = rs.level2 else {
            return;
        };
        let key = site.to_ascii_uppercase();
        let Some(scan) = cx.shared.level2.get(&key).cloned() else {
            let text = match cx.shared.level2_errors.get(&key) {
                Some(e) => format!("Level II: {e}"),
                None => "loading Level II…".to_string(),
            };
            ui.label(RichText::new(text).monospace().color(c32a(p.text3)));
            return;
        };
        let n = scan.polar.sweeps.len();
        if n == 0 {
            return;
        }
        let tilt = rs.level2_tilt.min(n - 1);
        if icon_button(ui, &p, ss_assets::symbols::CHEVRON_LEFT, None, "Lower tilt").clicked()
            && tilt > 0
        {
            cx.shared.config.radar.level2_tilt = tilt - 1;
            cx.shared.dirty_config = true;
        }
        ui.label(
            RichText::new(format!("{:.1}°", scan.polar.sweeps[tilt].elevation_deg))
                .monospace()
                .color(c32(p.a0)),
        )
        .on_hover_text(format!("Tilt {} of {n}", tilt + 1));
        if icon_button(
            ui,
            &p,
            ss_assets::symbols::CHEVRON_RIGHT,
            None,
            "Higher tilt",
        )
        .clicked()
            && tilt + 1 < n
        {
            cx.shared.config.radar.level2_tilt = tilt + 1;
            cx.shared.dirty_config = true;
        }
        let time = chrono::DateTime::parse_from_rfc3339(&scan.time)
            .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
            .unwrap_or_default();
        ui.label(RichText::new(time).monospace().color(c32a(p.text3)));

        // The product's colours (reflectivity has the usual legend).
        if product != L2Product::Reflectivity {
            let (lo, hi) = product.moment().range();
            let (lo, hi) = match product {
                L2Product::Cc => (0.5, 1.02),
                L2Product::Zdr => (-2.0, 6.0),
                _ => (lo, hi),
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(96.0, 10.0), egui::Sense::hover());
            for k in 0..24 {
                let v = lo + (hi - lo) * (k as f32 + 0.5) / 24.0;
                let c = product.color(v);
                let x0 = rect.left() + rect.width() * k as f32 / 24.0;
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x0, rect.top()),
                        egui::pos2(x0 + rect.width() / 24.0 + 0.5, rect.bottom()),
                    ),
                    0.0,
                    Color32::from_rgb(c[0], c[1], c[2]),
                );
            }
            let hint = match product {
                L2Product::Velocity | L2Product::StormRelative => {
                    format!("{lo:.0}…{hi:.0} m/s, green toward")
                }
                L2Product::Cc => format!("{lo:.2}…{hi:.2}"),
                _ => format!("{lo:.0}…{hi:.0} dB"),
            };
            ui.label(RichText::new(hint).small().color(c32a(p.text3)));
        }
        if self.l2.busy() {
            ui.spinner();
        }
    }

    fn control_bar(
        &mut self,
        ui: &mut Ui,
        cx: &mut PanelCx,
        rs: &RadarSettings,
        frames: &[LoopFrame],
    ) {
        let p = cx.palette;
        if let Some(site) = &rs.nexrad_site {
            let station = cx
                .shared
                .radar_stations
                .iter()
                .find(|s| s.id.eq_ignore_ascii_case(site))
                .map(|s| s.name.clone());
            let net = if ss_core::opera::is_opera(site) {
                "OPERA"
            } else {
                "NEXRAD"
            };
            let mut text = format!("{net} {site}");
            if let Some(name) = &station {
                text.push_str(&format!(" · {name}"));
            }
            ui.label(RichText::new(text).monospace().color(c32(p.text)));
            self.product_controls(ui, cx, rs, site);
            if station.is_none() && !cx.shared.radar_stations.is_empty() {
                ui.label(
                    RichText::new("site not found")
                        .monospace()
                        .color(c32(p.danger)),
                );
            }
            if icon_button(
                ui,
                &p,
                ss_assets::symbols::MAP,
                None,
                "Back to the composite radar",
            )
            .clicked()
            {
                cx.emit.push(Command::Site(None));
            }
            return;
        }

        if frames.is_empty() {
            ui.label(
                RichText::new("radar: loading frames…")
                    .monospace()
                    .color(c32a(p.text3)),
            );
            return;
        }
        use ss_assets::symbols as i;
        if icon_button(ui, &p, i::SKIP_PREVIOUS, None, "Previous frame").clicked() {
            self.playback.step(frames, -1);
        }
        let (play, tip) = if self.playback.playing {
            (i::PAUSE, "Pause the loop")
        } else {
            (i::PLAY_ARROW, "Play the loop")
        };
        if icon_button(ui, &p, play, None, tip).clicked() {
            let on = !self.playback.playing;
            self.playback.set_playing(on);
        }
        if icon_button(ui, &p, i::SKIP_NEXT, None, "Next frame").clicked() {
            self.playback.step(frames, 1);
        }
        let live = self.playback.is_live(frames);
        let live_text = RichText::new("LIVE").monospace();
        let live_text = if live {
            live_text.color(c32(p.a0)).strong()
        } else {
            live_text.color(c32a(p.text3))
        };
        if ui
            .add(
                egui::Button::new(live_text)
                    .frame(false)
                    .min_size(vec2(0.0, 22.0)),
            )
            .on_hover_text("Jump to the newest frame")
            .clicked()
        {
            self.playback.go_live(frames);
        }

        let cur = self.playback.idx.min(frames.len() - 1);
        let mut idx = cur;
        ui.spacing_mut().slider_width = (ui.available_width() - 190.0).max(60.0);
        ui.add(egui::Slider::new(&mut idx, 0..=frames.len() - 1).show_value(false));
        if idx != cur {
            self.playback.scrub(frames, idx);
        }

        let f = &frames[self.playback.idx.min(frames.len() - 1)];
        ui.label(
            RichText::new(frame_time_label(f.time, cx.shared.config.time_format))
                .monospace()
                .color(c32(p.text)),
        );
        if f.nowcast {
            ui.label(RichText::new("NOWCAST").monospace().color(c32a(p.text3)));
        } else if live {
            ui.label(RichText::new("LIVE").monospace().color(c32(p.a0)));
        }
    }

    /// Second row of the floating box: color-scheme selector plus layer
    /// toggles. Emits `:radar scheme` / `:layer` commands so the changes go
    /// through the normal pipeline (dirty config + log). Wrapped in a
    /// horizontal scroll so the 240 px docked slot still fits everything.
    fn layers_bar(&mut self, ui: &mut Ui, cx: &mut PanelCx, rs: &RadarSettings) {
        self.layers_bar_inner(ui, cx, rs, true);
    }

    /// The layers row; `scroll` wraps it in a horizontal scroll area (the
    /// narrow docked pane), otherwise it takes its natural width.
    fn layers_bar_inner(
        &mut self,
        ui: &mut Ui,
        cx: &mut PanelCx,
        rs: &RadarSettings,
        scroll: bool,
    ) {
        let p = cx.palette;
        let schemes = scheme_options(cx.shared.weather_maps.as_ref());
        let cur_name = schemes
            .iter()
            .find(|(id, _)| *id == rs.color_scheme)
            .map(|(id, n)| format!("{id}:{n}"))
            .unwrap_or_else(|| format!("{}", rs.color_scheme));
        let row = |ui: &mut Ui| {
            ui.horizontal(|ui| {
                    // Measure (and, with a Level II scan, cross section).
                    {
                        let armed = self.map.line_mode;
                        let xsect = rs.nexrad_site.is_some() && rs.level2.is_some();
                        let tip = match (armed, xsect) {
                            (true, true) => "Now drag a line across a storm",
                            (true, false) => "Now drag a line on the map",
                            (false, true) => "Cross section and distance: click, then drag a line across a storm to see it from the side",
                            (false, false) => "Distance and bearing: click, then drag a line on the map (click again to clear it)",
                        };
                        let shown = armed || self.map.line.is_some();
                        if icon_button(ui, &p, ss_assets::symbols::STRAIGHTEN, Some(shown), tip).clicked() {
                            if !armed && self.map.line.is_some() {
                                // Clear the last line (and its cross section).
                                self.map.line = None;
                                self.xs_tex = None;
                            } else {
                                self.map.line_mode = !armed;
                            }
                        }
                    }
                    egui::ComboBox::from_id_salt("radar_scheme")
                        .selected_text(RichText::new(cur_name).monospace().size(11.0))
                        .width(92.0)
                        .show_ui(ui, |ui| {
                            for (id, name) in &schemes {
                                let sel = *id == rs.color_scheme;
                                if ui.selectable_label(sel, format!("{id}: {name}")).clicked()
                                    && !sel
                                {
                                    cx.emit.push(Command::Radar(RadarCmd::Scheme(*id)));
                                }
                            }
                        });
                    for (label, active, cmd) in layer_toggles(rs) {
                        let (glyph, what) = layer_icon(label);
                        let tip = format!("{what}: {}", if active { "on" } else { "off" });
                        if icon_button(ui, &p, glyph, Some(active), &tip).clicked() {
                            cx.emit.push(cmd);
                        }
                    }
                });
        };
        if scroll {
            egui::ScrollArea::horizontal()
                .id_salt("radar_layers_scroll")
                .auto_shrink([false, true])
                .show(ui, row);
        } else {
            row(ui);
        }
    }

    /// Auto-load alerts for the visible area when the view moved or the data is old.
    /// Record viewport motion and report whether the view has been still for a second.
    fn track_view(&mut self, ui: &Ui, now: f64) -> bool {
        let v = &self.map.viewport;
        let sig = (
            v.center_lon.to_bits(),
            v.center_lat.to_bits(),
            v.zoom.to_bits(),
        );
        if sig != self.view_sig {
            self.view_sig = sig;
            self.last_view_change = now;
        }
        let settled = now - self.last_view_change >= 1.0;
        if !settled {
            ui.ctx().request_repaint_after(Duration::from_millis(1100));
        }
        settled
    }

    fn request_viewport_alerts(&mut self, ui: &Ui, cx: &mut PanelCx, rect: egui::Rect, now: f64) {
        if !cx.shared.config.radar.viewport_alerts {
            if !cx.shared.viewport_alerts.is_empty() {
                cx.shared.viewport_alerts.clear();
                cx.shared.viewport_alerts_bbox = None;
                cx.shared.bump_alerts();
            }
            self.last_alert_req = None;
            return;
        }
        let settled = self.track_view(ui, now);
        let v = &self.map.viewport;
        if v.zoom < VIEWPORT_ALERTS_MIN_ZOOM || v.is_animating() {
            return;
        }
        let bbox = v.visible_bbox(rect);
        let last = self.last_alert_req.as_ref().map(|(b, t)| (b, *t));
        if bbox_request_due(last, now, &bbox, settled, 120.0) {
            cx.tasks.viewport_alerts(bbox);
            self.last_alert_req = Some((bbox, now));
        }
    }

    /// Ask for fresh METAR winds when the view has moved or the data is old.
    fn request_winds(&mut self, ui: &Ui, cx: &mut PanelCx, rect: egui::Rect, now: f64) {
        let settled = self.track_view(ui, now);
        let v = &self.map.viewport;
        if v.zoom < BARB_MIN_ZOOM || v.is_animating() {
            return;
        }
        let bbox = v.visible_bbox(rect);
        let last = self.last_wind_req.as_ref().map(|(b, t)| (b, *t));
        if wind_request_due(last, now, &bbox, settled) {
            cx.tasks.wind_obs(bbox);
            self.last_wind_req = Some((bbox, now));
        }
    }
}

impl RadarPanel {
    /// Update layer visibility / templates and rebuild item layers whose inputs changed.
    fn sync_layers(
        &mut self,
        cx: &mut PanelCx,
        rs: &RadarSettings,
        frames: &[LoopFrame],
        ctx: &egui::Context,
    ) {
        let p = cx.palette;
        let sh = &*cx.shared;
        let overrides = &sh.config.alert_type_overrides;
        let host = sh.config.libre_wxr_host.trim_end_matches('/').to_string();
        let composite = rs.nexrad_site.is_none();
        let layers = &mut self.map.layers;

        // --- satellite ---
        let sat = sh
            .weather_maps
            .as_ref()
            .and_then(|m| m.satellite_infrared.last())
            .map(|f| f.path.clone());
        let sat_key = sat.as_ref().map(|path| format!("sat:{path}"));
        let (h, sp) = (host.clone(), sat.clone().unwrap_or_default());
        sync_raster(
            layers,
            "satellite",
            rs.satellite && sat.is_some(),
            0.75,
            sat_key,
            move |t| satellite_tile_url(&h, &sp, u32::from(t.z), t.x, t.y),
        );

        // --- composite radar: the whole loop in one fluid layer ---
        let dark = self.dark;
        if let Some(l) = layers.get_mut::<RadarLoopLayer>("radar") {
            l.visible = composite && !frames.is_empty();
            l.opacity = rs.opacity;
            l.set_frames(
                frames
                    .iter()
                    .map(|f| {
                        let (h, rsc, path) = (host.clone(), rs.clone(), f.path.clone());
                        LoopFrameSource {
                            key: radar_layer_key(&f.path, rs, dark),
                            url: std::sync::Arc::new(move |t| {
                                radar_tile_url(
                                    &h,
                                    &path,
                                    u32::from(t.z),
                                    t.x,
                                    t.y,
                                    rsc.color_scheme,
                                    rsc.smooth,
                                    rsc.snow,
                                    overlay_style(rsc.show_arrows, dark),
                                    overlay_style(rsc.show_cells, dark),
                                )
                            }),
                        }
                    })
                    .collect(),
            );
            l.pos = self.playback.pos;
        }

        // --- terrain relief (the photos already show relief) ---
        if let Some(l) = layers.get_mut::<RasterTileLayer>("terrain") {
            l.visible = rs.terrain && !rs.imagery;
            l.opacity = if self.dark { 0.28 } else { 0.42 };
        }

        // --- labels and roads ---
        layers.set_visible("labels", rs.labels);
        layers.set_visible("roads", rs.labels && rs.imagery);

        // --- storm cells and storm reports ---
        layers.set_visible("storm-cells", rs.storm_cells);
        layers.set_visible("storm-reports", rs.storm_reports);
        layers.set_visible("tropical", rs.tropical);
        layers.set_visible("tc-swaths", rs.tropical);
        let sig = hash_of(&(
            sh.storms_at.map(|t| t.timestamp()),
            sh.storm_cells.len(),
            sh.storm_reports.len(),
            sh.mosaic_cells.len(),
            sh.mosaic_cells.first().map(|c| c.valid.clone()),
            sh.tropical.len(),
            sh.tropical.first().map(|t| t.updated.clone()),
        ));
        if sig != self.storms_sig {
            self.storms_sig = sig;
            if let Some(l) = layers.get_mut::<StormLayer>("storm-cells") {
                let mut all = sh.storm_cells.clone();
                all.extend(sh.mosaic_cells.iter().cloned());
                l.marks = storm_cell_marks(&all);
            }
            if let Some(l) = layers.get_mut::<StormLayer>("tropical") {
                l.marks = tropical_marks(&sh.tropical);
            }
            if let Some(l) = layers.get_mut::<PolygonLayer>("tc-swaths") {
                l.set_items(tropical_swaths(&sh.tropical));
            }
            if let Some(l) = layers.get_mut::<StormLayer>("storm-reports") {
                l.marks = storm_report_marks(&sh.storm_reports);
            }
        }

        // --- OpenWeatherMap tiles ---
        let owm_key = sh
            .config
            .open_weather_map_api_key
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        for (name, on, which) in [
            ("owm-wind", rs.owm_wind, OwmLayer::Wind),
            ("owm-temp", rs.owm_temp, OwmLayer::Temp),
            ("owm-precip", rs.owm_precip, OwmLayer::Precip),
        ] {
            let vis = on && !owm_key.is_empty();
            let k = owm_key.clone();
            sync_raster(
                layers,
                name,
                vis,
                0.55,
                Some(format!("{name}:{}", owm_key.len())),
                move |t| owm::tile_url(which, u32::from(t.z), t.x, t.y, &k),
            );
        }

        // --- NEXRAD single-site WMS + range ring ---
        let wanted = rs
            .nexrad_site
            .as_ref()
            .map(|s| (s.to_ascii_uppercase(), rs.nexrad_product));
        if wanted != self.nexrad_cfg {
            if let Some((site, product)) = &wanted {
                if let Some(pos) = layers.layers.iter().position(|l| l.name() == "nexrad") {
                    layers.layers[pos] = Box::new(nexrad_layer(site, *product));
                }
                if let Some(l) = layers.get_mut::<WmsImageLayer>("nexrad") {
                    l.reset();
                }
            }
            self.nexrad_cfg = wanted.clone();
        }
        let nws_images = wanted
            .as_ref()
            .is_some_and(|(s, _)| !ss_core::opera::is_opera(s));
        layers.set_visible("nexrad", nws_images && rs.level2.is_none());
        let station = wanted.as_ref().and_then(|(site, _)| {
            sh.radar_stations
                .iter()
                .find(|s| s.id.eq_ignore_ascii_case(site))
        });
        if let Some(l) = layers.get_mut::<RangeRingLayer>("range-ring") {
            if let Some(s) = station {
                l.lon = s.lon;
                l.lat = s.lat;
            }
            l.radius_m = ncep::RANGE_RING_M;
            l.color = c32a(p.text2);
        }
        layers.set_visible("range-ring", station.is_some());

        // --- SPC outlook ---
        let ssig = hash_of(&(
            sh.spc_outlook
                .iter()
                .map(|f| (f.code.as_str(), f.level, f.fill.as_str(), f.stroke.as_str()))
                .collect::<Vec<_>>(),
            sh.spc_outlook.len(),
        ));
        if ssig != self.spc_sig {
            self.spc_sig = ssig;
            if let Some(l) = layers.get_mut::<PolygonLayer>("spc-outlook") {
                l.set_items(spc_items(&sh.spc_outlook));
            }
        }
        layers.set_visible("spc-outlook", sh.config.spc_outlook_enabled);

        // --- mesoscale discussions ---
        let is_poly = |a: &&ss_core::Alert| {
            matches!(
                a.geometry,
                Some(Geometry::Polygon(_)) | Some(Geometry::MultiPolygon(_))
            )
        };
        let msig = hash_of(
            &sh.mcd_alerts
                .iter()
                .filter(is_poly)
                .map(|a| {
                    (
                        a.id.as_str(),
                        a.headline.as_deref(),
                        effective_color(&a.class, overrides),
                    )
                })
                .collect::<Vec<_>>(),
        );
        if msig != self.md_sig {
            self.md_sig = msig;
            let items: Vec<PolyItem> = sh
                .mcd_alerts
                .iter()
                .filter(is_poly)
                .map(|a| {
                    let [r, g, b] = effective_color(&a.class, overrides);
                    PolyItem {
                        id: a.id.clone(),
                        geometry: a.geometry.clone().unwrap_or(Geometry::Other),
                        fill: Color32::from_rgba_unmultiplied(r, g, b, 30),
                        stroke: Color32::from_rgb(r, g, b),
                        stroke_width: 2.0,
                        pulse: false,
                        label: Some(
                            a.headline
                                .clone()
                                .unwrap_or_else(|| a.display_event.clone()),
                        ),
                    }
                })
                .collect();
            if let Some(l) = layers.get_mut::<PolygonLayer>("md") {
                l.set_items(items);
            }
        }
        layers.set_visible("md", rs.md_polygons);
        layers.set_visible("alerts", rs.show_polygons);

        // --- wind barbs ---
        let bsig = hash_of(&(
            sh.wind_obs
                .iter()
                .map(|w| {
                    (
                        w.station.id.as_str(),
                        w.obs.wind_direction_deg.map(f64::to_bits),
                        w.obs.wind_speed_kt.map(f64::to_bits),
                        w.obs.wind_gust_kt.map(f64::to_bits),
                    )
                })
                .collect::<Vec<_>>(),
            p.text,
        ));
        if bsig != self.barbs_sig {
            self.barbs_sig = bsig;
            if let Some(l) = layers.get_mut::<WindBarbLayer>("barbs") {
                l.color = c32(p.text);
                l.barbs = sh
                    .wind_obs
                    .iter()
                    .filter_map(|w| {
                        Some(Barb {
                            lon: w.station.lon,
                            lat: w.station.lat,
                            dir_deg: w.obs.wind_direction_deg? as f32,
                            speed_kt: w.obs.wind_speed_kt? as f32,
                            gust_kt: w.obs.wind_gust_kt.map(|g| g as f32),
                        })
                    })
                    .collect();
            }
        }
        layers.set_visible("barbs", rs.wind_barbs);

        // --- hail and rotation tracks ---
        for (kind, name, on) in [
            (ss_net::mrms::TrackKind::Hail, "hail-track", rs.hail_track),
            (
                ss_net::mrms::TrackKind::Rotation,
                "rotation-track",
                rs.rotation_track,
            ),
            (ss_net::mrms::TrackKind::Swath, "swath-track", rs.hail_track),
        ] {
            let track = sh.tracks.get(&kind).and_then(|(_, t)| t.clone());
            let key = track.as_ref().map(|t| Arc::as_ptr(t) as usize);
            if self.track_keys.get(&kind) != Some(&key) {
                self.track_keys.insert(kind, key);
                if let Some(l) = layers.get_mut::<ss_map::GeoImageLayer>(name) {
                    l.image = track.map(|t| {
                        let tex =
                            ctx.load_texture(name, t.image.clone(), egui::TextureOptions::NEAREST);
                        (tex, t.bounds)
                    });
                }
            }
            if let Some(l) = layers.get_mut::<ss_map::GeoImageLayer>(name) {
                l.visible = on;
                l.opacity = 0.9;
            }
        }

        // --- lightning ---
        layers.set_visible("lightning-mtg", rs.lightning);
        let credit = " · Lightning: NOAA GOES GLM, EUMETSAT";
        let opera = " · Radar: EUMETNET OPERA (CC BY 4.0)";
        let base = self
            .map
            .attribution
            .trim_end_matches(credit)
            .trim_end_matches(opera)
            .to_string();
        let mut attribution = base;
        if rs
            .nexrad_site
            .as_deref()
            .is_some_and(ss_core::opera::is_opera)
        {
            attribution.push_str(opera);
        }
        if rs.lightning {
            attribution.push_str(credit);
        }
        self.map.attribution = attribution;
        let now_s = chrono::Utc::now().timestamp();
        if let Some(l) = layers.get_mut::<ss_map::LightningLayer>("lightning") {
            l.visible = rs.lightning;
            if rs.lightning {
                l.flashes = sh
                    .lightning
                    .iter()
                    .map(|f| (f.lon, f.lat, (now_s - f.time) as f32 / 60.0))
                    .collect();
            }
        }

        // --- alert polygons ---
        let alerts = sh.all_alerts_for(cx.loc_id());
        let sig = hash_of(&(
            alerts
                .iter()
                .map(|a| {
                    (
                        a.id.as_str(),
                        a.class.as_str(),
                        effective_color(&a.class, overrides),
                    )
                })
                .collect::<Vec<_>>(),
            sh.config.storm_polygons_enabled,
        ));
        if sig != self.alerts_sig {
            self.alerts_sig = sig;
            let mut items: Vec<(StormTier, PolyItem)> = alerts
                .iter()
                .filter(|a| {
                    matches!(
                        a.geometry,
                        Some(Geometry::Polygon(_)) | Some(Geometry::MultiPolygon(_))
                    )
                })
                .map(|a| {
                    let [r, g, b] = effective_color(&a.class, overrides);
                    let tier = storm_tier(&a.class);
                    (
                        tier,
                        PolyItem {
                            id: a.id.clone(),
                            geometry: a.geometry.clone().unwrap_or(Geometry::Other),
                            fill: Color32::from_rgba_unmultiplied(r, g, b, 46),
                            stroke: Color32::from_rgb(r, g, b),
                            stroke_width: 1.5,
                            pulse: sh.config.storm_polygons_enabled && tier != StormTier::None,
                            label: Some(a.display_event.clone()),
                        },
                    )
                })
                .collect();
            // Storm-tier warnings draw above zone-based polygons.
            items.sort_by_key(|(t, _)| *t);
            if let Some(l) = layers.get_mut::<PolygonLayer>("alerts") {
                l.set_items(items.into_iter().map(|(_, i)| i).collect());
            }
        }

        // --- saved locations ---
        let lsig = hash_of(&(
            sh.locations
                .saved_locations
                .iter()
                .map(|l| {
                    (
                        l.id.as_str(),
                        l.lat.to_bits(),
                        l.lon.to_bits(),
                        l.label.as_str(),
                    )
                })
                .collect::<Vec<_>>(),
            cx.loc_id(),
            p.a0,
            p.text2,
        ));
        if lsig != self.locations_sig {
            self.locations_sig = lsig;
            let active = cx.loc_id();
            let markers: Vec<Marker> = sh
                .locations
                .saved_locations
                .iter()
                .map(|l| {
                    let is_active = active == l.id;
                    Marker {
                        id: l.id.clone(),
                        lon: l.lon,
                        lat: l.lat,
                        kind: if is_active {
                            MarkerKind::Crosshair
                        } else {
                            MarkerKind::Pin
                        },
                        color: if is_active { c32(p.a0) } else { c32a(p.text2) },
                        label: Some(l.label.clone()),
                    }
                })
                .collect();
            if let Some(l) = self.map.layers.get_mut::<MarkerLayer>("locations") {
                l.markers = markers;
            }
        }

        // --- radar sites (liveness is time dependent: refresh once a minute) ---
        let now = Utc::now();
        let ssig = hash_of(&(
            sh.radar_stations.len(),
            now.timestamp() / 60,
            p.safe,
            p.text3,
        ));
        if ssig != self.stations_sig {
            self.stations_sig = ssig;
            let markers: Vec<Marker> = sh
                .radar_stations
                .iter()
                .map(|s| {
                    let live = s
                        .last_received_time
                        .as_deref()
                        .is_some_and(|t| is_live(t, now));
                    Marker {
                        id: s.id.clone(),
                        lon: s.lon,
                        lat: s.lat,
                        kind: MarkerKind::RadarSite { live },
                        color: if live { c32(p.safe) } else { c32a(p.text3) },
                        label: Some(s.id.clone()),
                    }
                })
                .collect();
            if let Some(l) = self.map.layers.get_mut::<MarkerLayer>("radar-sites") {
                l.markers = markers;
            }
        }
    }

    /// Apply recenter / zoom / focus / clear requests posted for this window.
    fn handle_requests(&mut self, cx: &mut PanelCx) {
        let loc = cx.location.clone();
        let req = &mut *cx.req;

        if !self.positioned {
            if let Some(l) = &loc {
                // First frame with a known location: start there at zoom 7.
                self.map.viewport.fly_to(l.lon, l.lat, 7.0);
                self.positioned = true;
                req.map_recenter = false;
            }
        }
        if req.map_recenter {
            req.map_recenter = false;
            if let Some(l) = &loc {
                let z = self.map.viewport.zoom;
                self.map.viewport.fly_to(l.lon, l.lat, z);
            }
        }
        if let Some(z) = req.map_zoom.take() {
            let v = &self.map.viewport;
            let (lon, lat) = (v.center_lon, v.center_lat);
            self.map.viewport.fly_to(lon, lat, f64::from(z));
        }
        if let Some(b) = req.pending_focus.take() {
            let size = self.map.viewport.size;
            let z = fit_zoom(&b, size);
            self.map
                .viewport
                .fly_to((b.west + b.east) / 2.0, (b.south + b.north) / 2.0, z);
        }
        if req.clear_selection {
            req.clear_selection = false;
            self.map.clear_selection();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss_core::RadarFrame;

    #[test]
    fn fit_zoom_is_higher_for_smaller_boxes() {
        let size = vec2(800.0, 600.0);
        let big = fit_zoom(&BBox::new(-110.0, 30.0, -100.0, 40.0), size);
        let small = fit_zoom(&BBox::new(-101.0, 34.0, -100.5, 34.5), size);
        assert!(small > big);
        assert!((2.0..=12.0).contains(&big) && (2.0..=12.0).contains(&small));
    }

    #[test]
    fn fit_zoom_handles_degenerate_box() {
        let z = fit_zoom(&BBox::new(-100.0, 35.0, -100.0, 35.0), vec2(800.0, 600.0));
        assert_eq!(z, 12.0);
    }

    fn rf(time: i64) -> RadarFrame {
        RadarFrame {
            time,
            path: format!("/v2/radar/{time}"),
        }
    }

    fn maps(past: &[i64], now: &[i64]) -> WeatherMaps {
        WeatherMaps {
            radar_past: past.iter().map(|t| rf(*t)).collect(),
            radar_nowcast: now.iter().map(|t| rf(*t)).collect(),
            ..Default::default()
        }
    }

    fn frames(past: &[i64], now: &[i64]) -> Vec<LoopFrame> {
        loop_frames(&maps(past, now), true)
    }

    #[test]
    fn loop_frames_orders_past_then_nowcast() {
        let f = frames(&[10, 20], &[30]);
        assert_eq!(
            f.iter().map(|x| (x.time, x.nowcast)).collect::<Vec<_>>(),
            vec![(10, false), (20, false), (30, true)]
        );
        assert_eq!(f[0].path, "/v2/radar/10");
        assert_eq!(loop_frames(&maps(&[10, 20], &[30]), false).len(), 2);
    }

    #[test]
    fn latest_observed_index_cases() {
        assert_eq!(latest_observed_index(&frames(&[10, 20, 30], &[40, 50])), 2);
        assert_eq!(latest_observed_index(&frames(&[], &[40, 50])), 0);
        assert_eq!(latest_observed_index(&[]), 0);
    }

    #[test]
    fn tick_advances_after_interval_and_wraps() {
        let f = frames(&[1, 2, 3], &[]);
        let mut pb = Playback::new();
        pb.tick(&f, 0.0);
        assert_eq!(pb.idx, 2);
        pb.set_playing(true);
        pb.tick(&f, 10.0);
        assert_eq!(pb.idx, 2, "first tick only starts the clock");
        pb.tick(&f, 10.5);
        assert_eq!(pb.idx, 2);
        pb.tick(&f, 10.9);
        assert_eq!(pb.idx, 0, "wraps past the last frame");
        pb.tick(&f, 11.8);
        assert_eq!(pb.idx, 1);
    }

    #[test]
    fn playing_includes_nowcast_frames() {
        let f = frames(&[1, 2], &[3]);
        let mut pb = Playback::new();
        pb.tick(&f, 0.0);
        pb.set_playing(true);
        pb.tick(&f, 1.0);
        pb.tick(&f, 2.0);
        assert_eq!(pb.idx, 2);
        assert!(!pb.is_live(&f));
    }

    #[test]
    fn step_back_from_zero_wraps_and_stops_tracking() {
        let f = frames(&[1, 2, 3], &[]);
        let mut pb = Playback::new();
        pb.tick(&f, 0.0);
        pb.scrub(&f, 0);
        pb.step(&f, -1);
        assert_eq!(pb.idx, 2);
        assert!(!pb.tracking_live);
        pb.step(&f, 1);
        assert_eq!(pb.idx, 0);
    }

    #[test]
    fn stopping_on_latest_is_live_without_tracking() {
        let f = frames(&[1, 2, 3], &[]);
        let mut pb = Playback::new();
        pb.tick(&f, 0.0);
        pb.step(&f, -1);
        pb.step(&f, 1);
        assert!(pb.is_live(&f));
        assert!(!pb.tracking_live);
    }

    #[test]
    fn new_latest_frame_jumps_only_while_tracking() {
        let f1 = frames(&[1, 2, 3], &[]);
        let f2 = frames(&[1, 2, 3, 4], &[]);
        let mut pb = Playback::new();
        pb.tick(&f1, 0.0);
        assert_eq!(pb.idx, 2);
        pb.tick(&f2, 1.0);
        assert_eq!(pb.idx, 3);

        let mut pb = Playback::new();
        pb.tick(&f1, 0.0);
        pb.scrub(&f1, 0);
        pb.tick(&f2, 1.0);
        assert_eq!(pb.idx, 0);
        pb.go_live(&f2);
        assert!(pb.tracking_live && !pb.playing);
        assert_eq!(pb.idx, 3);
    }

    #[test]
    fn idx_is_clamped_when_loop_shrinks() {
        let mut pb = Playback::new();
        pb.tick(&frames(&[1, 2, 3, 4], &[]), 0.0);
        pb.scrub(&frames(&[1, 2, 3, 4], &[]), 3);
        pb.tick(&frames(&[1, 2], &[]), 1.0);
        assert_eq!(pb.idx, 1);
    }

    #[test]
    fn layer_key_tracks_url_inputs() {
        let base = RadarSettings::default();
        let k = radar_layer_key("/v2/radar/1", &base, true);
        assert_ne!(k, radar_layer_key("/v2/radar/2", &base, true));
        let mut s = base.clone();
        s.color_scheme = base.color_scheme.wrapping_add(1);
        assert_ne!(k, radar_layer_key("/v2/radar/1", &s, true));
        let mut s = base.clone();
        s.smooth = !base.smooth;
        assert_ne!(k, radar_layer_key("/v2/radar/1", &s, true));
        let mut s = base.clone();
        s.show_arrows = !base.show_arrows;
        assert_ne!(k, radar_layer_key("/v2/radar/1", &s, true));
        let mut a = base.clone();
        a.show_arrows = true;
        assert_ne!(
            radar_layer_key("/v2/radar/1", &a, true),
            radar_layer_key("/v2/radar/1", &a, false)
        );
    }

    fn spc(code: &str, level: i32, fill: &str, stroke: &str) -> SpcOutlookFeature {
        SpcOutlookFeature {
            code: code.into(),
            name: format!("{code} name"),
            level,
            stroke: stroke.into(),
            fill: fill.into(),
            valid: String::new(),
            expire: String::new(),
            issue: String::new(),
            geometry: Geometry::Other,
        }
    }

    #[test]
    fn spc_items_parse_colors_and_sort_by_level() {
        let items = spc_items(&[
            spc("ENH", 3, "#ff7f00", "#ff0000"),
            spc("SLGT", 2, "#ffff00", "nonsense"),
        ]);
        assert_eq!(items[0].id, "spc:SLGT:2");
        assert_eq!(items[1].id, "spc:ENH:3");
        assert_eq!(
            items[1].fill,
            Color32::from_rgba_unmultiplied(255, 127, 0, 48)
        );
        assert_eq!(items[1].stroke, Color32::from_rgb(255, 0, 0));
        assert_eq!(items[0].stroke, Color32::GRAY);
        assert_eq!(
            items[0].label.as_deref(),
            Some("SPC Day 1 outlook: SLGT name")
        );
        assert!(!items[0].pulse);
    }

    #[test]
    fn bbox_request_due_respects_max_age() {
        let a = BBox::new(-100.0, 30.0, -98.0, 32.0);
        assert!(!bbox_request_due(Some((&a, 0.0)), 119.0, &a, true, 120.0));
        assert!(bbox_request_due(Some((&a, 0.0)), 120.0, &a, true, 120.0));
        assert!(bbox_request_due(None, 0.0, &a, false, 120.0));
    }

    #[test]
    fn wind_request_due_cases() {
        let a = BBox::new(-100.0, 30.0, -98.0, 32.0);
        // Nothing requested yet.
        assert!(wind_request_due(None, 0.0, &a, false));
        // Same view, recent.
        assert!(!wind_request_due(Some((&a, 0.0)), 10.0, &a, true));
        // Stale.
        assert!(wind_request_due(Some((&a, 0.0)), 300.0, &a, true));
        // Small pan.
        let small = BBox::new(-99.8, 30.0, -97.8, 32.0);
        assert!(!wind_request_due(Some((&a, 0.0)), 10.0, &small, true));
        // Big pan, settled vs not.
        let far = BBox::new(-99.0, 30.0, -97.0, 32.0);
        assert!(wind_request_due(Some((&a, 0.0)), 10.0, &far, true));
        assert!(!wind_request_due(Some((&a, 0.0)), 10.0, &far, false));
        // Zoom in by one level.
        let zoomed = BBox::new(-99.5, 30.5, -98.5, 31.5);
        assert!(wind_request_due(Some((&a, 0.0)), 10.0, &zoomed, true));
        assert!(!wind_request_due(Some((&a, 0.0)), 10.0, &zoomed, false));
        // Zoom out by one level.
        let out = BBox::new(-101.0, 29.0, -97.0, 33.0);
        assert!(wind_request_due(Some((&a, 0.0)), 10.0, &out, true));
    }

    #[test]
    fn scheme_options_falls_back_to_builtins() {
        let opts = scheme_options(None);
        assert_eq!(opts.len(), 9);
        assert!(opts.iter().any(|(id, n)| *id == 1 && n == "Original"));
    }

    #[test]
    fn scheme_options_uses_loaded_index() {
        let m = WeatherMaps {
            color_schemes: vec![ss_core::ColorScheme {
                id: 9,
                name: "Custom".into(),
            }],
            ..Default::default()
        };
        let opts = scheme_options(Some(&m));
        assert_eq!(opts, vec![(9, "Custom".to_string())]);
        let empty = WeatherMaps::default();
        assert_eq!(scheme_options(Some(&empty)).len(), 9);
    }

    #[test]
    fn layer_toggles_reflect_state_and_flip() {
        let rs = RadarSettings::default();
        let toggles = layer_toggles(&rs);
        assert_eq!(toggles.len(), 10);
        let sat = toggles.iter().find(|(l, _, _)| *l == "SAT").unwrap();
        assert!(!sat.1);
        assert_eq!(
            sat.2,
            Command::Layer {
                name: "satellite".to_string(),
                on: Some(true),
            }
        );
        let mut on = rs.clone();
        on.satellite = true;
        on.show_polygons = false;
        let toggles = layer_toggles(&on);
        let sat = toggles.iter().find(|(l, _, _)| *l == "SAT").unwrap();
        assert!(sat.1);
        assert_eq!(
            sat.2,
            Command::Layer {
                name: "satellite".to_string(),
                on: Some(false),
            }
        );
        let ply = toggles.iter().find(|(l, _, _)| *l == "PLY").unwrap();
        assert!(!ply.1);
    }
}
