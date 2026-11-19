//! Detecting how the running copy was installed, downloading a release file
//! (checked against GitHub's SHA-256), and putting it in place.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures::StreamExt;
use sha2::{Digest, Sha256};

use crate::release::{App, Asset, Install};

/// What happened after installing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The new version is starting; quit this one now.
    Restarting,
    /// Handed to the system (e.g. the `.deb` opened in the software
    /// installer); the message says what to do next.
    HandedOff(String),
}

/// How the running copy was installed.
pub fn detect(app: App) -> Install {
    let Ok(exe) = std::env::current_exe() else {
        return Install::Manual;
    };
    detect_from(
        app,
        &exe,
        std::env::var_os("APPIMAGE").map(PathBuf::from),
        cfg!(windows),
        cfg!(target_os = "macos"),
    )
}

/// `detect` with its inputs explicit (for tests).
pub fn detect_from(
    _app: App,
    exe: &Path,
    appimage: Option<PathBuf>,
    windows: bool,
    macos: bool,
) -> Install {
    if windows {
        // The Inno Setup installer leaves its uninstaller beside the exe;
        // a portable (zip) copy has none and is updated by hand.
        let installed = exe
            .parent()
            .is_some_and(|d| d.join("unins000.exe").exists());
        return if installed {
            Install::WindowsInstaller
        } else {
            Install::Manual
        };
    }
    if macos {
        // …/Name.app/Contents/MacOS/<exe>
        let bundle = exe.ancestors().nth(3).map(Path::to_path_buf);
        return match bundle {
            Some(b) if b.extension().is_some_and(|e| e == "app") => Install::MacBundle(b),
            _ => Install::Manual,
        };
    }
    if let Some(a) = appimage.filter(|p| p.is_file()) {
        return Install::AppImage(a);
    }
    if exe.starts_with("/usr/") {
        return Install::Deb;
    }
    Install::Manual
}

/// A client for GitHub's API and downloads.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!("silverskies-updater/{}", ss_core::version::VERSION))
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Download `asset` into `dir`, counting bytes into `progress`, and check
/// its SHA-256 when GitHub published one. Returns the file's path.
pub async fn download(
    client: &reqwest::Client,
    asset: &Asset,
    dir: &Path,
    progress: Arc<AtomicU64>,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let path = dir.join(&asset.name);
    let resp = client
        .get(&asset.url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("download failed: {e}"))?;
    let mut file =
        std::fs::File::create(&path).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut stream = resp.bytes_stream();
    progress.store(0, Ordering::Relaxed);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download interrupted: {e}"))?;
        hash.update(&chunk);
        file.write_all(&chunk)
            .map_err(|e| format!("can't write {}: {e}", path.display()))?;
        progress.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if let Some(want) = &asset.sha256 {
        let got = hex(&hash.finalize());
        if &got != want {
            let _ = std::fs::remove_file(&path);
            return Err(format!(
                "the download of {} is corrupt (checksum mismatch); nothing was changed",
                asset.name
            ));
        }
    }
    Ok(path)
}

/// Put the downloaded `file` in place for `install`.
pub fn apply(install: &Install, file: &Path) -> Result<Outcome, String> {
    match install {
        Install::WindowsInstaller => {
            // Silent install over the existing copy; the installer closes
            // this app if it is still running and starts the new one.
            Command::new(file)
                .args([
                    "/SILENT",
                    "/SUPPRESSMSGBOXES",
                    "/NORESTART",
                    "/CLOSEAPPLICATIONS",
                ])
                .spawn()
                .map_err(|e| format!("couldn't start the installer: {e}"))?;
            Ok(Outcome::Restarting)
        }
        Install::MacBundle(bundle) => {
            replace_bundle(bundle, file)?;
            Command::new("open")
                .arg("-n")
                .arg(bundle)
                .spawn()
                .map_err(|e| format!("updated, but couldn't restart: {e}"))?;
            Ok(Outcome::Restarting)
        }
        Install::AppImage(path) => {
            replace_file(path, file)?;
            Command::new(path)
                .spawn()
                .map_err(|e| format!("updated, but couldn't restart: {e}"))?;
            Ok(Outcome::Restarting)
        }
        Install::Deb => {
            open(file)?;
            Ok(Outcome::HandedOff(format!(
                "The update package ({}) was opened in your software installer. \
                 Install it, then restart the app.",
                file.file_name().unwrap_or_default().to_string_lossy()
            )))
        }
        Install::Manual => Err("this copy can't update itself".into()),
    }
}

/// Open a file or URL with the system's default handler.
pub fn open(target: impl AsRef<std::ffi::OsStr>) -> Result<(), String> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    cmd.arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("couldn't open it: {e}"))
}

/// Replace a file atomically: write beside it, make it executable, rename
/// over it (a running AppImage keeps working until it exits).
pub fn replace_file(target: &Path, new: &Path) -> Result<(), String> {
    let staged = target.with_extension("update");
    std::fs::copy(new, &staged).map_err(|e| format!("can't write {}: {e}", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    std::fs::rename(&staged, target).map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        format!("can't replace {}: {e}", target.display())
    })
}

/// Swap a macOS `.app` bundle for the one inside `zip`. The new bundle is
/// unpacked next to the old one (same volume, so the swap is two renames)
/// and the old one is kept until the new one is in place.
fn replace_bundle(bundle: &Path, zip: &Path) -> Result<(), String> {
    let parent = bundle.parent().ok_or("no folder around the app")?;
    let work = parent.join(format!(".silverskies-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)
        .map_err(|e| format!("can't write to {}: {e}", parent.display()))?;
    let status = Command::new("ditto")
        .args(["-x", "-k"])
        .arg(zip)
        .arg(&work)
        .status()
        .map_err(|e| format!("couldn't unpack the update: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&work);
        return Err("couldn't unpack the update".into());
    }
    let new_app = std::fs::read_dir(&work)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or("the update has no app in it")?;
    let old = bundle.with_extension("app-old");
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(bundle, &old).map_err(|e| format!("can't move the old app: {e}"))?;
    if let Err(e) = std::fs::rename(&new_app, bundle) {
        let _ = std::fs::rename(&old, bundle);
        let _ = std::fs::remove_dir_all(&work);
        return Err(format!("can't put the new app in place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    let _ = std::fs::remove_dir_all(&work);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_install_kinds() {
        let tmp = std::env::temp_dir().join(format!("ss-update-detect-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        // Windows: installer copy vs portable.
        let exe = tmp.join("silverskies.exe");
        assert_eq!(
            detect_from(App::Simple, &exe, None, true, false),
            Install::Manual
        );
        std::fs::write(tmp.join("unins000.exe"), b"").unwrap();
        assert_eq!(
            detect_from(App::Simple, &exe, None, true, false),
            Install::WindowsInstaller
        );
        // macOS bundle.
        let mac = Path::new("/Applications/Silver Skies.app/Contents/MacOS/silverskies");
        assert_eq!(
            detect_from(App::Simple, mac, None, false, true),
            Install::MacBundle("/Applications/Silver Skies.app".into())
        );
        assert_eq!(
            detect_from(
                App::Simple,
                Path::new("/tmp/x/silverskies"),
                None,
                false,
                true
            ),
            Install::Manual
        );
        // Linux: AppImage, .deb, or a build directory.
        let img = tmp.join("SilverSkies.AppImage");
        std::fs::write(&img, b"").unwrap();
        assert_eq!(
            detect_from(
                App::Simple,
                Path::new("/tmp/.mount/usr/bin/x"),
                Some(img.clone()),
                false,
                false
            ),
            Install::AppImage(img)
        );
        assert_eq!(
            detect_from(
                App::Pro,
                Path::new("/usr/bin/silverskies-pro"),
                None,
                false,
                false
            ),
            Install::Deb
        );
        assert_eq!(
            detect_from(
                App::Pro,
                Path::new("/home/me/target/release/silverskies-pro"),
                None,
                false,
                false
            ),
            Install::Manual
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Serve `body` once on a local port; returns the URL.
    fn serve_once(body: &'static [u8]) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::Read;
            if let Some(Ok(mut s)) = listener.incoming().next() {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(body);
            }
        });
        format!("http://127.0.0.1:{port}/f")
    }

    #[test]
    fn downloads_and_verifies_checksums() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let dir = std::env::temp_dir().join(format!("ss-update-dl-{}", std::process::id()));
        let good = hex(&Sha256::digest(b"hello"));
        let asset = |url: String, sha: Option<String>| Asset {
            name: "f.bin".into(),
            url,
            size: 5,
            sha256: sha,
        };
        let progress = Arc::new(AtomicU64::new(0));
        let ok = rt
            .block_on(download(
                &client(),
                &asset(serve_once(b"hello"), Some(good)),
                &dir,
                progress.clone(),
            ))
            .unwrap();
        assert_eq!(std::fs::read(&ok).unwrap(), b"hello");
        assert_eq!(progress.load(Ordering::Relaxed), 5);
        let bad = rt.block_on(download(
            &client(),
            &asset(serve_once(b"hellO"), Some(hex(&Sha256::digest(b"hello")))),
            &dir,
            progress,
        ));
        assert!(bad.unwrap_err().contains("checksum"));
        assert!(!dir.join("f.bin").exists(), "a corrupt download is deleted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn replaces_a_file_in_place() {
        let tmp = std::env::temp_dir().join(format!("ss-update-replace-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let target = tmp.join("App.AppImage");
        let new = tmp.join("new.AppImage");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        replace_file(&target, &new).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "executable");
        assert!(!target.with_extension("update").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
