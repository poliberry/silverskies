//! The 3D globe window: `silverskies-globe` (CesiumJS in the system web
//! view), started on demand and fed the place and the radar volume over its
//! stdin. It is a separate program so Pro never depends on a web view.

use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use ss_core::radar3d::{RadarSource, Scene};

/// The helper next to this executable.
pub fn helper_path() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) {
        "silverskies-globe.exe"
    } else {
        "silverskies-globe"
    };
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    // Beside Pro (Windows, macOS bundle, .deb, a build), or where the
    // AppImage keeps it.
    [
        dir.join(name),
        dir.join("../lib/silverskies-pro").join(name),
    ]
    .into_iter()
    .find(|p| p.exists())
}

pub struct Globe {
    child: Child,
    stdin: Option<ChildStdin>,
    /// Lines the globe printed (its page's log), for Pro's log.
    out: Arc<Mutex<Vec<String>>>,
    /// The scene last sent (place id and source), so it's sent once.
    sent: Option<String>,
    /// The window and web view came up.
    pub started: bool,
}

impl Globe {
    pub fn start() -> Result<Globe, String> {
        let path = helper_path().ok_or_else(|| {
            "the 3D globe (silverskies-globe) isn't installed next to Silver Skies Pro".to_string()
        })?;
        let mut cmd = Command::new(path);
        // Inside an AppImage, use the system's libraries (WebKitGTK), not
        // the bundled ones.
        if std::env::var_os("APPIMAGE").is_some() {
            cmd.env_remove("LD_LIBRARY_PATH");
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("starting the 3D globe: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let out = Arc::new(Mutex::new(Vec::new()));
        if let Some(stdout) = child.stdout.take() {
            let out = out.clone();
            std::thread::Builder::new()
                .name("ss-globe-out".into())
                .spawn(move || {
                    for line in std::io::BufReader::new(stdout)
                        .lines()
                        .map_while(Result::ok)
                    {
                        if let Ok(mut o) = out.lock() {
                            o.push(line);
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        Ok(Globe {
            child,
            stdin: Some(stdin),
            out,
            sent: None,
            started: false,
        })
    }

    /// Still open?
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn send(&mut self, v: &serde_json::Value) {
        if let Some(s) = &mut self.stdin {
            let _ = writeln!(s, "{v}");
            let _ = s.flush();
        }
    }

    /// What the globe has printed since last asked.
    pub fn drain(&mut self) -> Vec<String> {
        let lines: Vec<String> = self
            .out
            .lock()
            .map(|mut o| std::mem::take(&mut *o))
            .unwrap_or_default();
        if lines.iter().any(|l| l == "ready") {
            self.started = true;
        }
        lines
    }

    /// Why it may not have started, for the log.
    pub fn failure_hint() -> &'static str {
        if cfg!(target_os = "linux") {
            "the 3D globe couldn't start: it needs WebKitGTK (e.g. sudo apt install libwebkit2gtk-4.1-0, \
             sudo dnf install webkit2gtk4.1, sudo pacman -S webkit2gtk-4.1)"
        } else if cfg!(windows) {
            "the 3D globe couldn't start: it needs the Microsoft Edge WebView2 runtime"
        } else {
            "the 3D globe couldn't start"
        }
    }

    /// Fly to the place and show its radar volume (once per scene).
    pub fn show(&mut self, place_id: &str, name: &str, scene: &Scene) {
        let key = format!("{place_id}:{:?}:{}", scene.source, scene.voxels.len());
        if self.sent.as_deref() == Some(key.as_str()) {
            return;
        }
        let new_place = self
            .sent
            .as_deref()
            .is_none_or(|k| k.split(':').next() != Some(place_id));
        self.sent = Some(key);
        if new_place {
            self.send(&serde_json::json!({
                "type": "scene", "lat": scene.lat, "lon": scene.lon, "name": name,
            }));
            self.send(&serde_json::json!({
                "type": "title", "text": format!("3D Globe · {name} · Silver Skies Pro"),
            }));
        }
        let label = match &scene.source {
            Some(RadarSource::Nexrad { site, .. }) => format!("{site} volume scan"),
            Some(RadarSource::Mosaic { .. }) => "Radar mosaic (estimated heights)".into(),
            None => "No radar".into(),
        };
        let r2 = |v: f32| (v * 100.0).round() / 100.0;
        let v: Vec<f32> = scene
            .voxels
            .iter()
            .flat_map(|x| [r2(x.x), r2(x.y), r2(x.z), x.dbz.round()])
            .collect();
        self.send(&serde_json::json!({
            "type": "radar", "lat": scene.lat, "lon": scene.lon,
            "voxel_km": scene.voxel_km, "label": format!("{name} · {label}"), "v": v,
        }));
    }
}

impl Drop for Globe {
    fn drop(&mut self) {
        // Closing its stdin closes the window.
        self.stdin.take();
    }
}
