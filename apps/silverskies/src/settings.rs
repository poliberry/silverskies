//! The settings window: units and time, appearance and motion, the weather
//! source (Open-Meteo or AccuWeather), the optional AI, and developer tools.

use egui::{Context, RichText, Ui};
use ss_core::theme::builtin_themes;
use ss_core::{Config, ThemePref, TimeFormat, UnitPref, WeatherProviderId};

use crate::app::App;
use crate::theme::{c32, c32a, scheme};

/// Auto-refresh choices in minutes; 0 = off.
const REFRESH_CHOICES: [u32; 7] = [5, 10, 15, 30, 60, 120, 0];

fn refresh_label(m: u32) -> String {
    match m {
        0 => "Off".into(),
        60 => "1 hour".into(),
        120 => "2 hours".into(),
        m => format!("{m} min"),
    }
}

/// The colour theme choices: the sky-matched default, then the builtins.
fn theme_choices() -> Vec<(String, String)> {
    let mut v = vec![("default".to_string(), "Match the sky".to_string())];
    v.extend(builtin_themes().into_iter().map(|t| (t.id, t.name)));
    v
}

fn section(ui: &mut Ui, app: &App, title: &str) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(title)
            .font(crate::theme::medium_font(15.0))
            .color(scheme(&app.palette).primary),
    );
}

fn row_label(ui: &mut Ui, app: &App, text: &str) {
    ui.label(RichText::new(text).color(c32a(app.palette.text2)));
}

/// What changed, so the app can react (refresh, save).
#[derive(Default)]
struct Changes {
    config: bool,
    prefs: bool,
}

pub fn window(ctx: &Context, app: &mut App) {
    let mut open = app.settings_open;
    let mut ch = Changes::default();
    let before = app.config.clone();
    // Fit the screen: the content scrolls inside a dialog no taller than
    // the window (minus margins, the title bar and padding).
    let max_h = (ctx.content_rect().height() - 190.0).max(200.0);
    egui::Window::new(RichText::new("Settings").size(18.0).strong())
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(520.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .max_height(max_h)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(max_h)
                .min_scrolled_height(max_h)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.set_width(500.0);
                    ui.spacing_mut().item_spacing.y = 10.0;
                    units_and_time(ui, app, &mut ch);
                    ui.separator();
                    appearance(ui, app, &mut ch);
                    ui.separator();
                    weather_source(ui, app, &mut ch);
                    ui.separator();
                    ai(ui, app, &mut ch);
                    ui.separator();
                    updates(ui, app, &mut ch);
                    ui.separator();
                    developer(ui, app);
                    ui.label(
                        RichText::new(format!(
                            "Settings are stored on this computer in {}",
                            app.store_dir()
                        ))
                        .size(11.5)
                        .color(c32a(app.palette.text3)),
                    );
                });
        });
    app.settings_open = open;
    if ch.config {
        app.save_config();
        app.after_config_change(&before);
    }
    if ch.prefs {
        app.save_prefs();
    }
}

fn units_and_time(ui: &mut Ui, app: &mut App, ch: &mut Changes) {
    section(ui, app, "Units and time");
    egui::Grid::new("settings_units")
        .num_columns(2)
        .spacing([18.0, 12.0])
        .show(ui, |ui| {
            row_label(ui, app, "Temperature");
            ui.horizontal(|ui| {
                for (u, l) in [(UnitPref::F, "°F, mph"), (UnitPref::C, "°C, km/h")] {
                    ch.config |= ui.selectable_value(&mut app.config.units, u, l).changed();
                }
            });
            ui.end_row();

            row_label(ui, app, "Clock");
            ui.horizontal(|ui| {
                for (t, l) in [(TimeFormat::H12, "12-hour"), (TimeFormat::H24, "24-hour")] {
                    ch.config |= ui
                        .selectable_value(&mut app.config.time_format, t, l)
                        .changed();
                }
            });
            ui.end_row();
        });
}

fn appearance(ui: &mut Ui, app: &mut App, ch: &mut Changes) {
    section(ui, app, "Appearance");
    egui::Grid::new("settings_appearance")
        .num_columns(2)
        .spacing([18.0, 12.0])
        .show(ui, |ui| {
            row_label(ui, app, "Mode");
            ui.horizontal(|ui| {
                for (v, l) in [
                    (ThemePref::System, "System"),
                    (ThemePref::Light, "Light"),
                    (ThemePref::Dark, "Dark"),
                ] {
                    ch.config |= ui.selectable_value(&mut app.config.theme, v, l).changed();
                }
            });
            ui.end_row();

            row_label(ui, app, "Colors");
            let choices = theme_choices();
            let current = choices
                .iter()
                .find(|(id, _)| *id == app.config.theme_id)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| "Custom".into());
            egui::ComboBox::from_id_salt("theme_id")
                .selected_text(current)
                .width(220.0)
                .show_ui(ui, |ui| {
                    for (id, name) in &choices {
                        ch.config |= ui
                            .selectable_value(&mut app.config.theme_id, id.clone(), name)
                            .changed();
                    }
                });
            ui.end_row();

            row_label(ui, app, "Color from");
            ui.horizontal(|ui| {
                use crate::prefs::ColorSource;
                for (v, l) in [
                    (ColorSource::Weather, "The weather"),
                    (ColorSource::System, "System accent color"),
                ] {
                    ch.prefs |= ui
                        .selectable_value(&mut app.prefs.color_source, v, l)
                        .changed();
                }
            });
            ui.end_row();

            row_label(ui, app, "Temperature card");
            ui.horizontal(|ui| {
                use crate::prefs::HeroStyle;
                for (v, l) in [
                    (HeroStyle::Sky, "Animated sky"),
                    (HeroStyle::Tonal, "Plain"),
                ] {
                    ch.prefs |= ui
                        .selectable_value(&mut app.prefs.hero_style, v, l)
                        .changed();
                }
            });
            ui.end_row();

            row_label(ui, app, "Background");
            ch.prefs |= ui
                .checkbox(&mut app.prefs.bing_wallpaper, "Bing picture of the day")
                .changed();
            ui.end_row();

            row_label(ui, app, "Top glow");
            ch.config |= ui
                .checkbox(
                    &mut app.config.top_glow_enabled,
                    "Accent glow along the top",
                )
                .changed();
            ui.end_row();

            row_label(ui, app, "Menu bar");
            ch.config |= ui
                .checkbox(
                    &mut app.config.menu_bar_always,
                    "Always show (otherwise press Alt)",
                )
                .changed();
            ui.end_row();

            row_label(ui, app, "Motion");
            ch.prefs |= ui
                .checkbox(
                    &mut app.prefs.reduce_motion,
                    "Reduce motion (no entrance or morph animations)",
                )
                .changed();
            ui.end_row();
        });
    if app.config.theme_id != "default" {
        ui.label(
            RichText::new(
                "Named color themes are dark; the Mode setting applies with Match the sky.",
            )
            .size(12.0)
            .color(c32a(app.palette.text2)),
        );
    }
}

fn weather_source(ui: &mut Ui, app: &mut App, ch: &mut Changes) {
    section(ui, app, "Weather");
    egui::Grid::new("settings_weather")
        .num_columns(2)
        .spacing([18.0, 12.0])
        .show(ui, |ui| {
            row_label(ui, app, "Forecast source");
            ui.horizontal(|ui| {
                for (v, l) in [
                    (WeatherProviderId::OpenMeteo, "Open-Meteo"),
                    (WeatherProviderId::AccuWeather, "AccuWeather"),
                ] {
                    ch.config |= ui
                        .selectable_value(&mut app.config.provider, v, l)
                        .changed();
                }
            });
            ui.end_row();

            if app.config.provider == WeatherProviderId::AccuWeather {
                row_label(ui, app, "AccuWeather key");
                ch.config |= key_field(ui, &mut app.config);
                ui.end_row();
            }

            row_label(ui, app, "Refresh every");
            egui::ComboBox::from_id_salt("refresh")
                .selected_text(refresh_label(app.config.auto_refresh_minutes))
                .show_ui(ui, |ui| {
                    for m in REFRESH_CHOICES {
                        ch.config |= ui
                            .selectable_value(
                                &mut app.config.auto_refresh_minutes,
                                m,
                                refresh_label(m),
                            )
                            .changed();
                    }
                });
            ui.end_row();
        });
    let note = match (
        app.config.provider,
        app.config
            .accu_weather_api_key
            .as_deref()
            .is_some_and(|k| !k.trim().is_empty()),
    ) {
        (WeatherProviderId::OpenMeteo, _) => "Open-Meteo is free and needs no key.".to_string(),
        (WeatherProviderId::AccuWeather, false) => {
            "AccuWeather needs an API key from developer.accuweather.com. Until one is added, \
             forecasts come from Open-Meteo."
                .to_string()
        }
        (WeatherProviderId::AccuWeather, true) => {
            "Free AccuWeather keys allow 50 requests a day and each place uses 3 per refresh, \
             so AccuWeather forecasts are reused for an hour. AccuWeather gives 12 hours and \
             5 days ahead."
                .to_string()
        }
    };
    ui.label(
        RichText::new(note)
            .size(12.0)
            .color(c32a(app.palette.text2)),
    );
}

/// The AccuWeather key: hidden by default, "Show" reveals it (and lets it
/// be copied); pasting works either way.
fn key_field(ui: &mut Ui, cfg: &mut Config) -> bool {
    let show_id = ui.make_persistent_id("accu_key_show");
    let mut show = ui.data(|d| d.get_temp::<bool>(show_id).unwrap_or(false));
    let mut key = cfg.accu_weather_api_key.clone().unwrap_or_default();
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui
            .add(
                egui::TextEdit::singleline(&mut key)
                    .password(!show)
                    .hint_text("API key")
                    .desired_width(220.0),
            )
            .changed()
        {
            let k = key.trim().to_string();
            cfg.accu_weather_api_key = (!k.is_empty()).then_some(k);
            changed = true;
        }
        if ui.checkbox(&mut show, "Show").changed() {
            ui.data_mut(|d| d.insert_temp(show_id, show));
        }
    });
    changed
}

fn ai(ui: &mut Ui, app: &mut App, ch: &mut Changes) {
    section(ui, app, "✦ AI");
    let p = app.palette;
    ch.config |=
        ss_ai::settings_ui::ai_settings_form(ui, &mut app.config.ai, c32a(p.text2), c32(p.danger));
    ch.prefs |= ui
        .add_enabled(
            app.config.ai.enabled,
            egui::Checkbox::new(
                &mut app.prefs.ai_auto_summary,
                "Write an AI summary automatically when a forecast loads",
            ),
        )
        .changed();
}

fn updates(ui: &mut Ui, app: &mut App, ch: &mut Changes) {
    use ss_update::State;
    section(ui, app, "Updates");
    let p = app.palette;
    let s = scheme(&p);
    ch.config |= ui
        .checkbox(
            &mut app.config.check_for_updates,
            "Check for updates automatically (at startup and once a day)",
        )
        .changed();
    let state = app.updater.state().clone();
    let status = match &state {
        State::Idle => format!(
            "You have Silver Skies {}.",
            ss_core::version::version_label()
        ),
        State::Checking => "Checking for updates…".to_string(),
        State::UpToDate => format!(
            "Silver Skies {} is up to date.",
            ss_core::version::version_label()
        ),
        State::Available(r) => format!(
            "Silver Skies {} is available (you have {}).",
            r.label(),
            ss_core::version::version_label()
        ),
        State::Downloading { release, .. } => format!("Downloading {}…", release.label()),
        State::Restarting => "Restarting into the new version…".to_string(),
        State::HandedOff(m) => m.clone(),
        State::Error(e) => format!("Couldn't update: {e}"),
    };
    let color = if matches!(state, State::Error(_)) {
        s.error
    } else {
        s.on_surface_variant
    };
    ui.label(RichText::new(status).color(color));
    if let Some((done, total)) = app.updater.progress() {
        let f = if total > 0 {
            done as f32 / total as f32
        } else {
            0.0
        };
        ui.add(
            egui::ProgressBar::new(f.clamp(0.0, 1.0))
                .desired_height(6.0)
                .fill(s.primary),
        );
    }
    ui.horizontal(|ui| match &state {
        State::Available(r) => {
            let tag = r.tag.clone();
            let label = if app.updater.can_install() {
                "Update and restart"
            } else {
                "Download"
            };
            if ui
                .add(egui::Button::new(RichText::new(label).color(s.on_primary)).fill(s.primary))
                .clicked()
            {
                app.install_update();
            }
            if ui.button("Release notes").clicked() {
                let _ = app.updater.open_page();
            }
            if ui.button("Skip this version").clicked() {
                app.skip_update(&tag);
            }
        }
        State::Checking | State::Downloading { .. } | State::Restarting => {
            ui.add(egui::Spinner::new().color(s.primary));
        }
        _ => {
            if ui.button("Check for updates").clicked() {
                app.check_for_updates(true);
            }
        }
    });
}

fn developer(ui: &mut Ui, app: &mut App) {
    let p = app.palette;
    egui::CollapsingHeader::new("Developer")
        .default_open(false)
        .show(ui, |ui| {
            ui.label(
                RichText::new(
                    "Add a local demo alert to preview how alerts look. Demo alerts are never saved.",
                )
                .size(12.5)
                .color(c32a(p.text2)),
            );
            let id = egui::Id::unique("demo_pick");
            let mut pick: (usize, usize) = ui.data_mut(|d| *d.get_temp_mut_or(id, (0, 0)));
            let groups = ss_core::alerts::demo::DEMO_GROUPS;
            let current = groups
                .get(pick.0)
                .and_then(|g| g.items.get(pick.1))
                .map(|i| i.0)
                .unwrap_or("—");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("demo_alert")
                    .selected_text(current)
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for (gi, g) in groups.iter().enumerate() {
                            ui.label(RichText::new(g.name).strong().color(c32(p.a0)));
                            for (ii, (name, _)) in g.items.iter().enumerate() {
                                ui.selectable_value(&mut pick, (gi, ii), *name);
                            }
                        }
                    });
                if ui.button("Add alert").clicked() {
                    if let Some((name, sev)) =
                        groups.get(pick.0).and_then(|g| g.items.get(pick.1))
                    {
                        app.demo_alerts.insert(
                            0,
                            ss_core::alerts::demo::build_demo_alert(name, *sev, chrono::Utc::now()),
                        );
                    }
                }
                if ui
                    .add_enabled(!app.demo_alerts.is_empty(), egui::Button::new("Clear"))
                    .clicked()
                {
                    app.demo_alerts.clear();
                }
            });
            ui.data_mut(|d| d.insert_temp(id, pick));
            ui.add_space(8.0);
            ui.label(
                RichText::new("Preview the temperature card's sky for another condition (not saved).")
                    .size(12.5)
                    .color(c32a(p.text2)),
            );
            ui.horizontal(|ui| {
                const SKIES: [(u8, &str); 8] = [
                    (0, "Clear"),
                    (2, "Partly cloudy"),
                    (3, "Overcast"),
                    (45, "Fog"),
                    (53, "Drizzle"),
                    (63, "Rain"),
                    (73, "Snow"),
                    (95, "Thunderstorm"),
                ];
                let (mut code, mut day) = app.demo_sky.unwrap_or((0, true));
                let name = SKIES.iter().find(|(c, _)| *c == code).map_or("—", |s| s.1);
                let before = (code, day);
                egui::ComboBox::from_id_salt("demo_sky")
                    .selected_text(if app.demo_sky.is_some() { name } else { "Real weather" })
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        for (c, n) in SKIES {
                            ui.selectable_value(&mut code, c, n);
                        }
                    });
                ui.checkbox(&mut day, "Day");
                if (code, day) != before || (app.demo_sky.is_none() && (code, day) != (0, true)) {
                    app.demo_sky = Some((code, day));
                }
                if ui
                    .add_enabled(app.demo_sky.is_some(), egui::Button::new("Real weather"))
                    .clicked()
                {
                    app.demo_sky = None;
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_choices_start_with_the_sky_default() {
        let c = theme_choices();
        assert_eq!(c[0].0, "default");
        assert!(c.len() > 1);
    }
}
