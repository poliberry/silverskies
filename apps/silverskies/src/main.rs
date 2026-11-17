//! Silver Skies — a simple, calm weather forecast app.
//!
//! Forecast at a glance, a plain-language summary, fuzzy search for places
//! and keywords, a simple radar loop and official WMO alerts. AI summaries
//! and answers are optional (bring your own Anthropic API key).

mod ai;
mod app;
mod menu;
mod prefs;
mod radar;
mod search;
mod searchbar;
mod settings;
mod summary;
mod theme;
mod views;
mod worker;

use ss_store::Store;
use tracing_subscriber::EnvFilter;

const ICON_DARK: &[u8] = include_bytes!("../assets/icon-dark.png");
const ICON_LIGHT: &[u8] = include_bytes!("../assets/icon-light.png");

pub fn app_icon(dark: bool) -> std::sync::Arc<egui::IconData> {
    ss_assets::icon_data(if dark { ICON_DARK } else { ICON_LIGHT })
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Background network work runs on this runtime; it must outlive the UI.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let handle = rt.handle().clone();
    let _guard = rt.enter();

    let store = Store::open()?;
    store.migrate_legacy();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Silver Skies")
            .with_app_id("silverskies")
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([760.0, 520.0])
            .with_icon(app_icon(true)),
        ..Default::default()
    };
    eframe::run_native(
        "silverskies",
        options,
        Box::new(move |cc| {
            let app = app::App::new(cc, handle, store).map_err(|e| e.to_string())?;
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_icons_decode() {
        for dark in [true, false] {
            let i = super::app_icon(dark);
            assert!(i.width > 1 && i.rgba.len() == (i.width * i.height * 4) as usize);
        }
    }
}
