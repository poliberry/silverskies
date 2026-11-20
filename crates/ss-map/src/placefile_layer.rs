//! Draws GRLevelX placefiles (see `ss_core::placefile`): lines, filled
//! polygons, text and icons from icon sheets (rotated about their hot
//! spot), each item only when the map is zoomed in past its threshold.

use std::any::Any;
use std::sync::Arc;

use egui::{pos2, vec2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, TextureHandle};
use ss_core::placefile::{Kind, Placefile, Pt};

use crate::layers::{DrawCtx, Layer};
use crate::viewport::Viewport;

/// An icon sheet's texture and size in pixels.
pub struct Sheet {
    pub tex: TextureHandle,
    pub size: [usize; 2],
    /// Draw in the item's colour (a white-on-black sheet).
    pub tint: bool,
}

/// One loaded placefile.
pub struct LoadedPlacefile {
    pub pf: Arc<Placefile>,
    /// By icon file number.
    pub sheets: Vec<(u8, Sheet)>,
}

pub struct PlacefileLayer {
    pub name: String,
    pub visible: bool,
    pub files: Vec<LoadedPlacefile>,
}

impl PlacefileLayer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            visible: true,
            files: Vec::new(),
        }
    }

    /// The hover text of what's under `pos`, if anything has one.
    pub fn hit_test(&self, pos: Pos2, vp: &Viewport, rect: Rect) -> Option<String> {
        if !self.visible {
            return None;
        }
        let range = half_width_nm(vp, rect);
        let mut best: Option<(f32, String)> = None;
        let mut offer = |d: f32, s: &str| {
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, s.to_string()));
            }
        };
        for f in &self.files {
            for it in f.pf.items.iter().filter(|it| it.threshold_nm >= range) {
                match &it.kind {
                    Kind::Icon {
                        at,
                        file,
                        hover: Some(h),
                        ..
                    } => {
                        let p = screen(vp, rect, at);
                        let r =
                            f.pf.icon_files
                                .iter()
                                .find(|i| i.number == *file)
                                .map_or(8.0, |i| (i.w.max(i.h) as f32 / 2.0).max(6.0));
                        let d = p.distance(pos);
                        if d <= r {
                            offer(d, h);
                        }
                    }
                    Kind::Text {
                        at, hover: Some(h), ..
                    } => {
                        let d = screen(vp, rect, at).distance(pos);
                        if d <= 10.0 {
                            offer(d, h);
                        }
                    }
                    Kind::Line {
                        label: Some(l),
                        pts,
                        ..
                    } => {
                        let s: Vec<Pos2> = pts.iter().map(|p| screen(vp, rect, p)).collect();
                        for w in s.windows(2) {
                            let d = seg_dist(pos, w[0], w[1]);
                            if d <= 5.0 {
                                offer(d + 2.0, l);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        best.map(|(_, s)| s)
    }
}

/// How far the map reaches from its centre to its side edge, nautical miles.
pub fn half_width_nm(vp: &Viewport, rect: Rect) -> f32 {
    let (lon0, lat0) = vp.lonlat_from_screen(rect.center(), rect);
    let (lon1, lat1) = vp.lonlat_from_screen(pos2(rect.right(), rect.center().y), rect);
    (ss_core::geo::haversine_km(lat0, lon0, lat1, lon1) / 1.852) as f32
}

fn screen(vp: &Viewport, rect: Rect, p: &Pt) -> Pos2 {
    let (lat, lon) = p.latlon();
    let (dx, dy) = p.pixels();
    vp.screen_from_lonlat(lon, lat, rect) + vec2(dx, dy)
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 {
        ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// Triangulate a simple polygon (ear clipping); indices into `pts`.
pub fn triangulate(pts: &[Pos2]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    let mut idx: Vec<usize> = (0..n).collect();
    if area < 0.0 {
        idx.reverse();
    }
    let cross = |o: Pos2, a: Pos2, b: Pos2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let inside = |p: Pos2, a: Pos2, b: Pos2, c: Pos2| {
        cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
    };
    let mut out = Vec::new();
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            if cross(a, b, c) <= 0.0 {
                continue; // reflex
            }
            let ear = idx
                .iter()
                .filter(|&&j| j != ia && j != ib && j != ic)
                .all(|&j| !inside(pts[j], a, b, c));
            if ear {
                out.push([ia, ib, ic]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break; // degenerate: give up on the rest
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

impl Layer for PlacefileLayer {
    fn name(&self) -> &str {
        &self.name
    }
    fn visible(&self) -> bool {
        self.visible
    }
    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn draw(&mut self, ctx: &mut DrawCtx, painter: &Painter) {
        let (vp, rect) = (ctx.viewport, ctx.rect);
        let range = half_width_nm(vp, rect);
        let cull = rect.expand(64.0);
        for f in &self.files {
            for it in f.pf.items.iter().filter(|it| it.threshold_nm >= range) {
                let color = c32(it.color);
                match &it.kind {
                    Kind::Line { width, pts, .. } => {
                        let s: Vec<Pos2> = pts.iter().map(|p| screen(vp, rect, p)).collect();
                        if s.iter().any(|p| cull.contains(*p)) {
                            painter.add(Shape::line(s, Stroke::new(width.max(1.0), color)));
                        }
                    }
                    Kind::Polygon { pts, fill } => {
                        let s: Vec<Pos2> = pts.iter().map(|p| screen(vp, rect, p)).collect();
                        if !s.iter().any(|p| cull.contains(*p)) {
                            continue;
                        }
                        let mut mesh = Mesh::default();
                        let c = c32(*fill);
                        for p in &s {
                            mesh.colored_vertex(*p, c);
                        }
                        for t in triangulate(&s) {
                            mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
                        }
                        painter.add(Shape::mesh(mesh));
                    }
                    Kind::Triangles { pts, fill } => {
                        let mut mesh = Mesh::default();
                        let c = c32(*fill);
                        for p in pts {
                            mesh.colored_vertex(screen(vp, rect, p), c);
                        }
                        for i in (0..pts.len() / 3 * 3).step_by(3) {
                            mesh.add_triangle(i as u32, i as u32 + 1, i as u32 + 2);
                        }
                        painter.add(Shape::mesh(mesh));
                    }
                    Kind::Text { at, font, text, .. } => {
                        let p = screen(vp, rect, at);
                        if !cull.contains(p) {
                            continue;
                        }
                        let size = f.pf.font_size(*font).clamp(7.0, 24.0);
                        let fid = FontId::monospace(size);
                        painter.text(
                            p + vec2(1.0, 1.0),
                            egui::Align2::CENTER_CENTER,
                            text,
                            fid.clone(),
                            Color32::from_black_alpha(200),
                        );
                        painter.text(p, egui::Align2::CENTER_CENTER, text, fid, color);
                    }
                    Kind::Icon {
                        at,
                        angle,
                        file,
                        index,
                        ..
                    } => {
                        let p = screen(vp, rect, at);
                        if !cull.contains(p) {
                            continue;
                        }
                        let (Some(def), Some((_, sheet))) = (
                            f.pf.icon_files.iter().find(|i| i.number == *file),
                            f.sheets.iter().find(|(n, _)| n == file),
                        ) else {
                            // No sheet (yet): a dot.
                            painter.circle_filled(p, 3.0, color);
                            continue;
                        };
                        let cols = (sheet.size[0] as u32 / def.w.max(1)).max(1);
                        let i = u32::from(index.saturating_sub(1));
                        let (col, row) = (i % cols, i / cols);
                        let [sw, sh] = [sheet.size[0] as f32, sheet.size[1] as f32];
                        let uv = Rect::from_min_size(
                            pos2(
                                col as f32 * def.w as f32 / sw,
                                row as f32 * def.h as f32 / sh,
                            ),
                            vec2(def.w as f32 / sw, def.h as f32 / sh),
                        );
                        let (w, h) = (def.w as f32, def.h as f32);
                        let corners = [
                            vec2(-def.hot_x, -def.hot_y),
                            vec2(w - def.hot_x, -def.hot_y),
                            vec2(w - def.hot_x, h - def.hot_y),
                            vec2(-def.hot_x, h - def.hot_y),
                        ];
                        let (s, c) = angle.to_radians().sin_cos();
                        let rot = |v: egui::Vec2| p + vec2(v.x * c - v.y * s, v.x * s + v.y * c);
                        let uvs = [
                            uv.left_top(),
                            uv.right_top(),
                            uv.right_bottom(),
                            uv.left_bottom(),
                        ];
                        let mut mesh = Mesh::with_texture(sheet.tex.id());
                        for (k, v) in corners.iter().enumerate() {
                            mesh.vertices.push(egui::epaint::Vertex {
                                pos: rot(*v),
                                uv: uvs[k],
                                color: if sheet.tint { color } else { Color32::WHITE },
                            });
                        }
                        mesh.add_triangle(0, 1, 2);
                        mesh.add_triangle(0, 2, 3);
                        painter.add(Shape::mesh(mesh));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangulates_concave_polygons() {
        // An L shape: 6 vertices, 4 triangles.
        let l = [
            pos2(0.0, 0.0),
            pos2(2.0, 0.0),
            pos2(2.0, 1.0),
            pos2(1.0, 1.0),
            pos2(1.0, 2.0),
            pos2(0.0, 2.0),
        ];
        let t = triangulate(&l);
        assert_eq!(t.len(), 4);
        let area: f32 = t
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (l[*a], l[*b], l[*c]);
                ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() / 2.0
            })
            .sum();
        assert!((area - 3.0).abs() < 1e-4, "{area}");
    }
}
