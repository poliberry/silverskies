//! Motion in the Material 3 Expressive style: physics-based springs rather
//! than fixed-duration easing. Two springs, as in Material's motion scheme:
//!
//! * **spatial** (position, size, shape): slightly under-damped, so things
//!   overshoot a touch and settle — the "expressive" bounce;
//! * **effects** (colour, opacity): critically damped, no overshoot.
//!
//! Everything goes through here so "Reduce motion" can turn it all off.

use std::cell::Cell;

use egui::{Context, Id};

thread_local! {
    static REDUCED: Cell<bool> = const { Cell::new(false) };
}

/// Set once per frame from the user's preference.
pub fn set_reduced(on: bool) {
    REDUCED.with(|r| r.set(on));
}

pub fn reduced() -> bool {
    REDUCED.with(Cell::get)
}

/// Spatial spring response at `t` seconds after start (0 → 1, overshooting
/// by about 4% and settling within ~0.55 s).
pub fn spatial(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    let (zeta, omega) = (0.72_f32, 13.0_f32);
    let wd = omega * (1.0 - zeta * zeta).sqrt();
    let decay = (-zeta * omega * t).exp();
    1.0 - decay * ((wd * t).cos() + (zeta * omega / wd) * (wd * t).sin())
}

/// Effects spring response (critically damped, no overshoot).
pub fn effects(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    let omega = 14.0_f32;
    1.0 - (-omega * t).exp() * (1.0 + omega * t)
}

/// How long the springs take to be visually settled.
const SETTLE: f32 = 0.9;

/// Seconds since `key` last changed for `id` (a new key restarts the clock).
fn since(ctx: &Context, id: Id, key: u64) -> f32 {
    let now = ctx.input(|i| i.time);
    let start = ctx.data_mut(|d| {
        let slot = d.get_temp_mut_or(id, (key, now));
        if slot.0 != key {
            *slot = (key, now);
        }
        slot.1
    });
    (now - start) as f32
}

/// An entrance: spatial progress (0 → ~1, with overshoot) and opacity
/// (0 → 1) for something that appears when `key` changes, after `delay`
/// seconds (for staggering).
pub fn enter(ctx: &Context, id: Id, key: u64, delay: f32) -> (f32, f32) {
    if reduced() {
        return (1.0, 1.0);
    }
    let t = since(ctx, id, key) - delay;
    if t < SETTLE {
        ctx.request_repaint();
    }
    (spatial(t), effects(t).clamp(0.0, 1.0))
}

/// A state change (selected, hovered, open…): 0 → 1 as `on` turns true and
/// back, shaped by the spatial spring on the way in.
pub fn toggle(ctx: &Context, id: Id, on: bool, seconds: f32) -> f32 {
    if reduced() {
        return if on { 1.0 } else { 0.0 };
    }
    let lin = ctx.animate_bool_with_time(id, on, seconds);
    // Map linear progress through the spring curve (scaled to its settle time).
    if on {
        spatial(lin * SETTLE * 0.7).max(0.0)
    } else {
        effects(lin * SETTLE * 0.7).clamp(0.0, 1.0)
    }
}

/// A smooth, non-bouncy fade for hover state layers.
pub fn fade(ctx: &Context, id: Id, on: bool) -> f32 {
    if reduced() {
        return if on { 1.0 } else { 0.0 };
    }
    ctx.animate_bool_with_time(id, on, 0.15)
}

/// A number that glides to its new value (temperatures counting up/down).
pub fn value(ctx: &Context, id: Id, target: f32) -> f32 {
    if reduced() {
        return target;
    }
    ctx.animate_value_with_time(id, target, 0.6)
}

/// A stable key for "this content": hash anything hashable.
pub fn key(parts: impl std::hash::Hash) -> u64 {
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
    BuildHasherDefault::<DefaultHasher>::default().hash_one(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_spring_overshoots_slightly_then_settles() {
        assert_eq!(spatial(0.0), 0.0);
        let peak = (1..200)
            .map(|i| spatial(i as f32 / 200.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.01 && peak < 1.08, "peak {peak}");
        assert!((spatial(SETTLE) - 1.0).abs() < 0.01);
    }

    #[test]
    fn effects_spring_never_overshoots() {
        let mut last = 0.0;
        for i in 0..200 {
            let v = effects(i as f32 / 100.0);
            assert!(v >= last - 1e-6 && v <= 1.0 + 1e-6);
            last = v;
        }
        assert!(effects(SETTLE) > 0.99);
    }

    #[test]
    fn reduced_motion_is_immediate() {
        set_reduced(true);
        let ctx = Context::default();
        assert_eq!(enter(&ctx, Id::NULL, 1, 0.0), (1.0, 1.0));
        assert_eq!(toggle(&ctx, Id::NULL, true, 0.3), 1.0);
        assert_eq!(value(&ctx, Id::NULL, 42.0), 42.0);
        set_reduced(false);
    }

    #[test]
    fn keys_are_stable() {
        assert_eq!(key(("a", 1)), key(("a", 1)));
        assert_ne!(key(("a", 1)), key(("b", 1)));
    }
}
