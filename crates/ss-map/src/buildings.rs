//! Building footprints with heights for the 3D view, from OpenFreeMap's
//! vector tiles (OpenStreetMap data, OpenMapTiles schema; free, keyless,
//! worldwide). Tiles are level 14 (~2.4 km at the equator), decoded on worker
//! threads, kept on disk and in memory.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use tokio::sync::Semaphore;

/// The TileJSON naming the current planet build.
pub const TILEJSON_URL: &str = "https://tiles.openfreemap.org/planet";
/// Buildings are only published to this level.
pub const LEVEL: u8 = 14;
const MAX_TILES: usize = 64;
const RETRY_AFTER: Duration = Duration::from_secs(30);

/// One building: its outline (longitude, latitude; closed implicitly), roof
/// and base heights above the ground in metres, and its OSM colour if any.
#[derive(Debug, Clone, PartialEq)]
pub struct Building {
    pub ring: Vec<[f64; 2]>,
    pub height: f32,
    pub min_height: f32,
    pub colour: Option<[u8; 3]>,
}

// --- Minimal Mapbox Vector Tile reader --------------------------------------

struct Pb<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Pb<'a> {
    fn new(b: &'a [u8]) -> Self {
        Pb { b, i: 0 }
    }

    fn done(&self) -> bool {
        self.i >= self.b.len()
    }

    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.b.get(self.i)?;
            self.i += 1;
            v |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Some(v);
            }
        }
        None
    }

    /// (field, wire type)
    fn key(&mut self) -> Option<(u32, u8)> {
        let k = self.varint()?;
        Some(((k >> 3) as u32, (k & 7) as u8))
    }

    fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.varint()? as usize;
        let s = self.b.get(self.i..self.i + n)?;
        self.i += n;
        Some(s)
    }

    fn skip(&mut self, wire: u8) -> Option<()> {
        match wire {
            0 => {
                self.varint()?;
            }
            1 => self.i += 8,
            2 => {
                self.bytes()?;
            }
            5 => self.i += 4,
            _ => return None,
        }
        (self.i <= self.b.len()).then_some(())
    }

    fn packed(&mut self) -> Option<Vec<u32>> {
        let mut p = Pb::new(self.bytes()?);
        let mut out = Vec::new();
        while !p.done() {
            out.push(p.varint()? as u32);
        }
        Some(out)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Str(String),
    Num(f64),
    Bool(bool),
}

fn read_value(b: &[u8]) -> Option<Value> {
    let mut p = Pb::new(b);
    let mut v = None;
    while !p.done() {
        let (f, w) = p.key()?;
        match (f, w) {
            (1, 2) => v = Some(Value::Str(String::from_utf8_lossy(p.bytes()?).into())),
            (2, 5) => {
                let s = p.b.get(p.i..p.i + 4)?;
                p.i += 4;
                v = Some(Value::Num(f64::from(f32::from_le_bytes(
                    s.try_into().ok()?,
                ))));
            }
            (3, 1) => {
                let s = p.b.get(p.i..p.i + 8)?;
                p.i += 8;
                v = Some(Value::Num(f64::from_le_bytes(s.try_into().ok()?)));
            }
            (4 | 5, 0) => v = Some(Value::Num(p.varint()? as i64 as f64)),
            (6, 0) => {
                let u = p.varint()?;
                v = Some(Value::Num(((u >> 1) as i64 ^ -((u & 1) as i64)) as f64));
            }
            (7, 0) => v = Some(Value::Bool(p.varint()? != 0)),
            _ => p.skip(w)?,
        }
    }
    v
}

/// Polygon rings of a feature, in tile units (0..extent), exterior rings
/// only (holes are dropped).
fn polygon_rings(geom: &[u32]) -> Vec<Vec<[i32; 2]>> {
    let mut rings = Vec::new();
    let (mut x, mut y) = (0i32, 0i32);
    let mut ring: Vec<[i32; 2]> = Vec::new();
    let mut i = 0;
    let zz = |v: u32| ((v >> 1) as i32) ^ -((v & 1) as i32);
    while i < geom.len() {
        let cmd = geom[i] & 7;
        let count = (geom[i] >> 3) as usize;
        i += 1;
        match cmd {
            1 | 2 => {
                for _ in 0..count {
                    let (Some(&dx), Some(&dy)) = (geom.get(i), geom.get(i + 1)) else {
                        return rings;
                    };
                    i += 2;
                    x += zz(dx);
                    y += zz(dy);
                    if cmd == 1 {
                        ring.clear();
                    }
                    ring.push([x, y]);
                }
            }
            7 => {
                // Exterior rings have positive area (clockwise on screen).
                if ring.len() >= 3 && signed_area(&ring) > 0.0 {
                    rings.push(std::mem::take(&mut ring));
                } else {
                    ring.clear();
                }
            }
            _ => return rings,
        }
    }
    rings
}

fn signed_area(r: &[[i32; 2]]) -> f64 {
    let mut a = 0.0;
    for k in 0..r.len() {
        let (p, q) = (r[k], r[(k + 1) % r.len()]);
        a += f64::from(p[0]) * f64::from(q[1]) - f64::from(q[0]) * f64::from(p[1]);
    }
    a / 2.0
}

fn parse_colour(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    let v = u32::from_str_radix(h, 16).ok()?;
    match h.len() {
        6 => Some([(v >> 16) as u8, (v >> 8) as u8, v as u8]),
        3 => Some([
            ((v >> 8) & 0xf) as u8 * 17,
            ((v >> 4) & 0xf) as u8 * 17,
            (v & 0xf) as u8 * 17,
        ]),
        _ => None,
    }
}

/// The buildings of vector tile `z`/`x`/`y`. Parts outside the tile proper
/// (its buffer) are left to the neighbouring tile: a building is kept by the
/// tile its outline's centre falls in.
pub fn decode_buildings(pbf: &[u8], z: u8, x: u32, y: u32) -> Result<Vec<Building>, String> {
    let bad = || "malformed vector tile".to_string();
    let mut out = Vec::new();
    let mut tile = Pb::new(pbf);
    while !tile.done() {
        let (f, w) = tile.key().ok_or_else(bad)?;
        if (f, w) != (3, 2) {
            tile.skip(w).ok_or_else(bad)?;
            continue;
        }
        let mut layer = Pb::new(tile.bytes().ok_or_else(bad)?);
        let mut name = String::new();
        let mut keys: Vec<String> = Vec::new();
        let mut values = Vec::new();
        let mut features: Vec<&[u8]> = Vec::new();
        let mut extent = 4096u32;
        while !layer.done() {
            let (f, w) = layer.key().ok_or_else(bad)?;
            match (f, w) {
                (1, 2) => name = String::from_utf8_lossy(layer.bytes().ok_or_else(bad)?).into(),
                (2, 2) => features.push(layer.bytes().ok_or_else(bad)?),
                (3, 2) => keys.push(String::from_utf8_lossy(layer.bytes().ok_or_else(bad)?).into()),
                (4, 2) => values.push(read_value(layer.bytes().ok_or_else(bad)?)),
                (5, 0) => extent = layer.varint().ok_or_else(bad)? as u32,
                _ => layer.skip(w).ok_or_else(bad)?,
            }
        }
        if name != "building" {
            continue;
        }
        let ext = f64::from(extent.max(1));
        let n = f64::from(1u32 << z);
        for fb in features {
            let mut fp = Pb::new(fb);
            let (mut tags, mut geom, mut kind) = (Vec::new(), Vec::new(), 0);
            while !fp.done() {
                let (f, w) = fp.key().ok_or_else(bad)?;
                match (f, w) {
                    (2, 2) => tags = fp.packed().ok_or_else(bad)?,
                    (3, 0) => kind = fp.varint().ok_or_else(bad)?,
                    (4, 2) => geom = fp.packed().ok_or_else(bad)?,
                    _ => fp.skip(w).ok_or_else(bad)?,
                }
            }
            if kind != 3 {
                continue;
            }
            let (mut height, mut min_height, mut colour, mut hide) = (None, 0.0, None, false);
            for kv in tags.chunks_exact(2) {
                let (Some(k), Some(Some(v))) =
                    (keys.get(kv[0] as usize), values.get(kv[1] as usize))
                else {
                    continue;
                };
                match (k.as_str(), v) {
                    ("render_height", Value::Num(h)) => height = Some(*h as f32),
                    ("render_min_height", Value::Num(h)) => min_height = *h as f32,
                    ("colour", Value::Str(c)) => colour = parse_colour(c),
                    ("hide_3d", Value::Bool(b)) => hide = *b,
                    _ => {}
                }
            }
            if hide {
                continue;
            }
            for ring in polygon_rings(&geom) {
                let (sx, sy) = ring.iter().fold((0.0, 0.0), |a, p| {
                    (a.0 + f64::from(p[0]), a.1 + f64::from(p[1]))
                });
                let (cx, cy) = (sx / ring.len() as f64, sy / ring.len() as f64);
                if !(0.0..ext).contains(&cx) || !(0.0..ext).contains(&cy) {
                    continue;
                }
                let ring = ring
                    .iter()
                    .map(|p| {
                        let wx = (f64::from(x) + f64::from(p[0]) / ext) / n;
                        let wy = (f64::from(y) + f64::from(p[1]) / ext) / n;
                        let lon = wx * 360.0 - 180.0;
                        let lat = (std::f64::consts::PI * (1.0 - 2.0 * wy)).sinh().atan();
                        [lon, lat.to_degrees()]
                    })
                    .collect();
                out.push(Building {
                    ring,
                    height: height.unwrap_or(6.0).max(min_height + 1.0),
                    min_height,
                    colour,
                });
            }
        }
    }
    Ok(out)
}

// --- Cache --------------------------------------------------------------------

enum State {
    Loading,
    Ready(Arc<Vec<Building>>),
    Failed(Instant),
}

type Key = (u32, u32);

enum Msg {
    Template(Result<String, String>),
    Tile(Key, Result<Vec<Building>, String>),
}

pub struct BuildingCache {
    client: reqwest::Client,
    rt: tokio::runtime::Handle,
    disk: Option<PathBuf>,
    ctx: egui::Context,
    /// Tile URL with `{z}/{x}/{y}`, from the TileJSON.
    template: Option<String>,
    template_asked: Option<Instant>,
    entries: HashMap<Key, (State, u64)>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    sem: Arc<Semaphore>,
    frame: u64,
}

impl BuildingCache {
    pub fn new(
        client: reqwest::Client,
        rt: tokio::runtime::Handle,
        disk: Option<PathBuf>,
        ctx: egui::Context,
    ) -> Self {
        if let Some(d) = &disk {
            let _ = std::fs::create_dir_all(d);
        }
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            client,
            rt,
            disk,
            ctx,
            template: None,
            template_asked: None,
            entries: HashMap::new(),
            tx,
            rx,
            sem: Arc::new(Semaphore::new(4)),
            frame: 0,
        }
    }

    fn ask_template(&mut self) {
        if self
            .template_asked
            .is_some_and(|t| t.elapsed() < RETRY_AFTER)
        {
            return;
        }
        self.template_asked = Some(Instant::now());
        let (client, tx, ctx) = (self.client.clone(), self.tx.clone(), self.ctx.clone());
        self.rt.spawn(async move {
            let r = async {
                let json: serde_json::Value = client
                    .get(TILEJSON_URL)
                    .send()
                    .await
                    .and_then(reqwest::Response::error_for_status)
                    .map_err(|e| e.to_string())?
                    .json()
                    .await
                    .map_err(|e| e.to_string())?;
                json.get("tiles")
                    .and_then(|t| t.get(0))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| "no tile URL in the TileJSON".to_string())
            }
            .await;
            let _ = tx.send(Msg::Template(r));
            ctx.request_repaint();
        });
    }

    /// The buildings of level-14 tile (`x`, `y`) if loaded; otherwise start
    /// fetching it.
    pub fn request(&mut self, x: u32, y: u32) -> Option<Arc<Vec<Building>>> {
        let key = (x, y);
        let frame = self.frame;
        let fetch = match self.entries.get_mut(&key) {
            Some((State::Ready(t), used)) => {
                *used = frame;
                return Some(t.clone());
            }
            Some((State::Loading, used)) => {
                *used = frame;
                false
            }
            Some((State::Failed(at), _)) => at.elapsed() >= RETRY_AFTER,
            None => true,
        };
        if !fetch {
            return None;
        }
        let path = self
            .disk
            .as_ref()
            .map(|d| d.join(format!("buildings-{LEVEL}-{x}-{y}.pbf")));
        let cached = path.as_ref().is_some_and(|p| p.exists());
        let url = match (&self.template, cached) {
            (Some(t), _) => Some(
                t.replace("{z}", &LEVEL.to_string())
                    .replace("{x}", &x.to_string())
                    .replace("{y}", &y.to_string()),
            ),
            (None, true) => None,
            (None, false) => {
                self.ask_template();
                return None;
            }
        };
        self.entries.insert(key, (State::Loading, frame));
        let (client, tx, sem, ctx) = (
            self.client.clone(),
            self.tx.clone(),
            self.sem.clone(),
            self.ctx.clone(),
        );
        self.rt.spawn(async move {
            let Ok(_permit) = sem.acquire().await else {
                return;
            };
            let bytes = match path.as_ref().and_then(|p| std::fs::read(p).ok()) {
                Some(b) => Ok(b),
                None => match url {
                    Some(url) => match client.get(&url).send().await {
                        Ok(r) => match r.error_for_status() {
                            Ok(r) => r
                                .bytes()
                                .await
                                .map(|b| b.to_vec())
                                .map_err(|e| e.to_string()),
                            Err(e) => Err(e.to_string()),
                        },
                        Err(e) => Err(e.to_string()),
                    },
                    None => Err("no tile URL".into()),
                },
            };
            let result = match bytes {
                Ok(b) => {
                    let decoded = tokio::task::spawn_blocking({
                        let b = b.clone();
                        move || decode_buildings(&b, LEVEL, x, y)
                    })
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                    if decoded.is_ok() {
                        if let Some(p) = &path {
                            if !p.exists() {
                                let _ = std::fs::write(p, &b);
                            }
                        }
                    }
                    decoded
                }
                Err(e) => Err(e),
            };
            let _ = tx.send(Msg::Tile(key, result));
            ctx.request_repaint();
        });
        None
    }

    /// Collect finished work and evict old tiles. Call once per frame.
    pub fn poll(&mut self) {
        self.frame += 1;
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Template(Ok(t)) => self.template = Some(t),
                Msg::Template(Err(e)) => tracing::debug!("building tiles: {e}"),
                Msg::Tile(key, r) => {
                    let used = self.entries.get(&key).map_or(self.frame, |e| e.1);
                    let state = match r {
                        Ok(t) => State::Ready(Arc::new(t)),
                        Err(e) => {
                            tracing::debug!("building tile {key:?}: {e}");
                            State::Failed(Instant::now())
                        }
                    };
                    self.entries.insert(key, (state, used));
                }
            }
        }
        let ready = self
            .entries
            .values()
            .filter(|e| matches!(e.0, State::Ready(_)))
            .count();
        if ready > MAX_TILES {
            let mut old: Vec<(u64, Key)> = self
                .entries
                .iter()
                .filter(|(_, e)| matches!(e.0, State::Ready(_)) && e.1 + 1 < self.frame)
                .map(|(k, e)| (e.1, *k))
                .collect();
            old.sort_unstable();
            for (_, k) in old.into_iter().take(ready - MAX_TILES) {
                self.entries.remove(&k);
            }
        }
    }

    pub fn loading(&self) -> usize {
        self.entries
            .values()
            .filter(|e| matches!(e.0, State::Loading))
            .count()
    }
}

/// Triangulate a simple polygon (ear clipping). Returns index triples into
/// `pts`; empty for degenerate input.
pub fn triangulate(pts: &[[f32; 2]]) -> Vec<[u32; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let area: f32 = (0..n)
        .map(|k| {
            let (p, q) = (pts[k], pts[(k + 1) % n]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum();
    // Work counter-clockwise.
    let mut idx: Vec<usize> = if area > 0.0 {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };
    let cross = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let mut out = Vec::with_capacity(n - 2);
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            if cross(a, b, c) <= 0.0 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                j != ia
                    && j != ib
                    && j != ic
                    && cross(a, b, pts[j]) >= 0.0
                    && cross(b, c, pts[j]) >= 0.0
                    && cross(c, a, pts[j]) >= 0.0
            });
            if inside {
                continue;
            }
            out.push([ia as u32, ib as u32, ic as u32]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 {
        out.push([idx[0] as u32, idx[1] as u32, idx[2] as u32]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut v: u64, out: &mut Vec<u8>) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    fn field(f: u32, bytes: &[u8], out: &mut Vec<u8>) {
        varint(u64::from(f << 3 | 2), out);
        varint(bytes.len() as u64, out);
        out.extend_from_slice(bytes);
    }

    fn packed(vals: &[u32]) -> Vec<u8> {
        let mut b = Vec::new();
        for &v in vals {
            varint(u64::from(v), &mut b);
        }
        b
    }

    fn zz(v: i32) -> u32 {
        ((v << 1) ^ (v >> 31)) as u32
    }

    #[test]
    fn decodes_a_building() {
        // A 100×100 square at (1000, 1000), clockwise on screen, 30 m tall.
        let geom = [
            9,
            zz(1000),
            zz(1000),
            (3 << 3) | 2,
            zz(100),
            zz(0),
            zz(0),
            zz(100),
            zz(-100),
            zz(0),
            15,
        ];
        let mut feat = Vec::new();
        field(2, &packed(&[0, 0]), &mut feat);
        varint(3 << 3, &mut feat);
        varint(3, &mut feat);
        field(4, &packed(&geom), &mut feat);
        let mut val = Vec::new();
        varint(4 << 3, &mut val);
        varint(30, &mut val);
        let mut layer = Vec::new();
        field(1, b"building", &mut layer);
        field(2, &feat, &mut layer);
        field(3, b"render_height", &mut layer);
        field(4, &val, &mut layer);
        varint(5 << 3, &mut layer);
        varint(4096, &mut layer);
        let mut tile = Vec::new();
        field(3, &layer, &mut tile);
        let b = decode_buildings(&tile, 14, 8192, 8192).unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].ring.len(), 4);
        assert_eq!(b[0].height, 30.0);
        // Tile (8192, 8192) at level 14 starts at 0°, 0°.
        assert!(b[0].ring[0][0] > 0.0 && b[0].ring[0][0] < 0.01);
        assert!(b[0].ring[0][1] < 0.0);
    }

    #[test]
    fn triangulates_concave_outlines() {
        // An L shape: 6 points, 4 triangles.
        let l = [
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
        ];
        let t = triangulate(&l);
        assert_eq!(t.len(), 4);
        let area: f32 = t
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (l[*a as usize], l[*b as usize], l[*c as usize]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
            })
            .sum();
        assert!((area - 3.0).abs() < 1e-4);
        assert_eq!(parse_colour("#a0b"), Some([0xaa, 0x00, 0xbb]));
    }
}

#[cfg(test)]
mod real_tile {
    #[test]
    #[ignore]
    fn decodes_a_real_tile() {
        let b = std::fs::read("/tmp/t.pbf").unwrap();
        let v = super::decode_buildings(&b, 14, 14788, 10055).unwrap();
        let tall = v.iter().filter(|b| b.height > 50.0).count();
        eprintln!(
            "{} buildings, {tall} over 50 m, first {:?}",
            v.len(),
            v.first()
        );
        assert!(!v.is_empty());
    }
}
