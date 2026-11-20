//! Level-of-detail terrain for the 3D view, so it can be zoomed from the
//! 400 km overview down to a few hundred metres, like a real map.
//!
//! The scene is covered by web-mercator tiles (level 9, ~60 km, at least);
//! each is split into its four children while the camera is close to it
//! compared with its size, down to level 18 (~150 m). Every leaf is a small
//! mesh: heights from the deepest loaded Terrarium tile (to level 15, then
//! interpolated), draped with the deepest loaded Esri imagery tile (to level
//! 19, then enlarged). Until a tile's own data arrives it uses its nearest
//! loaded ancestor, and before that the scene's base terrain and imagery, so
//! detail sharpens in without holes.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{pos2, Color32, Painter, Pos2, Rect, Shape, TextureId};
use ss_core::geo::{lonlat_to_world, world_to_lonlat};
use ss_core::radar3d::{local_km, Scene};
use ss_map::buildings::{self, Building, BuildingCache};
use ss_map::heights::{HeightCache, MAX_LEVEL};
use ss_map::tiles::{TileCache, TileKey};
use ss_map::viewport::TileId;

use super::radar3d::{terrain_color, View, V3};

/// Vertices per leaf side.
const N: usize = 17;
/// Coarsest leaves, and the deepest split.
const MIN_LEVEL: u8 = 9;
const MAX_SPLIT: u8 = 18;
/// Split a tile while the camera is nearer than this many tile widths.
const SPLIT: f32 = 3.0;
/// Cap on leaves per frame.
const MAX_LEAVES: usize = 420;
/// Deepest imagery level requested.
const IMAGERY_MAX: u8 = 19;
/// Leaves coarser than this use the scene's base imagery (sharper there).
const IMAGERY_MIN: u8 = 10;
const IMAGERY_KEY: &str = "esri-imagery-3d";

fn imagery_url(z: u8, x: u32, y: u32) -> String {
    format!(
        "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}?blankTile=false"
    )
}

/// A leaf's vertices: local east/north km and height km, plus which height
/// level they came from (255 = the base scene).
struct Geom {
    pts: Vec<[f32; 3]>,
    level: u8,
}

pub struct Lod {
    imagery: TileCache,
    heights: HeightCache,
    geom: HashMap<(TileId, (i64, i64)), Geom>,
    pub leaves: usize,
    blds: BuildingCache,
    /// Buildings of a tile in scene coordinates.
    local: HashMap<LocalKey, LocalTile>,
    /// Buildings drawn last frame.
    pub buildings: usize,
    /// One window bay (a window and its frame, one floor high), tiled
    /// across walls.
    facade: Option<egui::TextureHandle>,
}

/// A window bay is this wide and a floor this tall (metres).
const BAY_M: f32 = 3.0;
const FLOOR_M: f32 = 3.4;

/// The facade texture: glass with a lighter frame and floor slab, white
/// where the wall colour should show, so vertex colours tint it.
fn facade_image() -> egui::ColorImage {
    let n = 32;
    let mut px = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let glass = (4..28).contains(&x) && (7..27).contains(&y);
            let c = if glass {
                // Darker toward the bottom of each pane, a faint sky sheen on top.
                let t = (y - 7) as f32 / 20.0;
                let g = 0.42 + 0.16 * (1.0 - t);
                [g * 0.86, g * 0.94, g * 1.08]
            } else if y < 3 {
                // The floor slab.
                [0.82, 0.82, 0.82]
            } else {
                [1.0, 1.0, 1.0]
            };
            let b = |v: f32| (v.min(1.0) * 255.0) as u8;
            px.push(Color32::from_rgb(b(c[0]), b(c[1]), b(c[2])));
        }
    }
    egui::ColorImage::new([n, n], px)
}

/// The sun's direction (east, north, up) at a place and time, from the
/// standard low-precision solar position formulas; `None` below ~6°.
pub fn sun_direction(lat: f64, lon: f64, t: chrono::DateTime<chrono::Utc>) -> Option<V3> {
    use chrono::{Datelike, Timelike};
    let day = f64::from(t.ordinal()) - 1.0;
    let hour = f64::from(t.hour()) + f64::from(t.minute()) / 60.0;
    let g = 2.0 * std::f64::consts::PI / 365.0 * (day + (hour - 12.0) / 24.0);
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let tst = hour * 60.0 + eqtime + 4.0 * lon;
    let ha = (tst / 4.0 - 180.0).to_radians();
    let phi = lat.to_radians();
    let cos_zen = phi.sin() * decl.sin() + phi.cos() * decl.cos() * ha.cos();
    let el = cos_zen.clamp(-1.0, 1.0).asin();
    if el < 6f64.to_radians() {
        return None;
    }
    // Azimuth clockwise from north.
    let az = (ha.sin()).atan2(ha.cos() * phi.sin() - decl.tan() * phi.cos()) + std::f64::consts::PI;
    Some(V3::new(
        (az.sin() * el.cos()) as f32,
        (az.cos() * el.cos()) as f32,
        el.sin() as f32,
    ))
}

/// Convex hull (counter-clockwise), for shadow outlines.
fn hull(mut p: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut h: Vec<[f32; 2]> = Vec::with_capacity(p.len() * 2);
    for pass in 0..2 {
        let start = h.len();
        let it: Box<dyn Iterator<Item = &[f32; 2]>> = if pass == 0 {
            Box::new(p.iter())
        } else {
            Box::new(p.iter().rev())
        };
        for &q in it {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 {
                h.pop();
            }
            h.push(q);
        }
        h.pop();
    }
    h
}

/// Buildings are drawn when the camera is lower than this (km).
const BUILDINGS_BELOW_KM: f32 = 5.0;
/// Cap on buildings per frame (nearest first).
const MAX_BUILDINGS: usize = 6000;
/// Imagery level used for roofs.
const ROOF_LEVEL: u8 = 17;

/// A building tile in scene coordinates.
type LocalTile = std::sync::Arc<Vec<Local>>;
/// (level-14 tile, scene).
type LocalKey = ((u32, u32), (i64, i64));

/// A building in scene coordinates.
struct Local {
    /// Outline, east/north km, counter-clockwise.
    pts: Vec<[f32; 2]>,
    /// The same points in `ROOF_LEVEL` tile units (for the roof photo).
    world: Vec<[f64; 2]>,
    centre: [f32; 2],
    /// Half the outline's extent, km.
    radius: f32,
    top_km: f32,
    bottom_km: f32,
    roof: Vec<[u32; 3]>,
}

fn to_local(scene: &Scene, b: &Building) -> Option<Local> {
    let z = f64::from(ROOF_LEVEL);
    let mut pts: Vec<[f32; 2]> = Vec::with_capacity(b.ring.len());
    let mut world = Vec::with_capacity(b.ring.len());
    for &[lon, lat] in &b.ring {
        let (x, y) = local_km(scene.lat, scene.lon, lat, lon);
        pts.push([x, y]);
        let (wx, wy) = lonlat_to_world(lon, lat, z);
        world.push([wx / 256.0, wy / 256.0]);
    }
    let area: f32 = (0..pts.len())
        .map(|k| {
            let (p, q) = (pts[k], pts[(k + 1) % pts.len()]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum();
    if area.abs() < 1e-9 {
        return None;
    }
    if area < 0.0 {
        pts.reverse();
        world.reverse();
    }
    let n = pts.len() as f32;
    let centre = pts
        .iter()
        .fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
    let radius = pts
        .iter()
        .map(|p| (p[0] - centre[0]).hypot(p[1] - centre[1]))
        .fold(0.0, f32::max);
    Some(Local {
        roof: buildings::triangulate(&pts),
        pts,
        world,
        centre,
        radius,
        top_km: b.height / 1000.0,
        bottom_km: b.min_height / 1000.0,
    })
}

/// Scene identity for cached geometry (its centre, to ~1 m).
fn scene_key(s: &Scene) -> (i64, i64) {
    ((s.lat * 1e5) as i64, (s.lon * 1e5) as i64)
}

impl Lod {
    pub fn new(
        client: reqwest::Client,
        rt: tokio::runtime::Handle,
        tile_dir: Option<PathBuf>,
        ctx: egui::Context,
    ) -> Self {
        let blds = BuildingCache::new(
            client.clone(),
            rt.clone(),
            tile_dir.as_ref().map(|d| d.join("buildings")),
            ctx.clone(),
        );
        Self {
            // Hundreds of leaves (and their ancestors) are on screen at once
            // up close: keep enough textures that they don't churn.
            imagery: TileCache::new(client.clone(), rt.clone(), tile_dir.clone(), ctx.clone())
                .with_max_textures(2000),
            heights: HeightCache::new(client, rt, tile_dir.map(|d| d.join("terrain")), ctx),
            geom: HashMap::new(),
            leaves: 0,
            blds,
            local: HashMap::new(),
            buildings: 0,
            facade: None,
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        self.imagery.poll(ctx);
        self.heights.poll();
        self.blds.poll();
        if self.geom.len() > 4000 {
            self.geom.clear();
        }
        if self.local.len() > 200 {
            self.local.clear();
        }
    }

    /// Whether anything is still loading (to keep repainting).
    pub fn busy(&self) -> bool {
        self.heights.loading() > 0 || self.imagery.stats().loading > 0 || self.blds.loading() > 0
    }

    /// The tiles to draw for this camera.
    fn select(&self, scene: &Scene, eye: V3, ex: f32) -> Vec<TileId> {
        let half = f64::from(scene.half_km);
        let deg_lat = half / 111.2;
        let deg_lon = deg_lat / scene.lat.to_radians().cos().max(0.05);
        let z = f64::from(MIN_LEVEL);
        let (x0, y0) = lonlat_to_world(scene.lon - deg_lon, scene.lat + deg_lat, z);
        let (x1, y1) = lonlat_to_world(scene.lon + deg_lon, scene.lat - deg_lat, z);
        let n = 1i64 << MIN_LEVEL;
        let mut start = Vec::new();
        for ty in (y0 / 256.0) as i64..=(y1 / 256.0) as i64 {
            for tx in (x0 / 256.0).floor() as i64..=(x1 / 256.0).floor() as i64 {
                start.push(TileId {
                    z: MIN_LEVEL,
                    x: tx.rem_euclid(n) as u32,
                    y: ty.clamp(0, n - 1) as u32,
                });
            }
        }
        let ground = |x: f32, y: f32| {
            scene
                .terrain
                .as_ref()
                .map_or(0.0, |t| t.height_at(x, y) / 1000.0)
        };
        // How much a tile wants to split: its size over its distance. `None`
        // when it lies outside the scene.
        let need = |t: TileId| -> Option<f32> {
            let (a, b) = self.corners(scene, t);
            let h = scene.half_km;
            if a.0.min(b.0) > h || a.0.max(b.0) < -h || a.1.min(b.1) > h || a.1.max(b.1) < -h {
                return None;
            }
            let (cx, cy) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let size = (b.0 - a.0).abs().max((b.1 - a.1).abs());
            let d = V3::new(cx, cy, ground(cx, cy) * ex).sub(eye).len();
            Some(size / d.max(1e-4))
        };
        // Split the tiles nearest the camera (relative to their size) first,
        // so the leaf budget goes where the detail shows.
        let mut heap = std::collections::BinaryHeap::new();
        for t in start {
            if let Some(k) = need(t) {
                heap.push(Want(k, t));
            }
        }
        let mut leaves = Vec::new();
        while let Some(Want(k, t)) = heap.pop() {
            let room = leaves.len() + heap.len() + 4 <= MAX_LEAVES;
            if t.z < MAX_SPLIT && k > 1.0 / SPLIT && room {
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let c = TileId {
                        z: t.z + 1,
                        x: t.x * 2 + dx,
                        y: t.y * 2 + dy,
                    };
                    if let Some(k) = need(c) {
                        heap.push(Want(k, c));
                    }
                }
            } else {
                leaves.push(t);
            }
        }
        leaves
    }

    /// Local km of a tile's north-west and south-east corners.
    fn corners(&self, scene: &Scene, t: TileId) -> ((f32, f32), (f32, f32)) {
        let z = f64::from(t.z);
        let (lon0, lat0) = world_to_lonlat(f64::from(t.x) * 256.0, f64::from(t.y) * 256.0, z);
        let (lon1, lat1) =
            world_to_lonlat(f64::from(t.x + 1) * 256.0, f64::from(t.y + 1) * 256.0, z);
        (
            local_km(scene.lat, scene.lon, lat0, lon0),
            local_km(scene.lat, scene.lon, lat1, lon1),
        )
    }

    /// The deepest height level loaded for tile `t` (requesting its own),
    /// or `None` for the base scene.
    fn height_level(&mut self, t: TileId) -> Option<u8> {
        let zh = t.z.min(MAX_LEVEL);
        let (hx, hy) = (t.x >> (t.z - zh), t.y >> (t.z - zh));
        if self.heights.request(zh, hx, hy).is_some() {
            return Some(zh);
        }
        (zh.saturating_sub(8)..zh).rev().find(|&l| {
            self.heights
                .get(l, t.x >> (t.z - l), t.y >> (t.z - l))
                .is_some()
        })
    }

    fn geometry(&mut self, scene: &Scene, t: TileId) -> &Geom {
        let level = self.height_level(t);
        let key = (t, scene_key(scene));
        let want = level.unwrap_or(255);
        let stale = self.geom.get(&key).is_none_or(|g| g.level != want);
        if stale {
            let src = level.and_then(|l| {
                self.heights
                    .get(l, t.x >> (t.z - l), t.y >> (t.z - l))
                    .map(|tile| (l, tile))
            });
            let z = f64::from(t.z);
            let mut pts = Vec::with_capacity(N * N);
            for j in 0..N {
                for i in 0..N {
                    let fx = f64::from(t.x) + i as f64 / (N - 1) as f64;
                    let fy = f64::from(t.y) + j as f64 / (N - 1) as f64;
                    let (lon, lat) = world_to_lonlat(fx * 256.0, fy * 256.0, z);
                    let (x, y) = local_km(scene.lat, scene.lon, lat, lon);
                    let h_m = match &src {
                        Some((l, tile)) => {
                            let k = f64::from(1u32 << (t.z - l));
                            let (ax, ay) = (fx / k, fy / k);
                            tile.sample(ax - ax.floor(), ay - ay.floor())
                        }
                        None => scene.terrain.as_ref().map_or(0.0, |b| b.height_at(x, y)),
                    };
                    pts.push([x, y, h_m / 1000.0]);
                }
            }
            self.geom.insert(key, Geom { pts, level: want });
        }
        &self.geom[&key]
    }

    /// The imagery texture for tile `t` and the UV rect of `t` within it.
    fn imagery_for(&mut self, t: TileId) -> Option<(TextureId, Rect)> {
        if t.z < IMAGERY_MIN {
            return None;
        }
        let zi = t.z.min(IMAGERY_MAX);
        let up = t.z - zi;
        let it = TileId {
            z: zi,
            x: t.x >> up,
            y: t.y >> up,
        };
        // `t` within `it`.
        let k = (1u32 << up) as f32;
        let sub = Rect::from_min_size(
            pos2(
                (t.x - (it.x << up)) as f32 / k,
                (t.y - (it.y << up)) as f32 / k,
            ),
            egui::vec2(1.0 / k, 1.0 / k),
        );
        let key = TileKey::new(IMAGERY_KEY, it.z, it.x, it.y);
        let (tex, outer) = match self.imagery.request(&key, imagery_url(it.z, it.x, it.y)) {
            Some(tex) => (tex.id(), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))),
            None => {
                if self.imagery.is_failed(&key) && it.z > IMAGERY_MIN {
                    let (pz, px, py) = (it.z - 1, it.x / 2, it.y / 2);
                    let pk = TileKey::new(IMAGERY_KEY, pz, px, py);
                    let _ = self.imagery.request(&pk, imagery_url(pz, px, py));
                }
                ss_map::ancestor(&mut self.imagery, IMAGERY_KEY, it, it.z - IMAGERY_MIN)?
            }
        };
        let uv = Rect::from_min_size(
            outer.min + sub.min.to_vec2() * outer.size(),
            sub.size() * outer.size(),
        );
        Some((tex, uv))
    }

    /// Draw the terrain. `base` is the scene's own imagery texture (used
    /// where no tile imagery is loaded yet); `photo` false = height colours.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        painter: &Painter,
        view: &View,
        scene: &Scene,
        ex: f32,
        dark: bool,
        photo: bool,
        base: Option<TextureId>,
    ) {
        let leaves = self.select(scene, view.eye(), ex);
        self.leaves = leaves.len();
        let half = scene.half_km;
        let light = V3::new(-0.5, 0.6, 0.62).norm();
        let mut meshes: Vec<(f32, Mesh)> = Vec::with_capacity(leaves.len());
        for t in leaves {
            let tile_img = if photo { self.imagery_for(t) } else { None };
            let g = self.geometry(scene, t);
            let p = |i: usize, j: usize| g.pts[j.min(N - 1) * N + i.min(N - 1)];
            let mut mesh = Mesh::default();
            let mut depth = vec![f32::NAN; N * N];
            let mut sum = 0.0;
            let mut cnt = 0.0;
            for j in 0..N {
                for i in 0..N {
                    let [x, y, h] = p(i, j);
                    let dx = (p(i + 1, j)[2] - p(i.saturating_sub(1), j)[2]) * ex * 0.6;
                    let dy = (p(i, j.saturating_sub(1))[2] - p(i, j + 1)[2]) * ex * 0.6;
                    let cell = ((p(i + 1, j)[0] - p(i.saturating_sub(1), j)[0]).abs()).max(1e-3);
                    let nrm = V3::new(-dx, -dy, cell).norm();
                    let shade = nrm.dot(light).max(0.0);
                    let (pos, d) = view
                        .project(V3::new(x, y, h * ex))
                        .unwrap_or((Pos2::ZERO, f32::NAN));
                    depth[j * N + i] = d;
                    if d.is_finite() {
                        sum += d;
                        cnt += 1.0;
                    }
                    let (uv, color) = match (photo, tile_img, base) {
                        (true, Some((_, r)), _) => (
                            pos2(
                                r.min.x + r.width() * i as f32 / (N - 1) as f32,
                                r.min.y + r.height() * j as f32 / (N - 1) as f32,
                            ),
                            lit(shade, light),
                        ),
                        (true, None, Some(_)) => (
                            pos2(
                                ((x + half) / (2.0 * half)).clamp(0.0, 1.0),
                                ((half - y) / (2.0 * half)).clamp(0.0, 1.0),
                            ),
                            lit(shade, light),
                        ),
                        _ => (WHITE_UV, terrain_color(h * 1000.0, shade, dark)),
                    };
                    mesh.vertices.push(Vertex { pos, uv, color });
                }
            }
            if cnt == 0.0 {
                continue;
            }
            mesh.texture_id = match (photo, tile_img, base) {
                (true, Some((tex, _)), _) => tex,
                (true, None, Some(tex)) => tex,
                _ => TextureId::default(),
            };
            // Quads far to near, skipping those wholly outside the scene.
            let mut prims: Vec<(f32, Prim)> = Vec::with_capacity((N - 1) * (N - 1) + 4 * N);
            for j in 0..N - 1 {
                for i in 0..N - 1 {
                    let idx = j * N + i;
                    let ds = [
                        depth[idx],
                        depth[idx + 1],
                        depth[idx + N],
                        depth[idx + N + 1],
                    ];
                    if ds.iter().any(|d| d.is_nan()) {
                        continue;
                    }
                    let outside = [idx, idx + 1, idx + N, idx + N + 1].iter().all(|&k| {
                        let q = g.pts[k];
                        q[0].abs() > half || q[1].abs() > half
                    });
                    if !outside {
                        prims.push((ds.iter().sum::<f32>() / 4.0, Prim::Quad(idx as u32)));
                    }
                }
            }
            // Skirts: a short wall hanging under each edge, hiding the cracks
            // where a neighbouring tile has fewer vertices.
            let size = (g.pts[N - 1][0] - g.pts[0][0]).abs().max(1e-3);
            let drop = size * 0.03 * ex.max(1.0);
            let inside = |v: usize| g.pts[v][0].abs() < half && g.pts[v][1].abs() < half;
            for k in 0..N - 1 {
                for [a, b] in [
                    [k, k + 1],
                    [(N - 1) * N + k, (N - 1) * N + k + 1],
                    [k * N, (k + 1) * N],
                    [k * N + N - 1, (k + 1) * N + N - 1],
                ] {
                    let (da, db) = (depth[a], depth[b]);
                    // No skirts on the scene's own border (nothing beyond).
                    if da.is_nan() || db.is_nan() || !inside(a) || !inside(b) {
                        continue;
                    }
                    let low = |v: usize| {
                        let [x, y, h] = g.pts[v];
                        view.project(V3::new(x, y, h * ex - drop))
                    };
                    let (Some((pa, _)), Some((pb, _))) = (low(a), low(b)) else {
                        continue;
                    };
                    let base = mesh.vertices.len() as u32;
                    let (va, vb) = (mesh.vertices[a], mesh.vertices[b]);
                    mesh.vertices.push(Vertex { pos: pa, ..va });
                    mesh.vertices.push(Vertex { pos: pb, ..vb });
                    // Just behind the surface it hangs from.
                    prims.push((
                        (da + db) / 2.0 + 1e-3,
                        Prim::Skirt(a as u32, b as u32, base),
                    ));
                }
            }
            prims.sort_by(|a, b| b.0.total_cmp(&a.0));
            let n = N as u32;
            for (_, prim) in prims {
                match prim {
                    Prim::Quad(i) => {
                        mesh.indices
                            .extend_from_slice(&[i, i + 1, i + n + 1, i, i + n + 1, i + n])
                    }
                    Prim::Skirt(a, b, low) => {
                        mesh.indices
                            .extend_from_slice(&[a, b, low + 1, a, low + 1, low])
                    }
                }
            }
            meshes.push((sum / cnt, mesh));
        }
        meshes.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, m) in meshes {
            painter.add(Shape::mesh(m));
        }
    }

    /// 3D buildings (OpenStreetMap) around `focus` when the camera is low:
    /// walls lit by the sun, roofs from the satellite photo.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_buildings(
        &mut self,
        painter: &Painter,
        view: &View,
        scene: &Scene,
        ex: f32,
        focus: (f32, f32),
        dark: bool,
        photo: bool,
    ) {
        self.buildings = 0;
        let eye = view.eye();
        let alt = eye.z - self.ground(scene, eye.x, eye.y) * ex;
        if alt > BUILDINGS_BELOW_KM {
            return;
        }
        let radius = (alt * 2.5).clamp(0.8, 3.0);
        // Level-14 tiles around the focus.
        let deg_lat = 1.0 / 111.19;
        let deg_lon = deg_lat / scene.lat.to_radians().cos().max(0.05);
        let lat = |y: f32| scene.lat + f64::from(y) * deg_lat;
        let lon = |x: f32| scene.lon + f64::from(x) * deg_lon;
        let z = f64::from(buildings::LEVEL);
        let (x0, y0) = lonlat_to_world(lon(focus.0 - radius), lat(focus.1 + radius), z);
        let (x1, y1) = lonlat_to_world(lon(focus.0 + radius), lat(focus.1 - radius), z);
        let skey = scene_key(scene);
        let mut tiles = Vec::new();
        for ty in (y0 / 256.0) as u32..=(y1 / 256.0) as u32 {
            for tx in (x0 / 256.0) as u32..=(x1 / 256.0) as u32 {
                let key = ((tx, ty), skey);
                if let Some(l) = self.local.get(&key) {
                    self.blds.request(tx, ty);
                    tiles.push(l.clone());
                } else if let Some(b) = self.blds.request(tx, ty) {
                    let l: std::sync::Arc<Vec<Local>> =
                        std::sync::Arc::new(b.iter().filter_map(|b| to_local(scene, b)).collect());
                    self.local.insert(key, l.clone());
                    tiles.push(l);
                }
            }
        }
        // Nearest first, within the radius.
        let mut near: Vec<(f32, &Local)> = tiles
            .iter()
            .flat_map(|t| t.iter())
            .filter(|b| (b.centre[0] - focus.0).hypot(b.centre[1] - focus.1) < radius)
            .map(|b| ((b.centre[0] - eye.x).hypot(b.centre[1] - eye.y), b))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.truncate(MAX_BUILDINGS);
        let clip = painter.clip_rect().expand(40.0);
        // The real sun for this place and time; a soft afternoon light (and no
        // cast shadows) when it's down.
        let sun = sun_direction(scene.lat, scene.lon, chrono::Utc::now());
        let light = sun.unwrap_or_else(|| V3::new(-0.5, 0.6, 0.62).norm());
        let facade = self
            .facade
            .get_or_insert_with(|| {
                painter.ctx().load_texture(
                    "building-facade",
                    facade_image(),
                    egui::TextureOptions {
                        magnification: egui::TextureFilter::Linear,
                        minification: egui::TextureFilter::Linear,
                        wrap_mode: egui::TextureWrapMode::Repeat,
                        mipmap_mode: Some(egui::TextureFilter::Linear),
                    },
                )
            })
            .id();
        let mut shadows = Mesh::default();
        let mut meshes: Vec<(f32, [Mesh; 2])> = Vec::with_capacity(near.len());
        for (dist, b) in near {
            // Too small to see?
            if b.radius / dist.max(1e-3) * view.scale_at(1.0) < 1.5 {
                continue;
            }
            let ground = self.ground(scene, b.centre[0], b.centre[1]);
            let (z0, z1) = ((ground + b.bottom_km) * ex, (ground + b.top_km) * ex);
            let mut low = Vec::with_capacity(b.pts.len());
            let mut high = Vec::with_capacity(b.pts.len());
            let mut ok = true;
            for p in &b.pts {
                match (
                    view.project(V3::new(p[0], p[1], z0)),
                    view.project(V3::new(p[0], p[1], z1)),
                ) {
                    (Some(l), Some(h)) => {
                        low.push(l);
                        high.push(h);
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok || !high.iter().chain(&low).any(|(p, _)| clip.contains(*p)) {
                continue;
            }
            // Its shadow on the ground: the outline and its copy cast along
            // the sun, wrapped.
            if let Some(s) = sun {
                let reach = ((b.top_km - b.bottom_km) * ex / s.z).min(1.5);
                let (dx, dy) = (-s.x * reach, -s.y * reach);
                let mut pts = b.pts.clone();
                pts.extend(b.pts.iter().map(|p| [p[0] + dx, p[1] + dy]));
                let outline: Option<Vec<Pos2>> = hull(pts)
                    .iter()
                    .map(|p| {
                        view.project(V3::new(p[0], p[1], ground * ex + 0.0005))
                            .map(|(q, _)| q)
                    })
                    .collect();
                if let Some(o) = outline.filter(|o| o.len() >= 3) {
                    let v = shadows.vertices.len() as u32;
                    for pos in o.iter().copied() {
                        shadows.vertices.push(Vertex {
                            pos,
                            uv: WHITE_UV,
                            color: Color32::from_black_alpha(70),
                        });
                    }
                    for k in 1..o.len() as u32 - 1 {
                        shadows.indices.extend_from_slice(&[v, v + k, v + k + 1]);
                    }
                }
            }
            // The roof photo: the deepest imagery tile holding the whole roof
            // (a roof across two tiles would smear).
            let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
            for w in &b.world {
                lo = [lo[0].min(w[0]), lo[1].min(w[1])];
                hi = [hi[0].max(w[0]), hi[1].max(w[1])];
            }
            let mut t = TileId {
                z: ROOF_LEVEL,
                x: lo[0] as u32,
                y: lo[1] as u32,
            };
            let mut scale = 1.0;
            let fits = |t: TileId, scale: f64| {
                (hi[0] / scale) as u32 == t.x && (hi[1] / scale) as u32 == t.y
            };
            while t.z > ROOF_LEVEL - 2 && !fits(t, scale) {
                scale *= 2.0;
                t = TileId {
                    z: t.z - 1,
                    x: (lo[0] / scale) as u32,
                    y: (lo[1] / scale) as u32,
                };
            }
            // A roof wider than that gets a plain one rather than a blur.
            let img = if photo && fits(t, scale) {
                self.imagery_for(t)
            } else {
                None
            };
            let uv_at = |w: [f64; 2]| match img {
                Some((_, r)) => pos2(
                    r.min.x + r.width() * ((w[0] / scale - f64::from(t.x)) as f32).clamp(0.0, 1.0),
                    r.min.y + r.height() * ((w[1] / scale - f64::from(t.y)) as f32).clamp(0.0, 1.0),
                ),
                None => WHITE_UV,
            };
            // Concrete and glass, a little different per building.
            let vary =
                0.94 + 0.12 * ((b.centre[0] * 9973.0 + b.centre[1] * 7919.0).sin() * 0.5 + 0.5);
            let tint = |k: f32| {
                let base: [f32; 3] = if dark {
                    [0.70, 0.72, 0.76]
                } else {
                    [0.86, 0.86, 0.84]
                };
                let c = |v: f32| (v * k * vary * 255.0).clamp(0.0, 255.0) as u8;
                Color32::from_rgb(c(base[0]), c(base[1]), c(base[2]))
            };
            let mut mesh = Mesh::with_texture(facade);
            let tall_m = (b.top_km - b.bottom_km) * 1000.0;
            let floors = (tall_m / FLOOR_M).max(1.0).round();
            // Walls facing the eye, far to near.
            let n = b.pts.len();
            let mut walls: Vec<(f32, usize)> = (0..n)
                .filter_map(|i| {
                    let (p, q) = (b.pts[i], b.pts[(i + 1) % n]);
                    let (nx, ny) = (q[1] - p[1], p[0] - q[0]); // outward
                    let (mx, my) = ((p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0);
                    let facing = nx * (eye.x - mx) + ny * (eye.y - my);
                    (facing > 0.0).then(|| ((eye.x - mx).hypot(eye.y - my), i))
                })
                .collect();
            walls.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (_, i) in walls {
                let j = (i + 1) % n;
                let (p, q) = (b.pts[i], b.pts[j]);
                let len = (q[0] - p[0]).hypot(q[1] - p[1]).max(1e-6);
                let (nx, ny) = ((q[1] - p[1]) / len, (p[0] - q[0]) / len);
                let lit = (nx * light.x + ny * light.y).max(0.0);
                let k = if sun.is_some() {
                    0.5 + 0.55 * lit
                } else {
                    0.62 + 0.38 * lit
                };
                let (top, bottom) = (tint(k), tint(k * 0.82));
                // Whole bays and floors, so windows line up at the corners.
                let bays = (len * 1000.0 / BAY_M).max(1.0).round();
                let v = mesh.vertices.len() as u32;
                for (pos, uv, color) in [
                    (low[i].0, pos2(0.0, floors), bottom),
                    (low[j].0, pos2(bays, floors), bottom),
                    (high[j].0, pos2(bays, 0.0), top),
                    (high[i].0, pos2(0.0, 0.0), top),
                ] {
                    mesh.vertices.push(Vertex { pos, uv, color });
                }
                mesh.indices
                    .extend_from_slice(&[v, v + 1, v + 2, v, v + 2, v + 3]);
            }
            // The roof, when seen from above: the photo, or plain.
            let mut roof_mesh = Mesh::with_texture(img.map_or(TextureId::default(), |(t, _)| t));
            if eye.z > z1 {
                let v = roof_mesh.vertices.len() as u32;
                let roof = if img.is_some() {
                    Color32::WHITE
                } else {
                    tint(0.74)
                };
                for (h, w) in high.iter().zip(&b.world) {
                    roof_mesh.vertices.push(Vertex {
                        pos: h.0,
                        uv: uv_at(*w),
                        color: roof,
                    });
                }
                for tri in &b.roof {
                    roof_mesh
                        .indices
                        .extend_from_slice(&[v + tri[0], v + tri[1], v + tri[2]]);
                }
            }
            let depth = high.iter().map(|h| h.1).sum::<f32>() / high.len() as f32;
            meshes.push((depth, [mesh, roof_mesh]));
        }
        meshes.sort_by(|a, b| b.0.total_cmp(&a.0));
        self.buildings = meshes.len();
        if !shadows.indices.is_empty() {
            painter.add(Shape::mesh(shadows));
        }
        for (_, ms) in meshes {
            for m in ms {
                if !m.indices.is_empty() {
                    painter.add(Shape::mesh(m));
                }
            }
        }
    }

    /// The best-known ground height (km) at local (`x`, `y`): the deepest
    /// loaded height tile, else the base scene.
    pub fn ground(&mut self, scene: &Scene, x: f32, y: f32) -> f32 {
        let deg_lat = 1.0 / 111.19;
        let lat = scene.lat + f64::from(y) * deg_lat;
        let lon = scene.lon + f64::from(x) * deg_lat / scene.lat.to_radians().cos().max(0.05);
        for l in (MIN_LEVEL..=MAX_LEVEL).rev() {
            let (wx, wy) = lonlat_to_world(lon, lat, f64::from(l));
            let (tx, ty) = ((wx / 256.0) as u32, (wy / 256.0) as u32);
            if let Some(tile) = self.heights.get(l, tx, ty) {
                return tile.sample(wx / 256.0 - f64::from(tx), wy / 256.0 - f64::from(ty))
                    / 1000.0;
            }
        }
        scene
            .terrain
            .as_ref()
            .map_or(0.0, |t| t.height_at(x, y) / 1000.0)
    }
}

/// What a leaf mesh draws, in depth order.
enum Prim {
    /// The quad whose top-left vertex is this index.
    Quad(u32),
    /// An edge (two surface vertices) and its two lowered copies.
    Skirt(u32, u32, u32),
}

/// Daylight on the photo: flat ground at full brightness, slopes facing
/// away from the sun darker.
fn lit(shade: f32, light: V3) -> Color32 {
    // Gentle: the photo already has its own shading.
    let k = (0.6 + 0.45 * shade / light.z).clamp(0.55, 1.0);
    let g = (k * 255.0) as u8;
    Color32::from_rgb(g, g, g)
}

/// A tile in the split queue, ordered by how much it wants splitting.
struct Want(f32, TileId);

impl PartialEq for Want {
    fn eq(&self, o: &Self) -> bool {
        self.0.total_cmp(&o.0).is_eq()
    }
}
impl Eq for Want {}
impl PartialOrd for Want {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Want {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&o.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sun_is_high_at_noon_and_down_at_night() {
        let t = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().to_utc();
        // Melbourne, mid-summer: ~2 pm local is 03:00 UTC; the sun is high
        // and to the north.
        let s = sun_direction(-37.81, 144.96, t("2026-01-15T03:00:00Z")).unwrap();
        assert!(s.z > 0.85, "{s:?}");
        assert!(s.y > 0.0, "{s:?}");
        // Midnight local.
        assert!(sun_direction(-37.81, 144.96, t("2026-01-15T14:00:00Z")).is_none());
        // Morning sun is in the east.
        let m = sun_direction(-37.81, 144.96, t("2026-01-14T22:00:00Z")).unwrap();
        assert!(m.x > 0.5, "{m:?}");
    }

    #[test]
    fn hull_wraps_points() {
        let h = hull(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [0.5, 0.5],
        ]);
        assert_eq!(h.len(), 4);
    }
}
