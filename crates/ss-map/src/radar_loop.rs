//! A fluid radar loop: every frame of the loop lives in one layer, which
//! preloads the frames' tiles, crossfades between neighbouring frames, and
//! holds the last ready image of a tile instead of flashing to blank while
//! the next frame downloads. [`LoopClock`] drives playback: it holds each
//! frame, eases into the next, and waits (briefly) for a frame that is still
//! loading rather than showing it half-drawn.

use std::any::Any;
use std::sync::Arc;

use egui::{pos2, Color32, Painter, Rect};

use crate::layers::{DrawCtx, Layer};
use crate::tiles::TileKey;
use crate::viewport::TileId;

/// Builds the tile URL for one frame.
pub type TileUrlFn = Arc<dyn Fn(TileId) -> String + Send + Sync>;

/// One frame of the loop: a cache key unique to its imagery, and its URLs.
#[derive(Clone)]
pub struct LoopFrameSource {
    pub key: String,
    pub url: TileUrlFn,
}

/// Prefetch every frame when that's at most this many tile requests;
/// otherwise only a window ahead of the playhead.
const PREFETCH_ALL_BUDGET: usize = 360;
/// Frames fetched ahead of the playhead when not prefetching everything.
const LOOKAHEAD: usize = 4;

pub struct RadarLoopLayer {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub max_zoom: u8,
    frames: Vec<LoopFrameSource>,
    /// The playhead: frame index plus the crossfade into the next frame.
    pub pos: f32,
    /// Share (0..=1) of the visible tiles loaded, per frame, as of the last draw.
    ready: Vec<f32>,
}

impl RadarLoopLayer {
    pub fn new(name: impl Into<String>, max_zoom: u8, opacity: f32) -> Self {
        Self {
            name: name.into(),
            visible: true,
            opacity,
            max_zoom,
            frames: Vec::new(),
            pos: 0.0,
            ready: Vec::new(),
        }
    }

    /// Replace the frames (no-op when the keys are unchanged, so it can be
    /// called every frame).
    pub fn set_frames(&mut self, frames: Vec<LoopFrameSource>) {
        let same = frames.len() == self.frames.len()
            && frames.iter().zip(&self.frames).all(|(a, b)| a.key == b.key);
        if !same {
            self.ready = vec![0.0; frames.len()];
            self.frames = frames;
        }
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Share of frame `i`'s visible tiles that are loaded (0..=1).
    pub fn ready(&self, i: usize) -> f32 {
        self.ready.get(i).copied().unwrap_or(0.0)
    }

    fn frames_to_fetch(&self, tiles: usize) -> Vec<usize> {
        let n = self.frames.len();
        if n == 0 {
            return Vec::new();
        }
        if n * tiles <= PREFETCH_ALL_BUDGET {
            return (0..n).collect();
        }
        let i0 = self.pos.floor().max(0.0) as usize % n;
        (0..=LOOKAHEAD).map(|k| (i0 + k) % n).collect()
    }
}

/// Smooth ease for crossfades.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Layer for RadarLoopLayer {
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
        let n = self.frames.len();
        if n == 0 {
            return;
        }
        let tiles = ctx
            .viewport
            .visible_tiles_with_rects(ctx.rect, self.max_zoom);
        if tiles.is_empty() {
            return;
        }

        // Prefetch, counting how much of each frame is ready.
        let fetch = self.frames_to_fetch(tiles.len());
        for &fi in &fetch {
            let f = &self.frames[fi];
            let mut ready = 0usize;
            for (tile, _) in &tiles {
                let key = TileKey::new(f.key.clone(), tile.z, tile.x, tile.y);
                if ctx.cache.request(&key, (f.url)(*tile)).is_some() {
                    ready += 1;
                }
            }
            if let Some(r) = self.ready.get_mut(fi) {
                *r = ready as f32 / tiles.len() as f32;
            }
        }

        let pos = self.pos.clamp(0.0, (n - 1) as f32 + 0.999);
        let i0 = pos.floor() as usize;
        let i1 = (i0 + 1).min(n - 1);
        let fade = if i1 == i0 {
            0.0
        } else {
            smoothstep(pos - i0 as f32)
        };
        let full_uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let opacity = self.opacity.clamp(0.0, 1.0);
        let tint = |a: f32| Color32::WHITE.linear_multiply(opacity * a.clamp(0.0, 1.0));

        for (tile, r) in &tiles {
            let r = Rect::from_min_max(r.min.round(), r.max.round());
            let key_of = |fi: usize, z: u8, x: u32, y: u32| {
                TileKey::new(self.frames[fi].key.clone(), z, x, y)
            };
            // The image under the playhead, else the newest earlier frame of
            // this tile that is ready (hold, don't blank), else its parent.
            let mut base = None;
            for back in 0..n {
                let fi = (i0 + n - back) % n;
                if let Some(tex) = ctx.cache.get_ready(&key_of(fi, tile.z, tile.x, tile.y)) {
                    base = Some((tex.id(), full_uv));
                    break;
                }
            }
            if base.is_none() && tile.z > 0 {
                let pk = key_of(i0, tile.z - 1, tile.x / 2, tile.y / 2);
                if let Some(tex) = ctx.cache.get_ready(&pk) {
                    let (qx, qy) = ((tile.x & 1) as f32 * 0.5, (tile.y & 1) as f32 * 0.5);
                    base = Some((
                        tex.id(),
                        Rect::from_min_max(pos2(qx, qy), pos2(qx + 0.5, qy + 0.5)),
                    ));
                }
            }
            let next = (fade > 0.0)
                .then(|| ctx.cache.get_ready(&key_of(i1, tile.z, tile.x, tile.y)))
                .flatten()
                .map(|t| t.id());
            match (base, next) {
                (Some((tex, uv)), Some(nt)) => {
                    painter.image(tex, r, uv, tint(1.0 - fade));
                    painter.image(nt, r, full_uv, tint(fade));
                }
                (Some((tex, uv)), None) => {
                    painter.image(tex, r, uv, tint(1.0));
                }
                (None, Some(nt)) => {
                    painter.image(nt, r, full_uv, tint(fade));
                }
                (None, None) => {}
            }
        }
    }
}

/// Playback timing for a loop: hold each frame, then ease into the next
/// over the last part of the step; hold the live frame longer; wait (up to
/// `max_wait`) for the next frame to load.
#[derive(Debug, Clone)]
pub struct LoopClock {
    pub playing: bool,
    /// Seconds per frame (hold + crossfade).
    pub step: f32,
    /// Seconds of each step spent crossfading into the next frame.
    pub fade: f32,
    /// Seconds the live (newest observed) frame is held before looping.
    pub live_hold: f32,
    /// Longest wait for a still-loading frame before moving on anyway.
    pub max_wait: f32,
    /// Current frame.
    pub idx: usize,
    elapsed: f32,
}

impl Default for LoopClock {
    fn default() -> Self {
        Self {
            playing: true,
            step: 0.7,
            fade: 0.3,
            live_hold: 1.8,
            max_wait: 2.5,
            idx: 0,
            elapsed: 0.0,
        }
    }
}

impl LoopClock {
    /// A clock with the given timings (seconds), playing from frame 0.
    pub fn with_timing(step: f32, fade: f32, live_hold: f32, max_wait: f32) -> Self {
        Self {
            step,
            fade,
            live_hold,
            max_wait,
            ..Default::default()
        }
    }

    /// Advance by `dt` seconds through a loop of `n` frames whose newest
    /// observed frame is `live`; `next_ready` is how loaded the next frame
    /// is (0..=1). Returns the playhead for [`RadarLoopLayer::pos`].
    pub fn tick(&mut self, dt: f32, n: usize, live: usize, next_ready: f32) -> f32 {
        if n == 0 {
            self.idx = 0;
            return 0.0;
        }
        self.idx = self.idx.min(n - 1);
        if !self.playing || n == 1 {
            self.elapsed = 0.0;
            return self.idx as f32;
        }
        self.elapsed += dt.max(0.0);
        let wraps = self.idx + 1 >= n;
        let hold = if self.idx == live || wraps {
            self.live_hold.max(self.step)
        } else {
            self.step
        };
        let ready = next_ready >= 0.8;
        // Hold at the end of the step while the next frame loads.
        let limit = if ready { hold } else { hold + self.max_wait };
        if self.elapsed >= limit || (ready && self.elapsed >= hold) {
            self.idx = if wraps { 0 } else { self.idx + 1 };
            self.elapsed = 0.0;
            return self.idx as f32;
        }
        // No crossfade on the wrap back to the oldest frame (a cut reads as
        // "the loop restarts").
        if wraps {
            return self.idx as f32;
        }
        let fade_start = (hold - self.fade).max(0.0);
        let t = if ready || self.elapsed < hold {
            ((self.elapsed - fade_start) / self.fade.max(1e-3)).clamp(0.0, 0.999)
        } else {
            0.0
        };
        self.idx as f32 + t
    }

    /// Jump to a frame (scrubbing, stepping, going live).
    pub fn set(&mut self, idx: usize) {
        self.idx = idx;
        self.elapsed = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_holds_then_crossfades_then_steps() {
        let mut c = LoopClock {
            step: 1.0,
            fade: 0.4,
            live_hold: 2.0,
            ..Default::default()
        };
        // Frame 0 of 5, live is 3, next frame fully loaded.
        assert_eq!(c.tick(0.3, 5, 3, 1.0), 0.0);
        let mid = c.tick(0.5, 5, 3, 1.0); // 0.8 s: halfway through the fade
        assert!(mid > 0.4 && mid < 0.6, "{mid}");
        assert_eq!(c.tick(0.3, 5, 3, 1.0), 1.0); // 1.1 s: stepped
    }

    #[test]
    fn clock_waits_for_a_loading_frame_but_not_forever() {
        let mut c = LoopClock {
            step: 1.0,
            fade: 0.4,
            max_wait: 2.0,
            ..Default::default()
        };
        assert_eq!(c.tick(1.5, 5, 4, 0.1), 0.0); // not ready: hold, no fade
        assert_eq!(c.tick(1.0, 5, 4, 0.1), 0.0);
        assert_eq!(c.tick(0.6, 5, 4, 0.1), 1.0); // past step + max_wait
    }

    #[test]
    fn clock_holds_live_longer_and_wraps_with_a_cut() {
        let mut c = LoopClock {
            step: 1.0,
            fade: 0.4,
            live_hold: 2.0,
            idx: 2,
            ..Default::default()
        };
        assert_eq!(c.tick(1.5, 4, 2, 1.0), 2.0); // live: still holding at 1.5 s
        assert!(c.tick(0.4, 4, 2, 1.0) > 2.0); // fading into nowcast
        c.set(3);
        assert_eq!(c.tick(1.9, 4, 2, 1.0), 3.0); // last frame: no fade into the wrap
        assert_eq!(c.tick(0.2, 4, 2, 1.0), 0.0); // wrapped
    }

    #[test]
    fn paused_clock_sits_on_its_frame() {
        let mut c = LoopClock {
            playing: false,
            idx: 3,
            ..Default::default()
        };
        assert_eq!(c.tick(5.0, 6, 5, 1.0), 3.0);
        assert_eq!(c.tick(1.0, 0, 0, 1.0), 0.0);
    }

    #[test]
    fn set_frames_keeps_state_for_the_same_keys() {
        let mk = |k: &str| LoopFrameSource {
            key: k.into(),
            url: Arc::new(|_| String::new()),
        };
        let mut l = RadarLoopLayer::new("radar", 12, 0.8);
        l.set_frames(vec![mk("a"), mk("b")]);
        l.ready[1] = 1.0;
        l.set_frames(vec![mk("a"), mk("b")]);
        assert_eq!(l.ready(1), 1.0);
        l.set_frames(vec![mk("a"), mk("c")]);
        assert_eq!(l.ready(1), 0.0);
        assert_eq!(l.len(), 2);
    }
}
