//! An easter egg. Click the logo ten times.
//!
//! Plays `easter-egg.mid` from the settings folder if there is one (else a
//! short original loop), forever, until the app closes: through Windows'
//! built-in General MIDI synth (its GS soundfont) on Windows, or a small
//! software synth elsewhere.

use std::path::Path;

/// The picture.
pub const IMAGE: &[u8] = include_bytes!("../assets/easter-egg.png");

/// Clicks on the logo that start it.
pub const CLICKS: u32 = 10;

/// One timed MIDI channel message: seconds from the start, status, data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ev {
    pub t: f64,
    pub status: u8,
    pub d1: u8,
    pub d2: u8,
}

fn varint(b: &[u8], i: &mut usize) -> Option<u32> {
    let mut v = 0u32;
    for _ in 0..4 {
        let c = *b.get(*i)?;
        *i += 1;
        v = (v << 7) | u32::from(c & 0x7f);
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
    Some(v)
}

/// Read a Standard MIDI File (format 0 or 1) into channel messages in time
/// order, applying the tempo map.
pub fn parse_midi(b: &[u8]) -> Option<Vec<Ev>> {
    if b.get(0..4)? != b"MThd" {
        return None;
    }
    let division = u16::from_be_bytes([*b.get(12)?, *b.get(13)?]);
    if division & 0x8000 != 0 || division == 0 {
        return None; // SMPTE time isn't used by song files
    }
    let ppq = f64::from(division);
    let hlen = u32::from_be_bytes(b.get(4..8)?.try_into().ok()?) as usize;
    let mut i = 8 + hlen;
    // (tick, order, status, d1, d2) for channel messages; tempo changes as
    // status 0xFF with the microseconds in d1/d2 via a side list.
    let mut evs: Vec<(u64, usize, u8, u8, u8)> = Vec::new();
    let mut tempos: Vec<(u64, u32)> = Vec::new();
    let mut order = 0usize;
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes(b[i + 4..i + 8].try_into().ok()?) as usize;
        let (start, end) = (i + 8, (i + 8 + len).min(b.len()));
        if &b[i..i + 4] == b"MTrk" {
            let tr = &b[start..end];
            let (mut p, mut tick, mut running) = (0usize, 0u64, 0u8);
            while p < tr.len() {
                tick += u64::from(varint(tr, &mut p)?);
                let mut st = *tr.get(p)?;
                if st & 0x80 != 0 {
                    p += 1;
                } else {
                    st = running;
                }
                match st {
                    0xFF => {
                        let kind = *tr.get(p)?;
                        p += 1;
                        let l = varint(tr, &mut p)? as usize;
                        if kind == 0x51 && l == 3 {
                            let us = u32::from_be_bytes([0, tr[p], tr[p + 1], tr[p + 2]]);
                            tempos.push((tick, us));
                        }
                        if kind == 0x2F {
                            break;
                        }
                        p += l;
                    }
                    0xF0 | 0xF7 => {
                        let l = varint(tr, &mut p)? as usize;
                        p += l;
                    }
                    s if s >= 0x80 => {
                        running = s;
                        let two = !matches!(s & 0xF0, 0xC0 | 0xD0);
                        let d1 = *tr.get(p)?;
                        let d2 = if two { *tr.get(p + 1)? } else { 0 };
                        p += if two { 2 } else { 1 };
                        evs.push((tick, order, s, d1, d2));
                        order += 1;
                    }
                    _ => break,
                }
            }
        }
        i = end;
    }
    evs.sort_by_key(|e| (e.0, e.1));
    tempos.sort_by_key(|t| t.0);
    // Ticks to seconds through the tempo map.
    let mut out = Vec::with_capacity(evs.len());
    let (mut ti, mut last_tick, mut secs, mut us_per_q) = (0usize, 0u64, 0f64, 500_000u32);
    for (tick, _, s, d1, d2) in evs {
        while ti < tempos.len() && tempos[ti].0 <= tick {
            secs += (tempos[ti].0 - last_tick) as f64 * f64::from(us_per_q) / 1e6 / ppq;
            last_tick = tempos[ti].0;
            us_per_q = tempos[ti].1;
            ti += 1;
        }
        let t = secs + (tick - last_tick) as f64 * f64::from(us_per_q) / 1e6 / ppq;
        out.push(Ev {
            t,
            status: s,
            d1,
            d2,
        });
    }
    Some(out)
}

/// An original loop (not any existing song) for when there's no file:
/// 140 bpm, a minor riff over a half-time beat.
pub fn fallback_song() -> Vec<Ev> {
    let beat = 60.0 / 140.0;
    let s16 = beat / 4.0;
    let mut v = Vec::new();
    let mut note = |t: f64, len: f64, ch: u8, n: u8, vel: u8| {
        v.push(Ev {
            t,
            status: 0x90 | ch,
            d1: n,
            d2: vel,
        });
        v.push(Ev {
            t: t + len,
            status: 0x80 | ch,
            d1: n,
            d2: 0,
        });
    };
    // Lead on ch 0 (square), bass on ch 1, drums on ch 9.
    let riff: [(u8, u8); 16] = [
        (62, 2),
        (65, 1),
        (69, 1),
        (67, 2),
        (65, 2),
        (64, 1),
        (65, 1),
        (62, 2),
        (60, 1),
        (62, 1),
        (57, 2),
        (60, 2),
        (62, 2),
        (0, 2),
        (69, 1),
        (70, 1),
    ];
    for bar in 0..8 {
        let base = bar as f64 * 16.0 * s16;
        let mut pos = 0.0;
        let up = if bar % 4 == 3 { 5 } else { 0 };
        for (n, len) in riff {
            if n > 0 {
                note(base + pos * s16, f64::from(len) * s16 * 0.9, 0, n + up, 100);
            }
            pos += f64::from(len);
        }
        for k in 0..8 {
            let root = if bar % 4 == 3 { 31 } else { 26 };
            note(
                base + k as f64 * 2.0 * s16,
                s16 * 1.8,
                1,
                root + if k % 4 == 3 { 12 } else { 0 },
                110,
            );
        }
        for k in 0..16 {
            let t = base + k as f64 * s16;
            if k == 0 || k == 10 {
                note(t, s16, 9, 36, 120);
            }
            if k == 8 {
                note(t, s16, 9, 38, 120);
            }
            note(t, s16 * 0.5, 9, 42, if k % 2 == 0 { 80 } else { 50 });
        }
    }
    v.push(Ev {
        t: 0.0,
        status: 0xC0,
        d1: 80,
        d2: 0,
    }); // square lead
    v.push(Ev {
        t: 0.0,
        status: 0xC1,
        d1: 38,
        d2: 0,
    }); // synth bass
    v.sort_by(|a, b| a.t.total_cmp(&b.t));
    v
}

/// The song to play: the file next to the settings, or the fallback.
pub fn song(dir: &Path) -> Vec<Ev> {
    std::fs::read(dir.join("easter-egg.mid"))
        .ok()
        .and_then(|b| parse_midi(&b))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(fallback_song)
}

/// Start playing `song` on a loop until the process exits.
pub fn play_forever(song: Vec<Ev>) {
    let _ = std::thread::Builder::new()
        .name("ss-easter".into())
        .spawn(move || {
            #[cfg(windows)]
            {
                if windows_midi::play_forever(&song) {
                    return;
                }
            }
            soft::play_forever(&song);
        });
}

#[cfg(windows)]
mod windows_midi {
    use super::Ev;
    use std::time::{Duration, Instant};

    #[link(name = "winmm")]
    extern "system" {
        fn midiOutOpen(h: *mut isize, id: u32, cb: usize, inst: usize, flags: u32) -> u32;
        fn midiOutShortMsg(h: isize, msg: u32) -> u32;
    }
    const MIDI_MAPPER: u32 = 0xFFFF_FFFF;

    /// Through the Microsoft GS Wavetable Synth. False if it can't open.
    pub fn play_forever(song: &[Ev]) -> bool {
        let mut h: isize = 0;
        // SAFETY: plain winmm calls with a valid out-pointer.
        if unsafe { midiOutOpen(&mut h, MIDI_MAPPER, 0, 0, 0) } != 0 {
            return false;
        }
        let end = song.last().map_or(1.0, |e| e.t) + 0.5;
        loop {
            let start = Instant::now();
            for e in song {
                let at = Duration::from_secs_f64(e.t);
                let now = start.elapsed();
                if at > now {
                    std::thread::sleep(at - now);
                }
                let msg = u32::from(e.status) | (u32::from(e.d1) << 8) | (u32::from(e.d2) << 16);
                // SAFETY: `h` was opened above and is never closed.
                unsafe { midiOutShortMsg(h, msg) };
            }
            let rest = Duration::from_secs_f64(end).saturating_sub(start.elapsed());
            std::thread::sleep(rest);
            // All notes off before going round again.
            for ch in 0..16u32 {
                unsafe { midiOutShortMsg(h, 0xB0 | ch | (123 << 8)) };
            }
        }
    }
}

/// A small software synth: renders the song once, then loops it.
mod soft {
    use super::Ev;

    const RATE: u32 = 32_000;

    /// One sample of an instrument for General MIDI program `prog`.
    fn wave(prog: u8, phase: f32) -> f32 {
        let p = phase.fract();
        match prog {
            80 | 81 | 62..=63 => {
                if p < 0.5 {
                    0.5
                } else {
                    -0.5
                }
            } // square / synth brass
            32..=39 => (p * std::f32::consts::TAU).sin() * 0.9 + if p < 0.5 { 0.2 } else { -0.2 }, // bass
            82..=95 => 1.0 - 2.0 * p, // saw leads, pads
            _ => {
                (p * std::f32::consts::TAU).sin() * 0.8
                    + (p * 2.0 * std::f32::consts::TAU).sin() * 0.2
            }
        }
    }

    fn noise(seed: &mut u32) -> f32 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        (*seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Render to mono samples.
    pub fn render(song: &[Ev]) -> Vec<f32> {
        let len = song.last().map_or(1.0, |e| e.t) + 1.0;
        let n = (len * f64::from(RATE)) as usize;
        let mut out = vec![0f32; n.min(RATE as usize * 600)];
        let mut prog = [0u8; 16];
        // Pair note-ons with their offs.
        let mut open: Vec<(u8, u8, f64, u8)> = Vec::new();
        let mut notes: Vec<(u8, u8, f64, f64, u8)> = Vec::new(); // ch, note, on, off, vel
        for e in song {
            let ch = e.status & 0x0F;
            match e.status & 0xF0 {
                0xC0 => prog[ch as usize] = e.d1,
                0x90 if e.d2 > 0 => open.push((ch, e.d1, e.t, e.d2)),
                0x80 | 0x90 => {
                    if let Some(k) = open.iter().position(|o| o.0 == ch && o.1 == e.d1) {
                        let o = open.remove(k);
                        notes.push((o.0, o.1, o.2, e.t, o.3));
                    }
                }
                _ => {}
            }
        }
        let mut seed = 0x1234_5678u32;
        let rate = f64::from(RATE);
        for (ch, note, on, off, vel) in notes {
            let s0 = (on * rate) as usize;
            let amp = f32::from(vel) / 127.0 * 0.18;
            if ch == 9 {
                // Drums: kick, snare, hats and cymbals.
                let (dur, kind) = match note {
                    35 | 36 => (0.25, 0),
                    37..=40 => (0.18, 1),
                    _ => (0.06, 2),
                };
                let ns = (dur * rate) as usize;
                for k in 0..ns.min(out.len().saturating_sub(s0)) {
                    let t = k as f32 / RATE as f32;
                    let env = (1.0 - k as f32 / ns as f32).powi(2);
                    let v = match kind {
                        0 => {
                            (std::f32::consts::TAU * (50.0 + 120.0 * (-t * 30.0).exp()) * t).sin()
                                * 1.6
                        }
                        1 => {
                            noise(&mut seed) * 0.8 + (std::f32::consts::TAU * 190.0 * t).sin() * 0.4
                        }
                        _ => noise(&mut seed) * 0.35,
                    };
                    out[s0 + k] += v * env * amp;
                }
                continue;
            }
            let f = 440.0 * 2f32.powf((f32::from(note) - 69.0) / 12.0);
            let release = 0.06;
            let ns = (((off - on) + release) * rate) as usize;
            let held = ((off - on) * rate) as usize;
            for k in 0..ns.min(out.len().saturating_sub(s0)) {
                let t = k as f32 / RATE as f32;
                let attack = (k as f32 / (0.005 * RATE as f32)).min(1.0);
                let rel = if k > held {
                    1.0 - (k - held) as f32 / (release as f32 * RATE as f32)
                } else {
                    1.0
                };
                out[s0 + k] += wave(prog[ch as usize], f * t) * amp * attack * rel.max(0.0);
            }
        }
        // Soft clip.
        for s in &mut out {
            *s = s.tanh();
        }
        out
    }

    pub fn play_forever(song: &[Ev]) {
        use std::num::NonZero;
        let samples = render(song);
        let secs = samples.len() as f64 / f64::from(RATE);
        let opened = std::panic::catch_unwind(rodio::DeviceSinkBuilder::open_default_sink);
        let Ok(Ok(mut sink)) = opened else {
            tracing::warn!("easter egg: no audio output");
            return;
        };
        sink.log_on_drop(false);
        let (Some(ch), Some(sr)) = (NonZero::new(1u16), NonZero::new(RATE)) else {
            return;
        };
        // Round and round until the app closes.
        loop {
            let player = rodio::Player::connect_new(sink.mixer());
            player.append(rodio::buffer::SamplesBuffer::new(ch, sr, samples.clone()));
            std::thread::sleep(std::time::Duration::from_secs_f64(secs));
            player.detach();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_small_midi_file() {
        // Format 0, 96 ppq; tempo 120 bpm; note on C4 at 0, off at 96 ticks.
        let mut f = b"MThd\0\0\0\x06\0\0\0\x01\0\x60".to_vec();
        let trk: Vec<u8> = vec![
            0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20, // tempo 500000
            0x00, 0x90, 60, 100, // on
            0x60, 60, 0, // running status: on vel 0 (= off) after 96 ticks
            0x00, 0xFF, 0x2F, 0x00,
        ];
        f.extend(b"MTrk");
        f.extend((trk.len() as u32).to_be_bytes());
        f.extend(trk);
        let ev = parse_midi(&f).unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!((ev[0].status, ev[0].d1, ev[0].d2), (0x90, 60, 100));
        assert!((ev[1].t - 0.5).abs() < 1e-9, "{}", ev[1].t);
        assert!(parse_midi(b"nope").is_none());
    }

    #[test]
    fn the_fallback_renders() {
        let s = fallback_song();
        assert!(s.len() > 100);
        let pcm = soft::render(&s);
        assert!(pcm.iter().any(|v| v.abs() > 0.05));
        assert!(pcm.iter().all(|v| v.abs() <= 1.0));
    }

    /// A real file: `SS_EASTER_MIDI=/path/to.mid`.
    #[test]
    fn reads_a_real_file_when_available() {
        let Ok(p) = std::env::var("SS_EASTER_MIDI") else {
            return;
        };
        let ev = parse_midi(&std::fs::read(p).unwrap()).unwrap();
        let notes = ev
            .iter()
            .filter(|e| e.status & 0xF0 == 0x90 && e.d2 > 0)
            .count();
        println!(
            "{} events, {} notes, {:.1} s",
            ev.len(),
            notes,
            ev.last().unwrap().t
        );
        let pcm = soft::render(&ev);
        assert!(pcm.iter().any(|v| v.abs() > 0.05));
    }
}
