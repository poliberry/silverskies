//! The operating system's accent colour, for theming the app with it:
//! Windows (the DWM accent in the registry), macOS (`AppleAccentColor`),
//! GNOME 47+ (`accent-color`) and KDE (`kdeglobals`). Read on a background
//! thread every minute, so changing it in the system settings follows.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

static ACCENT: OnceLock<Arc<Mutex<Option<[u8; 3]>>>> = OnceLock::new();

/// The current system accent colour (None until read, or if the system
/// doesn't have one). Starts the background reader on first use.
pub fn get() -> Option<[u8; 3]> {
    let cell = ACCENT.get_or_init(|| {
        let cell = Arc::new(Mutex::new(None));
        let c2 = cell.clone();
        let _ = std::thread::Builder::new()
            .name("ss-accent".into())
            .spawn(move || loop {
                let v = read();
                if let Ok(mut g) = c2.lock() {
                    *g = v;
                }
                std::thread::sleep(Duration::from_secs(60));
            });
        cell
    });
    cell.lock().ok().and_then(|g| *g)
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = std::process::Command::new(cmd);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up.
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(windows)]
fn read() -> Option<[u8; 3]> {
    let out = run(
        "reg",
        &[
            "query",
            r"HKCU\Software\Microsoft\Windows\DWM",
            "/v",
            "AccentColor",
        ],
    )?;
    parse_windows(&out)
}

#[cfg(target_os = "macos")]
fn read() -> Option<[u8; 3]> {
    // Absent = the default (blue / multicolour).
    let v = run("defaults", &["read", "-g", "AppleAccentColor"]).unwrap_or_default();
    Some(mac_color(v.trim().parse().unwrap_or(4)))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn read() -> Option<[u8; 3]> {
    if let Some(name) = run(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "accent-color"],
    ) {
        if let Some(c) = gnome_color(&name) {
            return Some(c);
        }
    }
    let home = std::env::var_os("HOME")?;
    let kde = std::path::Path::new(&home).join(".config/kdeglobals");
    parse_kde(&std::fs::read_to_string(kde).ok()?)
}

#[cfg(not(any(windows, unix)))]
fn read() -> Option<[u8; 3]> {
    None
}

/// `AccentColor    REG_DWORD    0xffd77800` (0xAABBGGRR).
#[allow(dead_code)]
pub fn parse_windows(out: &str) -> Option<[u8; 3]> {
    let hex = out.split_whitespace().find_map(|t| t.strip_prefix("0x"))?;
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some([
        (v & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
    ])
}

/// macOS accent numbers to Apple's system colours.
#[allow(dead_code)]
pub fn mac_color(n: i32) -> [u8; 3] {
    match n {
        -1 => [0x8e, 0x8e, 0x93], // graphite
        0 => [0xff, 0x3b, 0x30],  // red
        1 => [0xff, 0x95, 0x00],  // orange
        2 => [0xff, 0xcc, 0x00],  // yellow
        3 => [0x28, 0xcd, 0x41],  // green
        5 => [0xaf, 0x52, 0xde],  // purple
        6 => [0xff, 0x2d, 0x55],  // pink
        _ => [0x00, 0x7a, 0xff],  // blue
    }
}

/// GNOME's named accents (`'blue'`) to their colours.
#[allow(dead_code)]
pub fn gnome_color(s: &str) -> Option<[u8; 3]> {
    Some(match s.trim().trim_matches('\'') {
        "blue" => [0x35, 0x84, 0xe4],
        "teal" => [0x21, 0x90, 0xa4],
        "green" => [0x3a, 0x94, 0x4a],
        "yellow" => [0xc8, 0x88, 0x00],
        "orange" => [0xed, 0x5b, 0x00],
        "red" => [0xe6, 0x2d, 0x42],
        "pink" => [0xd5, 0x61, 0x99],
        "purple" => [0x91, 0x41, 0xac],
        "slate" => [0x6f, 0x83, 0x96],
        _ => return None,
    })
}

/// KDE: `AccentColor=r,g,b` under `[General]`, else the selection colour.
#[allow(dead_code)]
pub fn parse_kde(text: &str) -> Option<[u8; 3]> {
    let rgb = |v: &str| -> Option<[u8; 3]> {
        let p: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (p.len() >= 3).then(|| [p[0], p[1], p[2]])
    };
    let mut section = "";
    let mut selection = None;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            section = l;
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            match (section, k.trim()) {
                ("[General]", "AccentColor") => return rgb(v),
                ("[Colors:Selection]", "BackgroundNormal") => selection = rgb(v),
                _ => {}
            }
        }
    }
    selection
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_system() {
        let reg = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\DWM\r\n    AccentColor    REG_DWORD    0xffd77800\r\n";
        assert_eq!(parse_windows(reg), Some([0x00, 0x78, 0xd7]));
        assert_eq!(mac_color(3), [0x28, 0xcd, 0x41]);
        assert_eq!(gnome_color("'teal'\n"), Some([0x21, 0x90, 0xa4]));
        assert_eq!(gnome_color("'unknown'"), None);
        let kde =
            "[Colors:Selection]\nBackgroundNormal=61,174,233\n[General]\nAccentColor=233,100,50\n";
        assert_eq!(parse_kde(kde), Some([233, 100, 50]));
        assert_eq!(
            parse_kde("[Colors:Selection]\nBackgroundNormal=61,174,233\n"),
            Some([61, 174, 233])
        );
    }
}
