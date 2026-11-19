//! Updates for Silver Skies and Silver Skies Pro from GitHub Releases.
//!
//! [`Updater`] checks the repository's latest release, compares it with the
//! running version and build, downloads the file that fits this app, OS and
//! CPU (checked against GitHub's SHA-256), and installs it:
//!
//! * Windows: runs the Inno Setup installer silently; it restarts the app.
//! * macOS: swaps the `.app` bundle for the one in the release `.zip`.
//! * Linux AppImage: replaces the AppImage file and restarts it.
//! * Linux `.deb`: opens the package in the system's software installer.
//!
//! Anything else (a portable zip, a build directory) gets the release page.
//! The work runs on a Tokio runtime; call [`Updater::poll`] every frame.

pub mod install;
pub mod release;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, TryRecvError};
use tokio::runtime::Handle;

pub use install::Outcome;
pub use release::{App, Install, Release};

/// Automatic checks happen this often while the app runs.
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// The first automatic check waits this long after startup.
pub const FIRST_CHECK_AFTER: Duration = Duration::from_secs(8);

/// Called from worker threads when there is something new to show.
pub type Repaint = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading {
        release: Release,
        total: u64,
    },
    /// The new version is starting; the app should quit.
    Restarting,
    /// Installed by the system's installer; the message says what's next.
    HandedOff(String),
    Error(String),
}

enum Msg {
    Checked(Result<Option<Release>, String>),
    Installed(Result<Outcome, String>),
}

pub struct Updater {
    pub app: App,
    pub install: Install,
    state: State,
    /// The newest release seen, even when it isn't newer than this copy.
    pub latest: Option<Release>,
    rx: Option<Receiver<Msg>>,
    received: Arc<AtomicU64>,
    client: reqwest::Client,
    started: Instant,
    last_check: Option<Instant>,
    /// The last check was started by the person (show "up to date").
    manual: bool,
    repo: String,
}

impl Updater {
    pub fn new(app: App) -> Self {
        Self::with_repo(app, release::REPO)
    }

    pub fn with_repo(app: App, repo: &str) -> Self {
        Self {
            app,
            install: install::detect(app),
            state: State::Idle,
            latest: None,
            rx: None,
            received: Arc::new(AtomicU64::new(0)),
            client: install::client(),
            started: Instant::now(),
            last_check: None,
            manual: false,
            repo: repo.to_string(),
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Whether the last check was asked for by the person (vs automatic).
    pub fn manual(&self) -> bool {
        self.manual
    }

    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    /// Bytes downloaded so far and the total, while downloading.
    pub fn progress(&self) -> Option<(u64, u64)> {
        match &self.state {
            State::Downloading { total, .. } => {
                Some((self.received.load(Ordering::Relaxed), *total))
            }
            _ => None,
        }
    }

    /// Whether this copy can install updates itself (otherwise: open the page).
    pub fn can_install(&self) -> bool {
        match &self.state {
            State::Available(r) => release::pick_asset(r, self.app, &self.install).is_some(),
            _ => false,
        }
    }

    /// An automatic check is due (startup delay passed, a day since the last).
    pub fn auto_check_due(&self) -> bool {
        !self.busy()
            && match self.last_check {
                None => self.started.elapsed() >= FIRST_CHECK_AFTER,
                Some(t) => t.elapsed() >= CHECK_EVERY,
            }
    }

    /// Look for a newer release. `manual` = the person asked (so "up to
    /// date" is worth saying).
    pub fn check(&mut self, rt: &Handle, repaint: Repaint, manual: bool) {
        if self.busy() {
            return;
        }
        self.last_check = Some(Instant::now());
        self.manual = manual;
        self.state = State::Checking;
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.rx = Some(rx);
        let client = self.client.clone();
        let url = release::latest_url(&self.repo);
        rt.spawn(async move {
            let r = fetch_latest(&client, &url).await;
            let _ = tx.send(Msg::Checked(r.map(Some)));
            repaint();
        });
    }

    /// Download and install the available release.
    pub fn start_install(&mut self, rt: &Handle, repaint: Repaint) {
        let State::Available(release) = &self.state else {
            return;
        };
        let Some(asset) = release::pick_asset(release, self.app, &self.install).cloned() else {
            return;
        };
        let release = release.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.rx = Some(rx);
        self.received.store(0, Ordering::Relaxed);
        self.state = State::Downloading {
            release,
            total: asset.size,
        };
        let client = self.client.clone();
        let received = self.received.clone();
        let install = self.install.clone();
        let dir = std::env::temp_dir().join("silverskies-update");
        let tick = repaint.clone();
        rt.spawn(async move {
            // Repaint while the download runs, for the progress bar.
            let ticker = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    tick();
                }
            });
            let r = match install::download(&client, &asset, &dir, received).await {
                Ok(file) => tokio::task::spawn_blocking(move || install::apply(&install, &file))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string())),
                Err(e) => Err(e),
            };
            ticker.abort();
            let _ = tx.send(Msg::Installed(r));
            repaint();
        });
    }

    /// Open the release page (for copies that can't update themselves).
    pub fn open_page(&self) -> Result<(), String> {
        let page = match &self.state {
            State::Available(r) => r.page.clone(),
            _ => format!("https://github.com/{}/releases/latest", self.repo),
        };
        install::open(page)
    }

    /// Pick up finished work. Returns true when the state changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let msg = match rx.try_recv() {
            Ok(m) => m,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                self.state = State::Error("the update was interrupted".into());
                return true;
            }
        };
        self.rx = None;
        self.state = match msg {
            Msg::Checked(Ok(Some(r))) => {
                self.latest = Some(r.clone());
                if release::is_newer(
                    &r.version,
                    ss_core::version::VERSION,
                    ss_core::version::BUILD,
                ) {
                    State::Available(r)
                } else {
                    State::UpToDate
                }
            }
            Msg::Checked(Ok(None)) => State::UpToDate,
            Msg::Checked(Err(e)) => State::Error(e),
            Msg::Installed(Ok(Outcome::Restarting)) => State::Restarting,
            Msg::Installed(Ok(Outcome::HandedOff(m))) => State::HandedOff(m),
            Msg::Installed(Err(e)) => State::Error(e),
        };
        true
    }
}

async fn fetch_latest(client: &reqwest::Client, url: &str) -> Result<Release, String> {
    let resp = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("couldn't reach GitHub: {e}"))?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Err("no releases published yet".into());
    }
    if status.as_u16() == 403 || status.as_u16() == 429 {
        return Err("GitHub is rate limiting update checks; try again later".into());
    }
    if !status.is_success() {
        return Err(format!("GitHub returned HTTP {}", status.as_u16()));
    }
    let text = resp.text().await.map_err(|e| e.to_string())?;
    release::parse_release(&text)
}

/// "8.4 MB"
pub fn fmt_bytes(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1} MB", n as f64 / 1_000_000.0)
    } else {
        format!("{:.0} KB", n as f64 / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes() {
        assert_eq!(fmt_bytes(8_781_395), "8.8 MB");
        assert_eq!(fmt_bytes(51_200), "51 KB");
    }

    #[test]
    fn checks_against_a_local_server() {
        // A tiny HTTP server that serves one release JSON.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for stream in listener.incoming().take(1) {
                let mut s = stream.unwrap();
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let body = r#"{"tag_name":"v99.0.0.1","html_url":"https://x","assets":[]}"#;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
            }
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut u = Updater::with_repo(App::Simple, "x/y");
        // Point the check at the local server.
        let url = format!("http://127.0.0.1:{port}/latest");
        let (tx, rx) = crossbeam_channel::bounded(1);
        u.rx = Some(rx);
        u.state = State::Checking;
        let client = u.client.clone();
        rt.spawn(async move {
            let _ = tx.send(Msg::Checked(fetch_latest(&client, &url).await.map(Some)));
        });
        for _ in 0..100 {
            if u.poll() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        match u.state() {
            State::Available(r) => assert_eq!(r.tag, "v99.0.0.1"),
            s => panic!("unexpected state {s:?}"),
        }
        // No files for this platform in that release: can't self-install.
        assert!(!u.can_install());
    }
}
