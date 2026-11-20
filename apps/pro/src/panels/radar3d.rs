//! 3D RADAR: the radar volume over shaded terrain, with storm cells,
//! tornado signatures and storm reports, in an orbitable 3D view.
//!
//! Where a NEXRAD covers the place, the echoes are a measured Level II
//! volume scan (each at its true height); elsewhere they are the global
//! radar mosaic extruded to estimated heights. Terrain is global.
//!
//! Drawing is done on the CPU: a perspective camera projects terrain quads
//! and radar voxels, which are depth-sorted together and drawn back to
//! front as one mesh, so it works with any egui backend.

use std::time::{Duration, Instant};

use egui::{
    epaint::{Mesh, Vertex, WHITE_UV},
    pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui,
};
use ss_core::radar3d::{local_km, RadarSource, Scene};
use ss_core::storms::{ReportKind, StormCell, Tvs};
use ss_core::theme::Palette;
use ss_core::PanelKind;

use super::{error_text, placeholder, Panel, PanelCx};
use crate::theme::{c32, c32a, icon};

/// New NEXRAD volumes arrive every ~5 minutes.
const REFRESH: Duration = Duration::from_secs(5 * 60);
const RETRY: Duration = Duration::from_secs(2 * 60);

/// A point in scene space: km east, km north, km up (already exaggerated).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl V3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn dot(self, o: V3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        V3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn norm(self) -> V3 {
        let l = self.dot(self).sqrt().max(1e-6);
        V3::new(self.x / l, self.y / l, self.z / l)
    }
}

/// An orbit camera around a target on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Compass bearing the camera looks *toward*, degrees.
    pub yaw: f32,
    /// Angle above the horizon, degrees.
    pub pitch: f32,
    /// Distance from the target, km.
    pub dist: f32,
    pub target: V3,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: 20.0,
            pitch: 34.0,
            dist: 440.0,
            target: V3::new(0.0, 0.0, 0.0),
        }
    }
}

/// A camera fixed for one frame: projects scene points to the screen.
pub struct View {
    eye: V3,
    right: V3,
    up: V3,
    fwd: V3,
    focal: f32,
    center: Pos2,
}

impl View {
    pub fn new(cam: &Camera, rect: Rect) -> View {
        let (yaw, pitch) = (cam.yaw.to_radians(), cam.pitch.to_radians());
        // Looking toward `yaw`: the eye sits behind the target.
        let back = V3::new(
            -pitch.cos() * yaw.sin(),
            -pitch.cos() * yaw.cos(),
            pitch.sin(),
        );
        let eye = V3::new(
            cam.target.x + back.x * cam.dist,
            cam.target.y + back.y * cam.dist,
            cam.target.z + back.z * cam.dist,
        );
        let fwd = cam.target.sub(eye).norm();
        let right = fwd.cross(V3::new(0.0, 0.0, 1.0)).norm();
        let up = right.cross(fwd);
        let fov = 45f32.to_radians();
        View {
            eye,
            right,
            up,
            fwd,
            focal: rect.height() * 0.5 / (fov / 2.0).tan(),
            center: rect.center(),
        }
    }

    /// Screen position and depth (km in front of the eye), `None` behind.
    pub fn project(&self, p: V3) -> Option<(Pos2, f32)> {
        let d = p.sub(self.eye);
        let z = d.dot(self.fwd);
        // Near plane 20 m in front of the eye (close-up terrain).
        if z < 0.02 {
            return None;
        }
        let x = d.dot(self.right) / z * self.focal;
        let y = d.dot(self.up) / z * self.focal;
        Some((pos2(self.center.x + x, self.center.y - y), z))
    }

    /// Pixels per km at a depth.
    pub fn scale_at(&self, depth: f32) -> f32 {
        self.focal / depth.max(0.02)
    }

    pub fn eye(&self) -> V3 {
        self.eye
    }
}

/// Reflectivity colours (NWS-like), with alpha growing with intensity so
/// light rain stays see-through and cores read solid.
pub fn dbz_color(dbz: f32) -> Color32 {
    let stops: [(f32, [u8; 3]); 10] = [
        (10.0, [0x40, 0x9c, 0xd8]),
        (20.0, [0x2e, 0xb8, 0x4a]),
        (30.0, [0x1f, 0x8f, 0x2a]),
        (35.0, [0xd8, 0xd8, 0x20]),
        (40.0, [0xf0, 0xb4, 0x00]),
        (45.0, [0xf0, 0x78, 0x00]),
        (50.0, [0xe8, 0x14, 0x14]),
        (55.0, [0xb0, 0x00, 0x00]),
        (60.0, [0xe0, 0x20, 0xe0]),
        (68.0, [0xff, 0xff, 0xff]),
    ];
    let mut c = stops[0].1;
    for w in stops.windows(2) {
        if dbz >= w[0].0 && dbz < w[1].0 {
            let t = (dbz - w[0].0) / (w[1].0 - w[0].0);
            let l = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8;
            c = [
                l(w[0].1[0], w[1].1[0]),
                l(w[0].1[1], w[1].1[1]),
                l(w[0].1[2], w[1].1[2]),
            ];
            break;
        }
        if dbz >= w[1].0 {
            c = w[1].1;
        }
    }
    let a = (70.0 + (dbz - 10.0).clamp(0.0, 50.0) * 3.6) as u8;
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
}

/// Terrain colour by height (m), lit by `shade` (0..1).
pub fn terrain_color(h: f32, shade: f32, dark: bool) -> Color32 {
    let base: [f32; 3] = if h <= 0.5 {
        [0.10, 0.20, 0.32]
    } else if h < 300.0 {
        [0.28, 0.42, 0.25]
    } else if h < 1000.0 {
        let t = (h - 300.0) / 700.0;
        [0.28 + 0.22 * t, 0.42 - 0.04 * t, 0.25 - 0.02 * t]
    } else if h < 2500.0 {
        let t = (h - 1000.0) / 1500.0;
        [0.50 - 0.08 * t, 0.38 - 0.08 * t, 0.23 + 0.02 * t]
    } else {
        let t = ((h - 2500.0) / 1500.0).min(1.0);
        [0.42 + 0.48 * t, 0.30 + 0.60 * t, 0.25 + 0.65 * t]
    };
    let k = if dark { 0.85 } else { 1.0 } * (0.35 + 0.75 * shade);
    let c = |v: f32| ((v * k).clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgb(c(base[0]), c(base[1]), c(base[2]))
}

/// A sky behind the scene: deep blue overhead fading to a pale horizon.
fn sky(painter: &egui::Painter, rect: Rect, dark: bool) {
    let (top, bottom) = if dark {
        (Color32::from_rgb(6, 12, 28), Color32::from_rgb(58, 82, 112))
    } else {
        (
            Color32::from_rgb(70, 120, 190),
            Color32::from_rgb(196, 214, 232),
        )
    };
    let mut m = Mesh::default();
    for (pos, col) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), bottom),
    ] {
        m.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color: col,
        });
    }
    m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(Shape::mesh(m));
}

/// A soft round sprite: white, alpha falling smoothly to 0 at the edge, so
/// overlapping echoes blend into continuous volumes instead of squares.
pub fn soft_sprite(size: usize) -> egui::ColorImage {
    let c = (size as f32 - 1.0) / 2.0;
    let pixels = (0..size * size)
        .map(|i| {
            let (x, y) = ((i % size) as f32, (i / size) as f32);
            let r = (((x - c).powi(2) + (y - c).powi(2)).sqrt() / c).min(1.0);
            let a = (1.0 - r * r).powf(1.6);
            Color32::from_white_alpha((a * 255.0) as u8)
        })
        .collect();
    egui::ColorImage::new([size, size], pixels)
}

/// The terrain as one mesh: every height sample a vertex (projected, with
/// its own lighting, so shading is smooth), quads sorted far to near. With
/// `imagery`, the satellite image is draped over it (lit by the relief);
/// otherwise it is coloured by height.
fn terrain_mesh(
    t: &ss_core::radar3d::Terrain,
    view: &View,
    ex: f32,
    dark: bool,
    imagery: Option<egui::TextureId>,
) -> Mesh {
    let n = t.n;
    let half = (n as f32 - 1.0) * t.cell_km / 2.0;
    let h = |c: usize, r: usize| t.heights_m[r.min(n - 1) * n + c.min(n - 1)] / 1000.0;
    let light = V3::new(-0.5, 0.6, 0.62).norm();
    let mut mesh = Mesh::default();
    let mut depth = vec![f32::NAN; n * n];
    for r in 0..n {
        for c in 0..n {
            let p = V3::new(
                -half + c as f32 * t.cell_km,
                half - r as f32 * t.cell_km,
                h(c, r) * ex,
            );
            // Normal from central differences (slopes a little softened).
            let dx = (h(c + 1, r) - h(c.saturating_sub(1), r)) * ex * 0.6;
            let dy = (h(c, r.saturating_sub(1)) - h(c, r + 1)) * ex * 0.6;
            let nrm = V3::new(-dx, -dy, 2.0 * t.cell_km).norm();
            let shade = nrm.dot(light).max(0.0);
            let (col, uv) = match imagery {
                // Daylight on the photo: flat ground at full brightness,
                // slopes facing away from the sun darker.
                Some(_) => {
                    let k = (0.35 + 0.75 * shade / light.z).clamp(0.3, 1.0);
                    let g = (k * 255.0) as u8;
                    (
                        Color32::from_rgb(g, g, g),
                        pos2(c as f32 / (n - 1) as f32, r as f32 / (n - 1) as f32),
                    )
                }
                None => (terrain_color(t.heights_m[r * n + c], shade, dark), WHITE_UV),
            };
            let (pos, d) = match view.project(p) {
                Some((pos, d)) => (pos, d),
                None => (Pos2::ZERO, f32::NAN),
            };
            depth[r * n + c] = d;
            mesh.vertices.push(Vertex {
                pos,
                uv,
                color: col,
            });
        }
    }
    if let Some(tex) = imagery {
        mesh.texture_id = tex;
    }
    let mut quads: Vec<(f32, u32)> = Vec::with_capacity((n - 1) * (n - 1));
    for r in 0..n - 1 {
        for c in 0..n - 1 {
            let i = r * n + c;
            let ds = [depth[i], depth[i + 1], depth[i + n], depth[i + n + 1]];
            if ds.iter().any(|d| d.is_nan()) {
                continue;
            }
            quads.push((ds.iter().sum::<f32>(), i as u32));
        }
    }
    quads.sort_by(|a, b| b.0.total_cmp(&a.0));
    let n32 = n as u32;
    mesh.indices.reserve(quads.len() * 6);
    for (_, i) in quads {
        mesh.indices
            .extend_from_slice(&[i, i + 1, i + n32 + 1, i, i + n32 + 1, i + n32]);
    }
    mesh
}

/// The echoes as soft sprites, far to near, sized to overlap their
/// neighbours so the volume reads as continuous cloud.
fn voxel_mesh(scene: &Scene, view: &View, ex: f32, min_dbz: f32, texture: egui::TextureId) -> Mesh {
    let mut pts: Vec<(f32, Pos2, f32, Color32)> = scene
        .voxels
        .iter()
        .filter(|v| v.dbz >= min_dbz)
        .filter_map(|v| {
            let (at, depth) = view.project(V3::new(v.x, v.y, v.z * ex))?;
            let size = (scene.voxel_km * view.scale_at(depth) * 2.3).clamp(2.0, 70.0);
            Some((depth, at, size, dbz_color(v.dbz).gamma_multiply(0.6)))
        })
        .collect();
    pts.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut mesh = Mesh::with_texture(texture);
    mesh.vertices.reserve(pts.len() * 4);
    mesh.indices.reserve(pts.len() * 6);
    for (_, c, s, col) in pts {
        let h = s / 2.0;
        let i = mesh.vertices.len() as u32;
        for (d, uv) in [
            (vec2(-h, -h), pos2(0.0, 0.0)),
            (vec2(h, -h), pos2(1.0, 0.0)),
            (vec2(h, h), pos2(1.0, 1.0)),
            (vec2(-h, h), pos2(0.0, 1.0)),
        ] {
            mesh.vertices.push(Vertex {
                pos: c + d,
                uv,
                color: col,
            });
        }
        mesh.indices
            .extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    }
    mesh
}

/// A marker to draw on top (storm cells, reports), with hover text.
struct Mark {
    at: Pos2,
    text: String,
}

pub struct Radar3dPanel {
    cam: Camera,
    /// Vertical exaggeration.
    exag: f32,
    min_dbz: f32,
    show_terrain: bool,
    show_radar: bool,
    show_storms: bool,
    spin: bool,
    requested: std::collections::HashMap<String, Instant>,
    /// Soft round sprite the echoes are drawn with.
    sprite: Option<egui::TextureHandle>,
    /// Drape satellite imagery over the terrain (else colour by height).
    show_imagery: bool,
    /// The uploaded imagery and the scene it belongs to.
    imagery_tex: Option<(String, egui::TextureHandle)>,
    /// Level-of-detail terrain and imagery, for zooming in close.
    lod: Option<super::terrain_lod::Lod>,
}

impl Radar3dPanel {
    /// The vertical exaggeration in use: the slider's value at the overview,
    /// easing to true scale as you zoom in to the ground (like a map).
    fn effective_exag(&self) -> f32 {
        let t = ((self.cam.dist - 15.0) / (200.0 - 15.0)).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        1.0 + (self.exag - 1.0) * t
    }

    pub fn new(
        client: reqwest::Client,
        rt: tokio::runtime::Handle,
        tile_dir: Option<std::path::PathBuf>,
        ctx: egui::Context,
    ) -> Self {
        Self {
            lod: Some(super::terrain_lod::Lod::new(client, rt, tile_dir, ctx)),
            ..Self::default()
        }
    }
}

impl Default for Radar3dPanel {
    fn default() -> Self {
        Self {
            cam: Camera::default(),
            exag: 6.0,
            min_dbz: 18.0,
            show_terrain: true,
            show_radar: true,
            show_storms: true,
            spin: false,
            requested: Default::default(),
            sprite: None,
            show_imagery: true,
            imagery_tex: None,
            lod: None,
        }
    }
}

fn threat_color(t: u8) -> Color32 {
    match t {
        4 => Color32::from_rgb(0xff, 0x2a, 0x2a),
        3 => Color32::from_rgb(0xff, 0x40, 0xff),
        2 => Color32::from_rgb(0xff, 0x9f, 0x1c),
        1 => Color32::from_rgb(0xff, 0xd4, 0x00),
        _ => Color32::from_gray(200),
    }
}

impl Radar3dPanel {
    fn toggle(ui: &mut Ui, p: &Palette, on: &mut bool, glyph: &str, tip: &str) {
        let color = if *on { c32(p.a0) } else { c32a(p.text3) };
        let mut b = egui::Button::new(RichText::new(glyph).font(icon(16.0)).color(color))
            .min_size(vec2(26.0, 22.0));
        if *on {
            b = b
                .fill(Color32::from_rgba_unmultiplied(
                    p.a0[0], p.a0[1], p.a0[2], 40,
                ))
                .stroke(Stroke::new(1.0, c32(p.a0)));
        }
        if ui.add(b).on_hover_text(tip).clicked() {
            *on = !*on;
        }
    }

    fn controls(&mut self, ui: &mut Ui, p: &Palette, scene: Option<&Scene>, cx: &mut PanelCx) {
        use ss_assets::symbols as i;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            Self::toggle(ui, p, &mut self.show_terrain, i::LANDSCAPE, "Terrain");
            Self::toggle(
                ui,
                p,
                &mut self.show_imagery,
                i::SATELLITE_ALT,
                "Satellite imagery on the terrain (off: colour by height)",
            );
            Self::toggle(ui, p, &mut self.show_radar, i::RADAR, "Radar echoes");
            Self::toggle(
                ui,
                p,
                &mut self.show_storms,
                i::THUNDERSTORM,
                "Storm cells, tornado signatures and reports",
            );
            Self::toggle(ui, p, &mut self.spin, i::ROTATION_3D, "Rotate slowly");
            if ui
                .add(
                    egui::Button::new(RichText::new(i::RESTART_ALT).font(icon(16.0)))
                        .min_size(vec2(26.0, 22.0)),
                )
                .on_hover_text("Reset the view (or double-click it)")
                .clicked()
            {
                self.cam = Camera::default();
            }
            if ui
                .add(
                    egui::Button::new(RichText::new(i::REFRESH).font(icon(16.0)))
                        .min_size(vec2(26.0, 22.0)),
                )
                .on_hover_text("Load the newest scan now")
                .clicked()
            {
                if let Some(l) = &cx.location {
                    self.requested.remove(&l.id);
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new("≥").monospace().color(c32a(p.text3)));
            ui.spacing_mut().slider_width = 70.0;
            ui.add(
                egui::Slider::new(&mut self.min_dbz, 10.0..=50.0)
                    .step_by(5.0)
                    .suffix(" dBZ"),
            )
            .on_hover_text("Hide echoes weaker than this");
            ui.label(RichText::new("height ×").monospace().color(c32a(p.text3)));
            ui.add(egui::Slider::new(&mut self.exag, 1.0..=10.0).step_by(0.5))
                .on_hover_text(
                    "Vertical exaggeration at the overview; it eases to true scale as you zoom in",
                );
        });
        if let Some(s) = scene {
            let (text, warn) = match &s.source {
                Some(RadarSource::Nexrad { site, time }) => (
                    format!("{site} volume scan · {} · measured heights", short_time(time)),
                    false,
                ),
                Some(RadarSource::Mosaic { time }) => (
                    format!(
                        "global radar mosaic · {} · heights estimated from intensity (no NEXRAD here)",
                        short_time(time)
                    ),
                    true,
                ),
                None => ("terrain only (no radar data)".into(), true),
            };
            ui.label(RichText::new(text).monospace().size(11.0).color(if warn {
                c32a(p.text3)
            } else {
                c32(p.a0)
            }));
        }
    }
}

fn short_time(rfc: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc)
        .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

impl Panel for Radar3dPanel {
    fn kind(&self) -> PanelKind {
        PanelKind::Radar3d
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut PanelCx) {
        let p = cx.palette;
        let Some(loc) = cx.location.clone() else {
            placeholder(ui, &p, "no location");
            return;
        };
        // Fetch: once the radar index is loaded (and, in range of the US
        // network, the site list), then every few minutes.
        let sh = &mut *cx.shared;
        let ready = sh.weather_maps.is_some();
        let failed = sh.scene3d_errors.contains_key(&loc.id);
        let due = self
            .requested
            .get(&loc.id)
            .is_none_or(|t| t.elapsed() >= REFRESH || (failed && t.elapsed() >= RETRY));
        if ready && due && !sh.scene3d_pending.contains(&loc.id) {
            self.requested.insert(loc.id.clone(), Instant::now());
            sh.scene3d_pending.insert(loc.id.clone());
            let frames = sh
                .weather_maps
                .as_ref()
                .map(|m| m.radar_past.clone())
                .unwrap_or_default();
            cx.tasks.scene3d(
                &loc,
                sh.radar_stations.clone(),
                sh.config.libre_wxr_host.clone(),
                frames,
            );
        }
        let scene = cx.shared.scenes3d.get(&loc.id).cloned();
        self.controls(ui, &p, scene.as_ref(), cx);
        let Some(scene) = scene else {
            if let Some(e) = cx.shared.scene3d_errors.get(&loc.id) {
                error_text(ui, &p, &format!("3D view failed: {e}"));
            } else {
                placeholder(ui, &p, "loading terrain and radar volume…");
            }
            return;
        };
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        self.interact(ui, &resp);
        let dark = crate::theme::is_dark(&p);
        let painter = ui.painter_at(rect);

        if self.sprite.is_none() {
            self.sprite = Some(ui.ctx().load_texture(
                "radar3d-sprite",
                soft_sprite(64),
                egui::TextureOptions::LINEAR,
            ));
        }
        if let Some(img) = &scene.imagery {
            let key = format!("{}:{:?}:{}", loc.id, scene.source, img.rgb.len());
            if self.imagery_tex.as_ref().is_none_or(|(k, _)| *k != key) {
                let ci = egui::ColorImage::from_rgb([img.size, img.size], &img.rgb);
                let tex =
                    ui.ctx()
                        .load_texture("radar3d-imagery", ci, egui::TextureOptions::LINEAR);
                self.imagery_tex = Some((key, tex));
            }
        }
        sky(&painter, rect, dark);
        // Orbit around the ground under the target (so close-ups over
        // mountains don't put the camera inside them).
        let (tx, ty) = (self.cam.target.x, self.cam.target.y);
        let g = match self.lod.as_mut() {
            Some(lod) => lod.ground(&scene, tx, ty),
            None => scene
                .terrain
                .as_ref()
                .map_or(0.0, |t| t.height_at(tx, ty) / 1000.0),
        };
        let ex = self.effective_exag();
        self.cam.target.z = g * ex;
        let view = View::new(&self.cam, rect);
        let mut lod_drawn = false;
        if let (true, Some(lod)) = (self.show_terrain, self.lod.as_mut()) {
            lod.poll(ui.ctx());
            let base = self
                .imagery_tex
                .as_ref()
                .filter(|_| scene.imagery.is_some())
                .map(|(_, t)| t.id());
            lod.draw(&painter, &view, &scene, ex, dark, self.show_imagery, base);
            lod.draw_buildings(
                &painter,
                &view,
                &scene,
                ex,
                (tx, ty),
                dark,
                self.show_imagery,
            );
            lod_drawn = true;
            if lod.busy() {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(150));
            }
        }
        let marks = self.draw_scene(&painter, &view, &scene, cx, dark, lod_drawn);
        self.overlay(&painter, rect, &view, &p);
        // Hover: the nearest marker.
        if let Some(pos) = resp.hover_pos() {
            if let Some(m) = marks
                .iter()
                .filter(|m| (m.at - pos).length() < 14.0)
                .min_by(|a, b| (a.at - pos).length().total_cmp(&(b.at - pos).length()))
            {
                resp.clone().on_hover_text_at_pointer(&m.text);
            }
        }
        if self.spin || resp.dragged() {
            ui.ctx().request_repaint();
        }
    }
}

impl Radar3dPanel {
    fn interact(&mut self, ui: &Ui, resp: &egui::Response) {
        if self.spin {
            self.cam.yaw = (self.cam.yaw + ui.input(|i| i.stable_dt) * 6.0).rem_euclid(360.0);
        }
        if resp.double_clicked() {
            self.cam = Camera::default();
        }
        let shift = ui.input(|i| i.modifiers.shift);
        if resp.dragged_by(egui::PointerButton::Primary) && !shift {
            let d = resp.drag_delta();
            self.cam.yaw = (self.cam.yaw - d.x * 0.35).rem_euclid(360.0);
            self.cam.pitch = (self.cam.pitch + d.y * 0.25).clamp(5.0, 88.0);
        } else if resp.dragged_by(egui::PointerButton::Secondary)
            || (resp.dragged_by(egui::PointerButton::Primary) && shift)
        {
            // Pan along the ground, in the camera's frame: the ground follows
            // the pointer (right = camera right, down = toward the camera).
            let d = resp.drag_delta() * self.cam.dist / 600.0;
            let (s, c) = self.cam.yaw.to_radians().sin_cos();
            let (right, fwd) = ((c, -s), (s, c));
            self.cam.target.x += -right.0 * d.x + fwd.0 * d.y;
            self.cam.target.y += -right.1 * d.x + fwd.1 * d.y;
            self.cam.target.x = self.cam.target.x.clamp(-250.0, 250.0);
            self.cam.target.y = self.cam.target.y.clamp(-250.0, 250.0);
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.cam.dist = (self.cam.dist * (-scroll * 0.0015).exp()).clamp(0.3, 1400.0);
            }
        }
    }

    /// Draw terrain and radar, then storm markers. Returns hover targets.
    fn draw_scene(
        &self,
        painter: &egui::Painter,
        view: &View,
        scene: &Scene,
        cx: &PanelCx,
        dark: bool,
        terrain_drawn: bool,
    ) -> Vec<Mark> {
        let ex = self.effective_exag();
        let ground = |x: f32, y: f32| -> f32 {
            scene
                .terrain
                .as_ref()
                .map_or(0.0, |t| t.height_at(x, y) / 1000.0)
        };
        if let (true, false, Some(t)) = (self.show_terrain, terrain_drawn, &scene.terrain) {
            let photo = self
                .imagery_tex
                .as_ref()
                .filter(|_| self.show_imagery && scene.imagery.is_some())
                .map(|(_, tex)| tex.id());
            painter.add(Shape::mesh(terrain_mesh(t, view, ex, dark, photo)));
        }
        if let (true, Some(sprite)) = (self.show_radar, &self.sprite) {
            painter.add(Shape::mesh(voxel_mesh(
                scene,
                view,
                ex,
                self.min_dbz,
                sprite.id(),
            )));
        }

        let mut marks = Vec::new();
        let font = FontId::monospace(11.0);
        // The place.
        if let Some((at, _)) = view.project(V3::new(0.0, 0.0, ground(0.0, 0.0) * ex)) {
            painter.circle_filled(at, 4.0, Color32::WHITE);
            painter.circle_stroke(at, 7.0, Stroke::new(1.5, Color32::WHITE));
            marks.push(Mark {
                at,
                text: cx
                    .location
                    .as_ref()
                    .map(|l| l.label.clone())
                    .unwrap_or_default(),
            });
        }
        // The radar, with range rings on the ground.
        if let Some((rx, ry)) = scene.radar_xy {
            for ring in [50.0f32, 100.0, 150.0, 200.0] {
                let pts: Vec<Pos2> = (0..=72)
                    .filter_map(|k| {
                        let a = (k as f32 * 5.0).to_radians();
                        let (x, y) = (rx + ring * a.sin(), ry + ring * a.cos());
                        if x.abs() > scene.half_km || y.abs() > scene.half_km {
                            return None;
                        }
                        view.project(V3::new(x, y, ground(x, y) * ex + 0.05))
                            .map(|q| q.0)
                    })
                    .collect();
                for w in pts.windows(2) {
                    if (w[1] - w[0]).length() < 80.0 {
                        painter.line_segment(
                            [w[0], w[1]],
                            Stroke::new(1.0, Color32::from_white_alpha(70)),
                        );
                    }
                }
            }
            if let Some((at, _)) = view.project(V3::new(rx, ry, ground(rx, ry) * ex)) {
                painter.circle_filled(at, 3.5, Color32::from_rgb(0x2e, 0xd5, 0x73));
                if let Some(RadarSource::Nexrad { site, .. }) = &scene.source {
                    painter.text(
                        at + vec2(6.0, 0.0),
                        Align2::LEFT_CENTER,
                        site,
                        font.clone(),
                        Color32::WHITE,
                    );
                    marks.push(Mark {
                        at,
                        text: format!("{site} (NEXRAD radar)"),
                    });
                }
            }
        }
        if self.show_storms {
            self.draw_storms(painter, view, scene, cx, &ground, &mut marks);
        }
        marks
    }

    fn draw_storms(
        &self,
        painter: &egui::Painter,
        view: &View,
        scene: &Scene,
        cx: &PanelCx,
        ground: &dyn Fn(f32, f32) -> f32,
        marks: &mut Vec<Mark>,
    ) {
        let ex = self.effective_exag();
        let sh = &*cx.shared;
        let inside = |x: f32, y: f32| x.abs() <= scene.half_km && y.abs() <= scene.half_km;
        let mosaic_here = sh.mosaic_for.as_deref() == cx.location.as_ref().map(|l| l.id.as_str());
        let cells: Vec<&StormCell> = sh
            .storm_cells
            .iter()
            .chain(sh.mosaic_cells.iter().filter(|_| mosaic_here))
            .collect();
        for c in cells {
            let (x, y) = local_km(scene.lat, scene.lon, c.lat, c.lon);
            if !inside(x, y) {
                continue;
            }
            let g = ground(x, y);
            let top = if c.top_kft > 0.0 {
                (c.top_kft * 0.3048) as f32
            } else {
                ss_core::radar3d::estimated_top_km(c.max_dbz as f32)
            };
            let (Some((base, _)), Some((tip, _))) = (
                view.project(V3::new(x, y, g * ex)),
                view.project(V3::new(x, y, (g + top) * ex)),
            ) else {
                continue;
            };
            let t = c.threat();
            let col = threat_color(t);
            if t == 0 && c.max_dbz < 45.0 {
                continue;
            }
            painter.line_segment([base, tip], Stroke::new(1.5, col.gamma_multiply(0.8)));
            painter.circle_filled(tip, if t >= 2 { 4.5 } else { 3.0 }, col);
            if c.tvs != Tvs::None {
                // A funnel from cloud base (~1.5 km) to the ground.
                let cb = view.project(V3::new(x, y, (g + 1.5) * ex));
                if let Some((cb, _)) = cb {
                    let w = 9.0;
                    painter.add(Shape::convex_polygon(
                        vec![cb + vec2(-w, 0.0), cb + vec2(w, 0.0), base],
                        Color32::from_rgba_unmultiplied(0xff, 0x2a, 0x2a, 170),
                        Stroke::new(1.5, Color32::from_rgb(0xff, 0x2a, 0x2a)),
                    ));
                }
            }
            if c.meso > 0 {
                if let Some((mid, _)) = view.project(V3::new(x, y, (g + 4.0) * ex)) {
                    painter.circle_stroke(mid, 8.0, Stroke::new(2.0, threat_color(3)));
                }
            }
            let kind = if c.tvs == Tvs::Tvs {
                "TORNADO VORTEX SIGNATURE"
            } else if c.meso > 0 {
                "MESOCYCLONE"
            } else {
                "STORM CELL"
            };
            marks.push(Mark {
                at: tip,
                text: format!(
                    "{kind} {}{} · {} · top {:.1} km",
                    c.radar,
                    c.id,
                    c.summary(),
                    top
                ),
            });
        }
        // Tornado, funnel-cloud and hail reports (US): pins on the ground.
        // (Wind and flood reports are left to the 2D map: too many to read.)
        for r in sh.storm_reports.iter().filter(|r| {
            matches!(
                r.kind,
                ReportKind::Tornado | ReportKind::FunnelCloud | ReportKind::Hail
            )
        }) {
            let (x, y) = local_km(scene.lat, scene.lon, r.lat, r.lon);
            if !inside(x, y) {
                continue;
            }
            let g = ground(x, y);
            let (Some((base, _)), Some((head, _))) = (
                view.project(V3::new(x, y, g * ex)),
                view.project(V3::new(x, y, (g + 1.2) * ex)),
            ) else {
                continue;
            };
            let col = c32(r.kind.color());
            painter.line_segment([base, head], Stroke::new(1.2, col));
            let rad = if r.kind == ReportKind::Tornado {
                6.0
            } else {
                4.5
            };
            painter.circle_filled(head, rad, col);
            painter.text(
                head,
                Align2::CENTER_CENTER,
                r.kind.letter(),
                FontId::monospace(8.5),
                Color32::from_gray(15),
            );
            marks.push(Mark {
                at: head,
                text: format!(
                    "{} report · {} {} · {}",
                    r.kind.label(),
                    r.place,
                    r.state,
                    r.remark
                ),
            });
        }
    }

    /// Compass, legend and hints.
    fn overlay(&self, painter: &egui::Painter, rect: Rect, view: &View, p: &Palette) {
        // North arrow: project a short northward step from the target.
        let t = self.cam.target;
        if let (Some((a, _)), Some((b, _))) =
            (view.project(t), view.project(V3::new(t.x, t.y + 50.0, t.z)))
        {
            let dir = (b - a).normalized();
            let c = rect.right_top() + vec2(-26.0, 26.0);
            painter.circle_filled(c, 15.0, Color32::from_black_alpha(140));
            painter.arrow(c - dir * 9.0, dir * 18.0, Stroke::new(2.0, c32(p.a0)));
            painter.text(
                c + dir * 21.0,
                Align2::CENTER_CENTER,
                "N",
                FontId::monospace(11.0),
                Color32::WHITE,
            );
        }
        // dBZ legend.
        let x0 = rect.left() + 10.0;
        let y = rect.bottom() - 16.0;
        let w = 14.0;
        for (k, d) in (10..=65).step_by(5).enumerate() {
            let r = Rect::from_min_size(pos2(x0 + k as f32 * w, y), vec2(w, 7.0));
            let c = dbz_color(d as f32);
            painter.rect_filled(r, 0.0, Color32::from_rgb(c.r(), c.g(), c.b()));
            if d % 20 == 10 || d == 65 {
                painter.text(
                    r.left_top() - vec2(0.0, 2.0),
                    Align2::LEFT_BOTTOM,
                    d.to_string(),
                    FontId::monospace(9.5),
                    Color32::WHITE,
                );
            }
        }
        let mut credit = if self.show_imagery && self.imagery_tex.is_some() {
            "Imagery: Esri, Maxar, Earthstar Geographics · Terrain: AWS Terrain Tiles · "
                .to_string()
        } else {
            "Terrain: AWS Terrain Tiles · ".to_string()
        };
        if self.lod.as_ref().is_some_and(|l| l.buildings > 0) {
            credit.push_str("Buildings: © OpenStreetMap, OpenFreeMap · ");
        }
        painter.text(
            rect.right_bottom() + vec2(-8.0, -6.0),
            Align2::RIGHT_BOTTOM,
            format!("{credit}drag: rotate · shift/right-drag: pan · scroll: zoom"),
            FontId::monospace(9.5),
            Color32::from_white_alpha(140),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_projects_the_target_to_the_centre() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let cam = Camera::default();
        let v = View::new(&cam, rect);
        let (at, depth) = v.project(cam.target).unwrap();
        assert!((at - rect.center()).length() < 0.5, "{at:?}");
        assert!((depth - cam.dist).abs() < 0.5);
        // Higher points appear higher on screen.
        let (up, _) = v.project(V3::new(0.0, 0.0, 50.0)).unwrap();
        assert!(up.y < at.y);
        // Points behind the eye are culled.
        let behind = V3::new(
            -cam.dist * 3.0 * cam.yaw.to_radians().sin(),
            -cam.dist * 3.0 * cam.yaw.to_radians().cos(),
            0.0,
        );
        assert!(v.project(behind).is_none());
    }

    #[test]
    fn looking_north_puts_east_on_the_right() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let cam = Camera {
            yaw: 0.0,
            ..Camera::default()
        };
        let v = View::new(&cam, rect);
        let (e, _) = v.project(V3::new(50.0, 0.0, 0.0)).unwrap();
        assert!(e.x > rect.center().x);
        // Farther north is farther away (smaller scale) and higher up the screen.
        let (n, dn) = v.project(V3::new(0.0, 100.0, 0.0)).unwrap();
        let (_, d0) = v.project(V3::new(0.0, 0.0, 0.0)).unwrap();
        assert!(dn > d0 && n.y < rect.center().y);
    }

    #[test]
    fn sprite_is_opaque_in_the_middle_and_clear_at_the_edge() {
        let img = soft_sprite(32);
        assert!(img.pixels[16 * 32 + 16].a() > 240);
        assert_eq!(img.pixels[0].a(), 0);
    }

    #[test]
    fn reflectivity_colours_get_stronger() {
        let weak = dbz_color(15.0);
        let core = dbz_color(55.0);
        assert!(core.a() > weak.a());
        assert_eq!(
            dbz_color(80.0).to_srgba_unmultiplied()[..3],
            [255, 255, 255],
            "extreme cores are white"
        );
    }
}
