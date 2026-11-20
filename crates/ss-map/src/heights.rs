//! Elevation tiles for 3D terrain: Terrarium-encoded PNGs (AWS Terrain
//! Tiles, free, worldwide, levels 0–15) decoded to metres on worker threads,
//! deduplicated, kept on disk and in memory, least-recently-used first out.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use tokio::sync::Semaphore;

/// Terrarium tiles by `{z}/{x}/{y}`.
pub const TERRARIUM_URL: &str = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium";
/// The deepest level published.
pub const MAX_LEVEL: u8 = 15;
const MAX_TILES: usize = 400;
const RETRY_AFTER: Duration = Duration::from_secs(20);

/// One tile's heights: `size × size` samples, row 0 = north, metres.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightTile {
    pub size: usize,
    pub h: Vec<f32>,
}

impl HeightTile {
    /// Height at (`u`, `v`) in 0..=1 across the tile, bilinear.
    pub fn sample(&self, u: f64, v: f64) -> f32 {
        let n = self.size;
        let fx = (u.clamp(0.0, 1.0) * (n - 1) as f64) as f32;
        let fy = (v.clamp(0.0, 1.0) * (n - 1) as f64) as f32;
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(n - 1), (y0 + 1).min(n - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let h = |x: usize, y: usize| self.h[y * n + x];
        let a = h(x0, y0) * (1.0 - tx) + h(x1, y0) * tx;
        let b = h(x0, y1) * (1.0 - tx) + h(x1, y1) * tx;
        a * (1.0 - ty) + b * ty
    }

    /// Flatten anything narrower than `2·radius + 1` samples that stands
    /// above its surroundings (a grey-scale opening: min filter, then max).
    /// Close up, the elevation data is a surface model: city blocks and
    /// tree stands show as lumps the photo gets smeared over.
    pub fn open(&mut self, radius: usize) {
        if radius == 0 || self.size == 0 {
            return;
        }
        let eroded = filter(&self.h, self.size, radius, f32::min);
        let opened = filter(&eroded, self.size, radius, f32::max);
        // Near the tile's edges the window is cut short and the opening
        // would sink the ground (trenches along every tile border): fade it
        // out toward them, so neighbouring tiles still meet.
        let n = self.size;
        let r = radius as f32;
        for y in 0..n {
            for x in 0..n {
                let edge = x.min(y).min(n - 1 - x).min(n - 1 - y) as f32;
                let w = ((edge - r) / r).clamp(0.0, 1.0);
                let k = y * n + x;
                self.h[k] += (opened[k] - self.h[k]) * w;
            }
        }
    }
}

/// Separable square min/max filter, clamped at the edges.
fn filter(src: &[f32], n: usize, r: usize, f: fn(f32, f32) -> f32) -> Vec<f32> {
    let pass = |src: &[f32], horizontal: bool| {
        let mut out = vec![0.0; n * n];
        for a in 0..n {
            for b in 0..n {
                let lo = b.saturating_sub(r);
                let hi = (b + r).min(n - 1);
                let mut v = src[if horizontal { a * n + lo } else { lo * n + a }];
                for c in lo + 1..=hi {
                    v = f(v, src[if horizontal { a * n + c } else { c * n + a }]);
                }
                out[if horizontal { a * n + b } else { b * n + a }] = v;
            }
        }
        out
    };
    pass(&pass(src, true), false)
}

/// The opening radius (samples) for a tile: features under ~200 m across are
/// flattened at levels 13 and deeper; coarser tiles are left alone.
pub fn spike_radius(z: u8, y: u32, size: usize) -> usize {
    if z < 13 || size == 0 {
        return 0;
    }
    let n = f64::from(1u32 << z);
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * (f64::from(y) + 0.5) / n))
        .sinh()
        .atan();
    let m_per_px = 40_075_016.7 * lat.cos() / (size as f64 * n);
    ((100.0 / m_per_px).ceil() as usize).min(32)
}

/// Decode a Terrarium PNG: `height = R·256 + G + B/256 − 32768` m (the sea
/// floor is clamped to sea level).
pub fn decode_terrarium(png: &[u8]) -> Result<HeightTile, String> {
    let img = image::load_from_memory(png)
        .map_err(|e| e.to_string())?
        .to_rgb8();
    let size = img.width() as usize;
    if size == 0 || img.height() as usize != size {
        return Err("terrain tile is not square".into());
    }
    let h = img
        .pixels()
        .map(|p| {
            let [r, g, b] = p.0;
            (f32::from(r) * 256.0 + f32::from(g) + f32::from(b) / 256.0 - 32768.0).max(0.0)
        })
        .collect();
    Ok(HeightTile { size, h })
}

enum State {
    Loading,
    Ready(Arc<HeightTile>),
    Failed(Instant),
}

type Key = (u8, u32, u32);

pub struct HeightCache {
    client: reqwest::Client,
    rt: tokio::runtime::Handle,
    disk: Option<PathBuf>,
    ctx: egui::Context,
    entries: HashMap<Key, (State, u64)>,
    tx: Sender<(Key, Result<HeightTile, String>)>,
    rx: Receiver<(Key, Result<HeightTile, String>)>,
    sem: Arc<Semaphore>,
    frame: u64,
}

impl HeightCache {
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
            entries: HashMap::new(),
            tx,
            rx,
            sem: Arc::new(Semaphore::new(8)),
            frame: 0,
        }
    }

    /// The tile if loaded, never fetching (marks it used).
    pub fn get(&mut self, z: u8, x: u32, y: u32) -> Option<Arc<HeightTile>> {
        let frame = self.frame;
        match self.entries.get_mut(&(z, x, y)) {
            Some((State::Ready(t), used)) => {
                *used = frame;
                Some(t.clone())
            }
            _ => None,
        }
    }

    /// The tile if loaded; otherwise start fetching it.
    pub fn request(&mut self, z: u8, x: u32, y: u32) -> Option<Arc<HeightTile>> {
        let key = (z, x, y);
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
        if fetch {
            self.entries.insert(key, (State::Loading, frame));
            let (client, tx, sem, ctx) = (
                self.client.clone(),
                self.tx.clone(),
                self.sem.clone(),
                self.ctx.clone(),
            );
            let url = format!("{TERRARIUM_URL}/{z}/{x}/{y}.png");
            let path = self
                .disk
                .as_ref()
                .map(|d| d.join(format!("terrarium-{z}-{x}-{y}.png")));
            self.rt.spawn(async move {
                let Ok(_permit) = sem.acquire().await else {
                    return;
                };
                let bytes = match path.as_ref().and_then(|p| std::fs::read(p).ok()) {
                    Some(b) => Ok(b),
                    None => match client.get(&url).send().await {
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
                };
                let result = match bytes {
                    Ok(b) => {
                        let decoded = tokio::task::spawn_blocking({
                            let b = b.clone();
                            move || {
                                decode_terrarium(&b).map(|mut t| {
                                    t.open(spike_radius(z, y, t.size));
                                    t
                                })
                            }
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
                let _ = tx.send((key, result));
                ctx.request_repaint();
            });
        }
        None
    }

    /// Collect finished downloads and evict old tiles. Call once per frame.
    pub fn poll(&mut self) {
        self.frame += 1;
        while let Ok((key, r)) = self.rx.try_recv() {
            let used = self.entries.get(&key).map_or(self.frame, |e| e.1);
            let state = match r {
                Ok(t) => State::Ready(Arc::new(t)),
                Err(e) => {
                    tracing::debug!("terrain tile {key:?}: {e}");
                    State::Failed(Instant::now())
                }
            };
            self.entries.insert(key, (state, used));
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

    /// Tiles loading right now.
    pub fn loading(&self) -> usize {
        self.entries
            .values()
            .filter(|e| matches!(e.0, State::Loading))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_terrarium_heights() {
        // 1000 m = R 131, G 232; the sea floor reads as sea level.
        let mut img = image::RgbImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgb([131, 232, 0]));
        img.put_pixel(1, 0, image::Rgb([120, 0, 0]));
        img.put_pixel(0, 1, image::Rgb([128, 0, 128]));
        img.put_pixel(1, 1, image::Rgb([131, 232, 0]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let t = decode_terrarium(&png).unwrap();
        assert_eq!(t.size, 2);
        assert!((t.h[0] - 1000.0).abs() < 0.01);
        assert_eq!(t.h[1], 0.0);
        assert!((t.h[2] - 0.5).abs() < 0.01);
        // Halfway between 1000 and 0 along the top row.
        assert!((t.sample(0.5, 0.0) - 500.0).abs() < 0.1);
    }

    #[test]
    fn opening_flattens_lumps_but_keeps_slopes() {
        let n = 32;
        // A gentle slope with a 3-sample "building" on it.
        let mut h: Vec<f32> = (0..n * n).map(|i| (i % n) as f32).collect();
        for y in 10..13 {
            for x in 10..13 {
                h[y * n + x] += 50.0;
            }
        }
        let mut t = HeightTile { size: n, h };
        t.open(3);
        assert!(
            (t.h[11 * n + 11] - 11.0).abs() < 4.0,
            "{}",
            t.h[11 * n + 11]
        );
        // The slope away from the lump is untouched.
        assert_eq!(t.h[25 * n + 20], 20.0);
        assert_eq!(spike_radius(12, 0, 256), 0);
        assert!(spike_radius(15, 1 << 14, 256) >= 10);
    }
}
