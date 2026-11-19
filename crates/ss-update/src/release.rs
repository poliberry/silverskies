//! The latest GitHub release: parsing, version comparison and picking the
//! download that fits this app, OS and CPU.

use serde_json::Value;

/// Where releases are published.
pub const REPO: &str = "poliberry/silverskies";

/// GitHub's "latest release" endpoint (skips drafts and pre-releases).
/// `SS_UPDATE_URL` replaces it (testing against a local mock only).
pub fn latest_url(repo: &str) -> String {
    std::env::var("SS_UPDATE_URL")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| format!("https://api.github.com/repos/{repo}/releases/latest"))
}

/// Which app is updating itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    Simple,
    Pro,
}

impl App {
    /// Prefix of the app's release files (`SilverSkiesPro-2.0.2.2-…`).
    pub fn file_prefix(self) -> &'static str {
        match self {
            App::Simple => "SilverSkies",
            App::Pro => "SilverSkiesPro",
        }
    }

    /// The Linux package / binary name (`silverskies-pro_2.0.2-2_amd64.deb`).
    pub fn bin(self) -> &'static str {
        match self {
            App::Simple => "silverskies",
            App::Pro => "silverskies-pro",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            App::Simple => "Silver Skies",
            App::Pro => "Silver Skies Pro",
        }
    }
}

/// One downloadable file of a release.
#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// SHA-256 as lowercase hex, when GitHub reports it.
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// `v2.0.2.2`
    pub tag: String,
    /// `[2, 0, 2, 2]`: version then build.
    pub version: Vec<u64>,
    pub name: String,
    /// Release notes (Markdown).
    pub notes: String,
    /// The release page.
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// `2.0.2 (build 2)`, from the tag.
    pub fn label(&self) -> String {
        match self.version.split_last() {
            Some((build, ver)) if ver.len() >= 3 => format!(
                "{} (build {build})",
                ver.iter().map(u64::to_string).collect::<Vec<_>>().join(".")
            ),
            _ => self.tag.trim_start_matches('v').to_string(),
        }
    }
}

/// `v2.0.2.2` → `[2, 0, 2, 2]`; `None` for anything that isn't dotted numbers.
pub fn parse_tag(tag: &str) -> Option<Vec<u64>> {
    let t = tag.trim().trim_start_matches(['v', 'V']);
    if t.is_empty() {
        return None;
    }
    t.split('.').map(|p| p.parse().ok()).collect()
}

/// The release from GitHub's JSON.
pub fn parse_release(json: &str) -> Result<Release, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("bad release JSON: {e}"))?;
    let s = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let tag = s("tag_name");
    let version = parse_tag(&tag).ok_or_else(|| format!("unrecognised release tag {tag:?}"))?;
    let assets = v
        .get("assets")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|a| {
            Some(Asset {
                name: a.get("name")?.as_str()?.to_string(),
                url: a.get("browser_download_url")?.as_str()?.to_string(),
                size: a.get("size").and_then(Value::as_u64).unwrap_or(0),
                sha256: a
                    .get("digest")
                    .and_then(Value::as_str)
                    .and_then(|d| d.strip_prefix("sha256:"))
                    .map(str::to_ascii_lowercase),
            })
        })
        .collect();
    Ok(Release {
        tag,
        version,
        name: s("name"),
        notes: s("body"),
        page: s("html_url"),
        assets,
    })
}

/// Whether `release` is newer than the running app. A release build
/// compares version and build; a local (development) build only offers a
/// newer *version*, since its build number means nothing.
pub fn is_newer(release: &[u64], version: &str, build: Option<&str>) -> bool {
    let Some(mut cur) = parse_tag(version) else {
        return false;
    };
    match build.map(str::trim).and_then(|b| b.parse::<u64>().ok()) {
        Some(b) => {
            cur.push(b);
            release > cur.as_slice()
        }
        None => {
            let n = cur.len().min(release.len());
            release[..n] > cur[..n]
        }
    }
}

/// How this copy of the app was installed (decides how to update it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// Windows: the Inno Setup installer.
    WindowsInstaller,
    /// macOS: a `.app` bundle at this path, replaced from the `.zip`.
    MacBundle(std::path::PathBuf),
    /// Linux: an AppImage at this path, replaced in place.
    AppImage(std::path::PathBuf),
    /// Linux, installed from the `.deb` (needs the system installer).
    Deb,
    /// Can't update itself (e.g. run from a build directory): offer the page.
    Manual,
}

/// The CPU the app was built for, as release files name it.
pub fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    }
}

/// The file to download for `install`.
pub fn pick_asset<'a>(release: &'a Release, app: App, install: &Install) -> Option<&'a Asset> {
    let prefix = format!("{}-", app.file_prefix());
    let find = |pred: &dyn Fn(&str) -> bool| release.assets.iter().find(|a| pred(&a.name));
    match install {
        Install::WindowsInstaller => {
            let setup = format!("{}-Setup-", app.file_prefix());
            find(&|n| n.starts_with(&setup) && n.ends_with(".exe"))
        }
        Install::MacBundle(_) => {
            let tail = format!("-macos-{}.zip", arch());
            find(&|n| n.starts_with(&prefix) && !n.contains("-Setup-") && n.ends_with(&tail))
        }
        Install::AppImage(_) => {
            let tail = format!("-{}.AppImage", arch());
            find(&|n| n.starts_with(&prefix) && n.ends_with(&tail))
        }
        Install::Deb => {
            let start = format!("{}_", app.bin());
            find(&|n| n.starts_with(&start) && n.ends_with("_amd64.deb"))
        }
        Install::Manual => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the real v2.0.2.2 release.
    const JSON: &str = r#"{"tag_name":"v2.0.2.2","name":"Silver Skies & Silver Skies Pro 2.0.2 (build 2)",
      "html_url":"https://github.com/poliberry/silverskies/releases/tag/v2.0.2.2","body":"notes",
      "assets":[
       {"name":"SilverSkies-2.0.2.2-macos-arm64.zip","size":8084956,"digest":"sha256:F1FB","browser_download_url":"https://g/SilverSkies-2.0.2.2-macos-arm64.zip"},
       {"name":"SilverSkies-2.0.2.2-macos-x86_64.zip","size":1,"browser_download_url":"https://g/SilverSkies-2.0.2.2-macos-x86_64.zip"},
       {"name":"SilverSkies-2.0.2.2-x86_64.AppImage","size":1,"browser_download_url":"https://g/a"},
       {"name":"SilverSkies-Setup-2.0.2.2.exe","size":1,"browser_download_url":"https://g/s"},
       {"name":"SilverSkiesPro-2.0.2.2-macos-arm64.zip","size":1,"browser_download_url":"https://g/p"},
       {"name":"SilverSkiesPro-2.0.2.2-x86_64.AppImage","size":1,"browser_download_url":"https://g/pa"},
       {"name":"SilverSkiesPro-Setup-2.0.2.2.exe","size":1,"browser_download_url":"https://g/ps"},
       {"name":"silverskies-pro_2.0.2-2_amd64.deb","size":1,"browser_download_url":"https://g/pd"},
       {"name":"silverskies_2.0.2-2_amd64.deb","size":1,"browser_download_url":"https://g/d"}]}"#;

    #[test]
    fn parses_the_release() {
        let r = parse_release(JSON).unwrap();
        assert_eq!(r.tag, "v2.0.2.2");
        assert_eq!(r.version, vec![2, 0, 2, 2]);
        assert_eq!(r.label(), "2.0.2 (build 2)");
        assert_eq!(r.assets.len(), 9);
        assert_eq!(r.assets[0].sha256.as_deref(), Some("f1fb"));
        assert_eq!(r.assets[1].sha256, None);
    }

    #[test]
    fn compares_versions_and_builds() {
        assert!(is_newer(&[2, 0, 2, 2], "2.0.2", Some("1")));
        assert!(!is_newer(&[2, 0, 2, 2], "2.0.2", Some("2")));
        assert!(!is_newer(&[2, 0, 2, 2], "2.0.2", Some("3")));
        assert!(is_newer(&[2, 0, 3, 1], "2.0.2", Some("9")));
        assert!(is_newer(&[2, 1, 0, 1], "2.0.2", Some("9")));
        assert!(!is_newer(&[1, 9, 9, 9], "2.0.2", Some("1")));
        // Development builds: only a newer version counts.
        assert!(!is_newer(&[2, 0, 2, 2], "2.0.2", None));
        assert!(is_newer(&[2, 0, 3, 1], "2.0.2", None));
    }

    #[test]
    fn rejects_odd_tags() {
        assert_eq!(parse_tag("v2.0.2.1"), Some(vec![2, 0, 2, 1]));
        assert_eq!(parse_tag("nightly"), None);
        assert_eq!(parse_tag("v"), None);
        assert!(parse_release(r#"{"tag_name":"latest"}"#).is_err());
    }

    #[test]
    fn picks_the_right_file() {
        let r = parse_release(JSON).unwrap();
        let name = |app, i: &Install| pick_asset(&r, app, i).map(|a| a.name.clone());
        assert_eq!(
            name(App::Simple, &Install::WindowsInstaller).as_deref(),
            Some("SilverSkies-Setup-2.0.2.2.exe")
        );
        assert_eq!(
            name(App::Pro, &Install::WindowsInstaller).as_deref(),
            Some("SilverSkiesPro-Setup-2.0.2.2.exe")
        );
        // "SilverSkies-" must not match the Pro files and vice versa.
        assert_eq!(
            name(App::Simple, &Install::AppImage("/a".into())).as_deref(),
            Some("SilverSkies-2.0.2.2-x86_64.AppImage")
        );
        assert_eq!(
            name(App::Pro, &Install::Deb).as_deref(),
            Some("silverskies-pro_2.0.2-2_amd64.deb")
        );
        assert_eq!(
            name(App::Simple, &Install::Deb).as_deref(),
            Some("silverskies_2.0.2-2_amd64.deb")
        );
        let mac = name(App::Simple, &Install::MacBundle("/A.app".into())).unwrap();
        assert!(mac.ends_with(&format!("-macos-{}.zip", arch())));
        assert_eq!(name(App::Pro, &Install::Manual), None);
    }
}
