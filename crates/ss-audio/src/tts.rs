//! Native text-to-speech wrapper plus a thread-owning handle.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};

use crate::AudioError;

fn tts_err(e: impl std::fmt::Display) -> AudioError {
    AudioError::Tts(e.to_string())
}

/// Map a user multiplier (0.5..=2.0, 1.0 = normal) onto a backend range.
///
/// Piecewise linear so that 1.0 always lands exactly on `normal` even when
/// the backend range is asymmetric. Monotonic in `mult`.
pub fn map_rate(mult: f32, min: f32, normal: f32, max: f32) -> f32 {
    let m = if mult.is_finite() {
        mult.clamp(0.5, 2.0)
    } else {
        1.0
    };
    if m <= 1.0 {
        min + (normal - min) * (m - 0.5) / 0.5
    } else {
        normal + (max - normal) * (m - 1.0)
    }
}

/// Text-to-speech: the platform engine (WinRT/SAPI, AVFoundation) or, on
/// Linux, the `espeak-ng` / `espeak` command, falling back to
/// speech-dispatcher. May be `!Send` depending on the
/// backend; use [`TtsHandle`] from multi-threaded code.
pub struct Tts {
    backend: Backend,
}

enum Backend {
    Native(tts::Tts),
    Command(CommandTts),
}

impl Tts {
    pub fn new() -> Result<Tts, AudioError> {
        if !cfg!(target_os = "linux") {
            let inner = tts::Tts::default().map_err(tts_err)?;
            return Ok(Tts {
                backend: Backend::Native(inner),
            });
        }
        // Linux: speech-dispatcher is often missing, not running or without
        // an output module, and then fails silently. The espeak-ng command
        // just works when installed, so it comes first (`SS_TTS=speechd`
        // prefers speech-dispatcher).
        let prefer_speechd = std::env::var("SS_TTS").is_ok_and(|v| v == "speechd");
        if !prefer_speechd {
            if let Some(c) = CommandTts::find() {
                return Ok(Tts {
                    backend: Backend::Command(c),
                });
            }
        }
        match tts::Tts::default() {
            Ok(t) if t.voices().is_ok_and(|v| !v.is_empty()) => {
                return Ok(Tts {
                    backend: Backend::Native(t),
                })
            }
            Ok(_) => tracing::warn!("speech-dispatcher has no voices"),
            Err(e) => tracing::warn!("speech-dispatcher unavailable: {e}"),
        }
        CommandTts::find()
            .map(|c| Tts {
                backend: Backend::Command(c),
            })
            .ok_or_else(|| AudioError::Tts("no speech engine found: install espeak-ng".into()))
    }

    /// Which engine is speaking, for the log and settings.
    pub fn engine(&self) -> &'static str {
        match &self.backend {
            Backend::Native(_) if cfg!(target_os = "linux") => "speech-dispatcher",
            Backend::Native(_) => "system voice",
            Backend::Command(c) => c.program,
        }
    }

    /// Speak `text`.
    pub fn speak(&mut self, text: &str, interrupt: bool) -> Result<(), AudioError> {
        match &mut self.backend {
            Backend::Native(t) => t.speak(text, interrupt).map(|_| ()).map_err(tts_err),
            Backend::Command(c) => {
                c.speak(text, interrupt);
                Ok(())
            }
        }
    }

    pub fn stop(&mut self) -> Result<(), AudioError> {
        match &mut self.backend {
            Backend::Native(t) => t.stop().map(|_| ()).map_err(tts_err),
            Backend::Command(c) => {
                c.stop();
                Ok(())
            }
        }
    }

    pub fn is_speaking(&mut self) -> bool {
        match &mut self.backend {
            Backend::Native(t) => t.is_speaking().unwrap_or(false),
            Backend::Command(c) => c.pump(),
        }
    }

    /// `mult` in 0.5..=2.0, 1.0 = backend normal rate.
    pub fn set_rate(&mut self, mult: f32) -> Result<(), AudioError> {
        match &mut self.backend {
            Backend::Native(t) => {
                if !t.supported_features().rate {
                    return Ok(());
                }
                let r = map_rate(mult, t.min_rate(), t.normal_rate(), t.max_rate());
                t.set_rate(r).map(|_| ()).map_err(tts_err)
            }
            Backend::Command(c) => {
                c.rate = map_rate(mult, 90.0, 170.0, 340.0);
                Ok(())
            }
        }
    }

    /// `mult` in 0.5..=2.0, 1.0 = backend normal pitch. No-op if unsupported.
    pub fn set_pitch(&mut self, mult: f32) -> Result<(), AudioError> {
        match &mut self.backend {
            Backend::Native(t) => {
                if !t.supported_features().pitch {
                    return Ok(());
                }
                let p = map_rate(mult, t.min_pitch(), t.normal_pitch(), t.max_pitch());
                t.set_pitch(p).map(|_| ()).map_err(tts_err)
            }
            Backend::Command(c) => {
                c.pitch = map_rate(mult, 20.0, 50.0, 90.0);
                Ok(())
            }
        }
    }

    pub fn voices(&self) -> Vec<String> {
        match &self.backend {
            Backend::Native(t) => t
                .voices()
                .map(|v| v.iter().map(|v| v.name()).collect())
                .unwrap_or_default(),
            Backend::Command(c) => c.voices(),
        }
    }

    pub fn set_voice_by_name(&mut self, name: &str) -> Result<(), AudioError> {
        match &mut self.backend {
            Backend::Native(t) => {
                let voices = t.voices().map_err(tts_err)?;
                let voice = voices
                    .iter()
                    .find(|v| v.name() == name)
                    .or_else(|| {
                        let lower = name.to_lowercase();
                        voices
                            .iter()
                            .find(|v| v.name().to_lowercase().contains(&lower))
                    })
                    .ok_or_else(|| AudioError::Tts(format!("voice not found: {name}")))?;
                t.set_voice(voice).map_err(tts_err)
            }
            Backend::Command(c) => {
                c.voice = Some(name.to_string());
                Ok(())
            }
        }
    }
}

/// Speech through the `espeak-ng` (or `espeak`) command: one process per
/// utterance, queued.
struct CommandTts {
    program: &'static str,
    rate: f32,
    pitch: f32,
    voice: Option<String>,
    queue: std::collections::VecDeque<String>,
    child: Option<std::process::Child>,
}

impl CommandTts {
    fn find() -> Option<CommandTts> {
        ["espeak-ng", "espeak"].into_iter().find_map(|program| {
            let ok = std::process::Command::new(program)
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            ok.then(|| CommandTts {
                program,
                rate: 170.0,
                pitch: 50.0,
                voice: None,
                queue: Default::default(),
                child: None,
            })
        })
    }

    fn speak(&mut self, text: &str, interrupt: bool) {
        if interrupt {
            self.stop();
        }
        self.queue.push_back(text.to_string());
        self.pump();
    }

    fn stop(&mut self) {
        self.queue.clear();
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Start the next utterance when the last has finished. True while
    /// anything is playing or queued.
    fn pump(&mut self) -> bool {
        if let Some(c) = &mut self.child {
            match c.try_wait() {
                Ok(None) => return true,
                _ => self.child = None,
            }
        }
        if let Some(text) = self.queue.pop_front() {
            let mut cmd = std::process::Command::new(self.program);
            cmd.arg("-s")
                .arg(format!("{}", self.rate.round()))
                .arg("-p")
                .arg(format!("{}", self.pitch.round()));
            if let Some(v) = &self.voice {
                cmd.arg("-v").arg(v);
            }
            // `--` so text starting with '-' isn't read as an option.
            cmd.arg("--")
                .arg(text)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            match cmd.spawn() {
                Ok(c) => self.child = Some(c),
                Err(e) => tracing::warn!("{}: {e}", self.program),
            }
        }
        self.child.is_some() || !self.queue.is_empty()
    }

    /// Voice names from `--voices` (the fourth column).
    fn voices(&self) -> Vec<String> {
        std::process::Command::new(self.program)
            .arg("--voices")
            .output()
            .map(|o| parse_espeak_voices(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    }
}

impl Drop for CommandTts {
    fn drop(&mut self) {
        self.stop();
    }
}

fn parse_espeak_voices(out: &str) -> Vec<String> {
    out.lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().nth(3).map(str::to_string))
        .collect()
}

/// Commands accepted by the TTS thread.
#[derive(Debug, Clone)]
pub enum TtsCmd {
    Speak { text: String, interrupt: bool },
    Stop,
    SetRate(f32),
    SetPitch(f32),
    SetVoice(String),
    Shutdown,
}

/// Cheap, thread-safe front for a TTS engine living on its own thread.
pub struct TtsHandle {
    tx: Sender<TtsCmd>,
    busy: Arc<AtomicBool>,
    voices: Arc<parking_lot::Mutex<Vec<String>>>,
    engine: &'static str,
    thread: Option<JoinHandle<()>>,
}

impl TtsHandle {
    /// Spawn the TTS thread. Fails if the backend cannot be initialised.
    pub fn spawn() -> Result<TtsHandle, AudioError> {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (init_tx, init_rx) =
            crossbeam_channel::bounded::<Result<(&'static str, Vec<String>), AudioError>>(1);
        let busy = Arc::new(AtomicBool::new(false));
        let voices = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let busy_t = busy.clone();
        let thread = std::thread::Builder::new()
            .name("ss-tts".into())
            .spawn(move || match Tts::new() {
                Ok(tts) => {
                    tracing::info!("speech engine: {}", tts.engine());
                    let _ = init_tx.send(Ok((tts.engine(), tts.voices())));
                    run_loop(tts, rx, busy_t);
                }
                Err(e) => {
                    let _ = init_tx.send(Err(e));
                }
            })
            .map_err(tts_err)?;
        let engine = match init_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok((e, v))) => {
                *voices.lock() = v;
                e
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(AudioError::Timeout),
        };
        Ok(TtsHandle {
            tx,
            busy,
            voices,
            engine,
            thread: Some(thread),
        })
    }

    pub fn speak_queue(&self, text: &str) {
        self.send(TtsCmd::Speak {
            text: text.to_string(),
            interrupt: false,
        });
    }

    pub fn speak_now(&self, text: &str) {
        self.send(TtsCmd::Speak {
            text: text.to_string(),
            interrupt: true,
        });
    }

    pub fn stop(&self) {
        self.send(TtsCmd::Stop);
    }

    pub fn set_rate(&self, mult: f32) {
        self.send(TtsCmd::SetRate(mult));
    }

    pub fn set_pitch(&self, mult: f32) {
        self.send(TtsCmd::SetPitch(mult));
    }

    pub fn set_voice(&self, name: &str) {
        self.send(TtsCmd::SetVoice(name.to_string()));
    }

    /// The engine in use (`espeak-ng`, `speech-dispatcher`, …).
    pub fn engine(&self) -> &'static str {
        self.engine
    }

    /// Voice names captured at startup.
    pub fn voices(&self) -> Vec<String> {
        self.voices.lock().clone()
    }

    /// True while speech is queued or playing.
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    fn send(&self, cmd: TtsCmd) {
        let _ = self.tx.send(cmd);
    }
}

impl Drop for TtsHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(TtsCmd::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// After submitting speech, report busy for at least this long even if the
/// backend has not started playing yet (WinRT lags behind `speak`).
const BUSY_GRACE: Duration = Duration::from_millis(600);

fn run_loop(mut tts: Tts, rx: Receiver<TtsCmd>, busy: Arc<AtomicBool>) {
    let mut grace_until = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(TtsCmd::Speak { text, interrupt }) => match tts.speak(&text, interrupt) {
                Ok(_) => grace_until = Instant::now() + BUSY_GRACE,
                Err(e) => tracing::warn!("tts speak failed: {e}"),
            },
            Ok(TtsCmd::Stop) => {
                if let Err(e) = tts.stop() {
                    tracing::warn!("tts stop failed: {e}");
                }
                grace_until = Instant::now();
            }
            Ok(TtsCmd::SetRate(r)) => {
                if let Err(e) = tts.set_rate(r) {
                    tracing::warn!("tts set_rate failed: {e}");
                }
            }
            Ok(TtsCmd::SetPitch(p)) => {
                if let Err(e) = tts.set_pitch(p) {
                    tracing::warn!("tts set_pitch failed: {e}");
                }
            }
            Ok(TtsCmd::SetVoice(v)) => {
                if let Err(e) = tts.set_voice_by_name(&v) {
                    tracing::warn!("tts set_voice failed: {e}");
                }
            }
            Ok(TtsCmd::Shutdown) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                let _ = tts.stop();
                busy.store(false, Ordering::Relaxed);
                return;
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        busy.store(
            Instant::now() < grace_until || tts.is_speaking(),
            Ordering::Relaxed,
        );
    }
}

/// Collapse NWS text artifacts: `*` bullets, `...` pauses, newlines, repeated spaces.
fn clean(s: &str) -> String {
    let s = s.replace("...", ". ").replace('*', " ");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strip trailing sentence punctuation/space so parts can be joined with ". ".
fn trim_end_punct(s: &str) -> &str {
    s.trim_end_matches(|c: char| c == '.' || c == '!' || c == '?' || c == ',' || c.is_whitespace())
}

/// Truncate at the last sentence boundary within `max_chars`; fall back to a
/// word boundary (with a trailing period).
fn truncate_sentences(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let cut_byte = s
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let head = &s[..cut_byte];
    let next_is_space = s[cut_byte..].chars().next().is_none_or(char::is_whitespace);
    let mut end = None;
    for (i, c) in head.char_indices() {
        if matches!(c, '.' | '!' | '?') {
            let after = &head[i + c.len_utf8()..];
            let at_boundary = match after.chars().next() {
                Some(n) => n.is_whitespace(),
                None => next_is_space,
            };
            if at_boundary {
                end = Some(i + c.len_utf8());
            }
        }
    }
    if let Some(e) = end {
        return head[..e].to_string();
    }
    let word_cut = head.rfind(' ').map(|i| &head[..i]).unwrap_or(head);
    format!("{}.", trim_end_punct(word_cut))
}

/// Build the spoken text for an alert.
pub fn alert_announcement(
    display_event: &str,
    headline: Option<&str>,
    area: Option<&str>,
    description: Option<&str>,
    max_chars: usize,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    for p in [Some(display_event), headline, area].into_iter().flatten() {
        let c = clean(p);
        let t = trim_end_punct(&c);
        if !t.is_empty() {
            parts.push(format!("{t}."));
        }
    }
    if let Some(d) = description {
        let c = clean(d);
        // "...." artifacts can leave ". ." sequences behind.
        let c = c.replace(". .", ".");
        if !c.is_empty() {
            let t = truncate_sentences(&c, max_chars);
            if !t.is_empty() {
                parts.push(t);
            }
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_espeak_voices() {
        let out = "Pty Language       Age/Gender VoiceName          File          Other Languages\n 5  af              --/M      Afrikaans          gmw/af\n 2  en-us           --/M      English_(America)  gmw/en-US\n";
        assert_eq!(
            parse_espeak_voices(out),
            vec!["Afrikaans".to_string(), "English_(America)".to_string()]
        );
    }

    #[test]
    fn rate_mapping() {
        assert_eq!(map_rate(1.0, 0.5, 1.0, 6.0), 1.0);
        assert_eq!(map_rate(0.5, 0.5, 1.0, 6.0), 0.5);
        assert_eq!(map_rate(2.0, 0.5, 1.0, 6.0), 6.0);
        assert_eq!(map_rate(9.0, 0.5, 1.0, 6.0), 6.0);
        assert_eq!(map_rate(f32::NAN, 0.5, 1.0, 6.0), 1.0);
        let mut prev = f32::MIN;
        for i in 0..=30 {
            let r = map_rate(0.5 + i as f32 * 0.05, 0.5, 1.0, 6.0);
            assert!(r >= prev);
            prev = r;
        }
    }

    #[test]
    fn announcement_basic() {
        let s = alert_announcement(
            "Tornado Warning",
            Some("Tornado Warning issued for Foo County"),
            Some("Foo; Bar"),
            Some("* WHAT...Tornado.\n\n* WHERE...Foo   County."),
            500,
        );
        assert_eq!(
            s,
            "Tornado Warning. Tornado Warning issued for Foo County. Foo; Bar. WHAT. Tornado. WHERE. Foo County."
        );
    }

    #[test]
    fn announcement_none_fields() {
        assert_eq!(
            alert_announcement("Flood Watch", None, None, None, 100),
            "Flood Watch."
        );
        assert_eq!(
            alert_announcement("Flood Watch", None, Some("Bar"), None, 100),
            "Flood Watch. Bar."
        );
    }

    #[test]
    fn announcement_truncates_at_sentence() {
        let d = "First sentence here. Second sentence is longer and runs on. Third.";
        let s = alert_announcement("Ev", None, None, Some(d), 40);
        assert_eq!(s, "Ev. First sentence here.");
        let s = alert_announcement("Ev", None, None, Some(d), 200);
        assert_eq!(s, format!("Ev. {d}"));
    }

    #[test]
    fn announcement_truncates_at_word_without_sentence() {
        let s = alert_announcement("Ev", None, None, Some("one two three four five"), 12);
        assert_eq!(s, "Ev. one two.");
    }

    #[test]
    fn truncation_is_char_safe() {
        let s = truncate_sentences("ééééé ééééé ééééé", 7);
        assert_eq!(s, "ééééé.");
    }
}
