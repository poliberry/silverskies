mod app;
mod audio;
mod chrome;
mod cmdbar;
mod commands;
mod events;
mod header;
mod icons;
mod layout;
mod panels;
mod settings;
mod state;
mod tasks;
mod theme;
mod windows;

use ss_store::Store;
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,wgpu=warn,naga=warn")),
        )
        .init();

    // The runtime must outlive the event loop: background tasks run on it.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let handle = rt.handle().clone();
    let _guard = rt.enter();

    // Pro keeps its own config directory. On first run after the split it
    // starts from the setup the single app had (the "Silver Skies" dir).
    let store = Store::open_app(ss_store::APP_PRO)?;
    if let Ok(old) = Store::app_dir(ss_store::APP_SILVER_SKIES) {
        store.import_from(&old);
    }
    store.migrate_legacy();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Silver Skies Pro")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([640.0, 400.0])
        .with_icon(icons::app_icon(true));
    if let Some(b) = store
        .load_session()
        .main_bounds
        .and_then(windows::sane_bounds)
    {
        viewport = viewport
            .with_position([b.x, b.y])
            .with_inner_size([b.width, b.height]);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "silverskies-pro",
        options,
        Box::new(move |cc| {
            let app = app::App::new(cc, handle, store).map_err(|e| e.to_string())?;
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
