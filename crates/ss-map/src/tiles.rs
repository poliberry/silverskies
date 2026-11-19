//! Async tile/image cache: network + disk fetch, decode on workers, texture upload on the UI thread.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use egui::{ColorImage, TextureHandle, TextureOptions};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

/// Maximum decoded textures kept in memory.
pub const MAX_TEXTURES: usize = 512;
/// Concurrent fetches for what is on screen now (basemap, overlays, the
/// radar frame being shown).
pub const MAX_CONCURRENT: usize = 10;
/// Concurrent fetches for background work (radar frames ahead of the
/// playhead). A separate pool, so prefetching never delays visible tiles.
pub const MAX_BACKGROUND: usize = 4;
/// A queued fetch that nothing has asked for in this many frames is dropped
/// when its turn comes (the map moved on); asking again re-queues it.
pub const STALE_FRAMES: u64 = 30;

/// How urgently a tile is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority {
    /// On screen now.
    #[default]
    Visible,
    /// Wanted soon (prefetch).
    Background,
}
/// Failed tiles are retried after this long.
pub const RETRY_AFTER: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TileKey {
    /// Unique layer key, including the URL template identity (and frame time for radar).
    pub layer: String,
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl TileKey {
    pub fn new(layer: impl Into<String>, z: u8, x: u32, y: u32) -> Self {
        Self {
            layer: layer.into(),
            z,
            x,
            y,
        }
    }
    /// Key for a free-standing image (WMS etc.).
    pub fn image(key: impl Into<String>) -> Self {
        Self::new(key, 0, 0, 0)
    }
}

#[derive(Clone)]
pub enum TileState {
    Loading,
    Ready(TextureHandle),
    Failed { at: Instant },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TileStats {
    pub ready: usize,
    pub loading: usize,
    pub failed: usize,
}

struct Entry {
    state: TileState,
    last_used: u64,
    /// Last frame a `request` asked for this tile (shared with its fetch, so
    /// a queued fetch can tell that nobody wants it any more).
    wanted: Arc<AtomicU64>,
    priority: Priority,
}

/// Why a fetch produced no image.
#[derive(Debug)]
enum Fetched {
    Image(ColorImage),
    Failed,
    /// Dropped from the queue because nothing asked for it lately.
    Stale,
}

type Decoded = (TileKey, Fetched);

pub struct TileCache {
    client: reqwest::Client,
    rt: tokio::runtime::Handle,
    disk_dir: Option<PathBuf>,
    ctx: egui::Context,
    entries: HashMap<TileKey, Entry>,
    tx: Sender<Decoded>,
    rx: Receiver<Decoded>,
    sem: Arc<Semaphore>,
    background: Arc<Semaphore>,
    frame: u64,
    /// `frame`, readable from fetch tasks.
    now: Arc<AtomicU64>,
    max_textures: usize,
}

impl TileCache {
    pub fn new(
        client: reqwest::Client,
        rt: tokio::runtime::Handle,
        disk_dir: Option<PathBuf>,
        egui_ctx: egui::Context,
    ) -> Self {
        if let Some(d) = &disk_dir {
            if let Err(e) = std::fs::create_dir_all(d) {
                tracing::warn!("tile disk cache dir {d:?} unavailable: {e}");
            }
        }
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            client,
            rt,
            disk_dir,
            ctx: egui_ctx,
            entries: HashMap::new(),
            tx,
            rx,
            sem: Arc::new(Semaphore::new(MAX_CONCURRENT)),
            background: Arc::new(Semaphore::new(MAX_BACKGROUND)),
            frame: 0,
            now: Arc::new(AtomicU64::new(0)),
            max_textures: MAX_TEXTURES,
        }
    }

    /// Texture if ready; otherwise enqueue a fetch (deduplicated) and return `None`.
    /// Results are persisted on disk (if configured).
    pub fn request(&mut self, key: &TileKey, url: String) -> Option<&TextureHandle> {
        self.request_inner(key, url, true, Priority::Visible)
    }

    /// Like `request`, at the given priority. A tile queued in the background
    /// that becomes visible is promoted (re-queued in the visible pool).
    pub fn request_with(
        &mut self,
        key: &TileKey,
        url: String,
        priority: Priority,
    ) -> Option<&TextureHandle> {
        self.request_inner(key, url, true, priority)
    }

    /// Like `request` but for a single free-standing image (no tile coordinates) that is
    /// not persisted to disk (e.g. WMS GetMap).
    pub fn request_image(&mut self, key: &str, url: &str) -> Option<&TextureHandle> {
        self.request_inner(
            &TileKey::image(key),
            url.to_string(),
            false,
            Priority::Visible,
        )
    }

    /// Texture if already ready, never fetches. Marks it as recently used.
    pub fn get_ready(&mut self, key: &TileKey) -> Option<&TextureHandle> {
        let frame = self.frame;
        match self.entries.get_mut(key) {
            Some(e) => {
                e.last_used = frame;
                match &e.state {
                    TileState::Ready(t) => Some(t),
                    _ => None,
                }
            }
            None => None,
        }
    }

    fn request_inner(
        &mut self,
        key: &TileKey,
        url: String,
        use_disk: bool,
        priority: Priority,
    ) -> Option<&TextureHandle> {
        let frame = self.frame;
        let mut shared_wanted = None;
        let needs_fetch = match self.entries.get_mut(key) {
            Some(e) => {
                shared_wanted = Some(e.wanted.clone());
                e.last_used = frame;
                e.wanted.store(frame, Ordering::Relaxed);
                match &e.state {
                    TileState::Ready(_) => false,
                    // Waiting in the background pool but needed now: queue
                    // it again in the visible pool (first result wins).
                    TileState::Loading => {
                        priority == Priority::Visible && e.priority == Priority::Background
                    }
                    TileState::Failed { at } => at.elapsed() >= RETRY_AFTER,
                }
            }
            None => true,
        };
        if needs_fetch {
            // A promoted fetch shares the queued one's "wanted" clock, so
            // neither looks stale while the tile is still asked for.
            let wanted = shared_wanted
                .filter(|_| {
                    matches!(
                        self.entries.get(key).map(|e| &e.state),
                        Some(TileState::Loading)
                    )
                })
                .unwrap_or_else(|| Arc::new(AtomicU64::new(frame)));
            self.entries.insert(
                key.clone(),
                Entry {
                    state: TileState::Loading,
                    last_used: frame,
                    wanted: wanted.clone(),
                    priority,
                },
            );
            self.spawn_fetch(key.clone(), url, use_disk, priority, wanted);
        }
        match self.entries.get(key) {
            Some(Entry {
                state: TileState::Ready(t),
                ..
            }) => Some(t),
            _ => None,
        }
    }

    fn spawn_fetch(
        &self,
        key: TileKey,
        url: String,
        use_disk: bool,
        priority: Priority,
        wanted: Arc<AtomicU64>,
    ) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        let sem = match priority {
            Priority::Visible => self.sem.clone(),
            Priority::Background => self.background.clone(),
        };
        let now = self.now.clone();
        let ctx = self.ctx.clone();
        let disk = if use_disk {
            self.disk_dir.clone()
        } else {
            None
        };
        self.rt.spawn(async move {
            let Ok(_permit) = sem.acquire().await else {
                return;
            };
            // Our turn: skip it if the map has moved on.
            let idle = now
                .load(Ordering::Relaxed)
                .saturating_sub(wanted.load(Ordering::Relaxed));
            let result = if idle > STALE_FRAMES {
                Fetched::Stale
            } else {
                match fetch_and_decode(&client, &url, disk).await {
                    Ok(img) => Fetched::Image(img),
                    Err(e) => {
                        tracing::debug!("tile fetch failed {url}: {e}");
                        Fetched::Failed
                    }
                }
            };
            let _ = tx.send((key, result));
            ctx.request_repaint();
        });
    }

    /// Drain finished downloads, upload textures, evict least-recently-used. Call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        let frame = self.frame;
        self.now.store(frame, Ordering::Relaxed);
        while let Ok((key, res)) = self.rx.try_recv() {
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            // A promoted tile has two fetches; keep the first image.
            if matches!(entry.state, TileState::Ready(_)) {
                continue;
            }
            match res {
                Fetched::Image(img) => {
                    let name = format!("{}/{}/{}/{}", key.layer, key.z, key.x, key.y);
                    let tex = ctx.load_texture(name, img, TextureOptions::LINEAR);
                    entry.state = TileState::Ready(tex);
                    entry.last_used = frame;
                }
                Fetched::Failed => entry.state = TileState::Failed { at: Instant::now() },
                Fetched::Stale => {
                    // Forget it; asking again re-queues it.
                    self.entries.remove(&key);
                }
            }
        }
        self.evict();
    }

    fn evict(&mut self) {
        let ready = self
            .entries
            .values()
            .filter(|e| matches!(e.state, TileState::Ready(_)))
            .count();
        if ready <= self.max_textures {
            return;
        }
        let mut cand: Vec<(u64, TileKey)> = self
            .entries
            .iter()
            .filter(|(_, e)| matches!(e.state, TileState::Ready(_)) && e.last_used + 1 < self.frame)
            .map(|(k, e)| (e.last_used, k.clone()))
            .collect();
        cand.sort_by_key(|(u, _)| *u);
        for (_, k) in cand.into_iter().take(ready - self.max_textures) {
            self.entries.remove(&k);
        }
    }

    /// Drop an entry so the next request re-fetches.
    pub fn invalidate(&mut self, key: &TileKey) {
        self.entries.remove(key);
    }

    pub fn stats(&self) -> TileStats {
        let mut s = TileStats::default();
        for e in self.entries.values() {
            match e.state {
                TileState::Ready(_) => s.ready += 1,
                TileState::Loading => s.loading += 1,
                TileState::Failed { .. } => s.failed += 1,
            }
        }
        s
    }
}

pub fn url_hash(url: &str) -> String {
    let d = Sha256::digest(url.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

async fn fetch_and_decode(
    client: &reqwest::Client,
    url: &str,
    disk: Option<PathBuf>,
) -> Result<ColorImage, String> {
    let path = disk.map(|d| d.join(format!("{}.bin", url_hash(url))));

    let mut bytes: Option<Vec<u8>> = None;
    if let Some(p) = &path {
        if let Ok(b) = tokio::fs::read(p).await {
            bytes = Some(b);
        }
    }
    let from_disk = bytes.is_some();
    if bytes.is_none() {
        let started = Instant::now();
        let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
        tracing::debug!(url, ms = started.elapsed().as_millis() as u64, "tile");
        let resp = resp.error_for_status().map_err(|e| e.to_string())?;
        bytes = Some(resp.bytes().await.map_err(|e| e.to_string())?.to_vec());
    }
    let bytes = bytes.unwrap_or_default();

    let decode_bytes = bytes.clone();
    let decoded = tokio::task::spawn_blocking(move || decode(&decode_bytes))
        .await
        .map_err(|e| e.to_string())?;
    match decoded {
        Ok(img) => {
            if !from_disk {
                if let Some(p) = &path {
                    // Only persist bytes that decoded successfully.
                    let _ = tokio::fs::write(p, &bytes).await;
                }
            }
            Ok(img)
        }
        Err(e) => {
            if from_disk {
                if let Some(p) = &path {
                    let _ = tokio::fs::remove_file(p).await;
                }
            }
            Err(e)
        }
    }
}

pub fn decode(bytes: &[u8]) -> Result<ColorImage, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_hex() {
        let h = url_hash("https://example.com/1/2/3.png");
        assert_eq!(h.len(), 64);
        assert_eq!(h, url_hash("https://example.com/1/2/3.png"));
        assert_ne!(h, url_hash("https://example.com/1/2/4.png"));
    }

    #[test]
    fn decode_png_roundtrip() {
        let mut buf = Vec::new();
        let img = image::RgbaImage::from_pixel(2, 3, image::Rgba([10, 20, 30, 255]));
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let ci = decode(&buf).unwrap();
        assert_eq!(ci.size, [2, 3]);
        assert!(decode(b"garbage").is_err());
    }

    #[test]
    fn unwanted_queued_fetches_are_dropped() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let ctx = egui::Context::default();
        let mut c = TileCache::new(
            reqwest::Client::new(),
            rt.handle().clone(),
            None,
            ctx.clone(),
        );
        // Occupy every background slot so the next fetch has to queue.
        let hold = rt.block_on(async {
            let mut v = Vec::new();
            for _ in 0..MAX_BACKGROUND {
                v.push(c.background.clone().acquire_owned().await.unwrap());
            }
            v
        });
        let key = TileKey::new("t", 3, 1, 1);
        c.request_with(
            &key,
            "http://127.0.0.1:9/x.png".into(),
            Priority::Background,
        );
        // The map moves on: many frames pass without anyone asking for it.
        for _ in 0..=STALE_FRAMES + 1 {
            c.poll(&ctx);
        }
        drop(hold);
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(20));
            c.poll(&ctx);
            if c.stats() == TileStats::default() {
                break;
            }
        }
        assert_eq!(c.stats(), TileStats::default(), "stale fetch was dropped");
    }

    #[test]
    fn background_tiles_are_promoted_when_visible() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let ctx = egui::Context::default();
        let mut c = TileCache::new(
            reqwest::Client::new(),
            rt.handle().clone(),
            None,
            ctx.clone(),
        );
        let key = TileKey::new("t", 3, 1, 1);
        c.request_with(
            &key,
            "http://127.0.0.1:9/x.png".into(),
            Priority::Background,
        );
        assert_eq!(c.entries[&key].priority, Priority::Background);
        c.request(&key, "http://127.0.0.1:9/x.png".into());
        assert_eq!(c.entries[&key].priority, Priority::Visible);
        assert_eq!(c.stats().loading, 1, "still one entry");
    }

    #[test]
    fn dedupes_and_reports_failure() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let ctx = egui::Context::default();
        let mut c = TileCache::new(
            reqwest::Client::new(),
            rt.handle().clone(),
            None,
            ctx.clone(),
        );
        let key = TileKey::new("t", 1, 0, 0);
        assert!(c.request(&key, "http://127.0.0.1:9/x.png".into()).is_none());
        assert!(c.request(&key, "http://127.0.0.1:9/x.png".into()).is_none());
        assert_eq!(c.stats().loading, 1);
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            c.poll(&ctx);
            if c.stats().failed == 1 {
                break;
            }
        }
        assert_eq!(
            c.stats(),
            TileStats {
                ready: 0,
                loading: 0,
                failed: 1
            }
        );
    }
}
