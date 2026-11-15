//! The settings window: units, time, theme, refresh and the optional AI.

use egui::{Context, RichText};
use ss_core::{ThemePref, TimeFormat, UnitPref};

use crate::app::App;
use crate::theme::{c32, c32a};

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

pub fn window(ctx: &Context, app: &mut App) {
    let p = app.palette;
    let mut open = app.settings_open;
    let mut config_changed = false;
    let mut prefs_changed = false;
    egui::Window::new(RichText::new("Settings").size(18.0).strong())
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(440.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            egui::Grid::new("settings_grid")
                .num_columns(2)
                .spacing([18.0, 12.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Temperature").color(c32a(p.text2)));
                    ui.horizontal(|ui| {
                        config_changed |= ui
                            .selectable_value(&mut app.config.units, UnitPref::F, "°F, mph")
                            .changed();
                        config_changed |= ui
                            .selectable_value(&mut app.config.units, UnitPref::C, "°C, km/h")
                            .changed();
                    });
                    ui.end_row();

                    ui.label(RichText::new("Clock").color(c32a(p.text2)));
                    ui.horizontal(|ui| {
                        config_changed |= ui
                            .selectable_value(&mut app.config.time_format, TimeFormat::H12, "12-hour")
                            .changed();
                        config_changed |= ui
                            .selectable_value(&mut app.config.time_format, TimeFormat::H24, "24-hour")
                            .changed();
                    });
                    ui.end_row();

                    ui.label(RichText::new("Appearance").color(c32a(p.text2)));
                    ui.horizontal(|ui| {
                        for (v, l) in [
                            (ThemePref::System, "System"),
                            (ThemePref::Light, "Light"),
                            (ThemePref::Dark, "Dark"),
                        ] {
                            config_changed |=
                                ui.selectable_value(&mut app.config.theme, v, l).changed();
                        }
                    });
                    ui.end_row();

                    ui.label(RichText::new("Refresh every").color(c32a(p.text2)));
                    egui::ComboBox::from_id_salt("refresh")
                        .selected_text(refresh_label(app.config.auto_refresh_minutes))
                        .show_ui(ui, |ui| {
                            for m in REFRESH_CHOICES {
                                config_changed |= ui
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

            ui.separator();
            ui.label(RichText::new("✦ AI summaries & answers").size(16.0).strong());
            ui.label(
                RichText::new(
                    "Optional. Writes forecast summaries and answers questions you type in the \
                     search bar, using Claude with your own Anthropic API key. Your forecast for \
                     the selected place is sent with each request; nothing else is.",
                )
                .size(12.5)
                .color(c32a(p.text2)),
            );
            prefs_changed |= ui
                .checkbox(&mut app.prefs.ai_enabled, "Turn on AI")
                .changed();
            ui.add_enabled_ui(app.prefs.ai_enabled, |ui| {
                egui::Grid::new("ai_grid")
                    .num_columns(2)
                    .spacing([18.0, 10.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("API key").color(c32a(p.text2)));
                        let key = app.prefs.anthropic_api_key.get_or_insert_with(String::new);
                        prefs_changed |= ui
                            .add(
                                egui::TextEdit::singleline(key)
                                    .password(true)
                                    .hint_text("sk-ant-…")
                                    .desired_width(260.0),
                            )
                            .changed();
                        ui.end_row();

                        ui.label(RichText::new("Model").color(c32a(p.text2)));
                        prefs_changed |= ui
                            .add(egui::TextEdit::singleline(&mut app.prefs.ai_model).desired_width(260.0))
                            .changed();
                        ui.end_row();
                    });
                prefs_changed |= ui
                    .checkbox(
                        &mut app.prefs.ai_auto_summary,
                        "Write an AI summary automatically when a forecast loads",
                    )
                    .changed();
            });
            if app.prefs.ai_enabled && app.prefs.ai_key().is_none() {
                ui.label(RichText::new("Add an API key to use AI.").color(c32(p.danger)));
            }
            ui.separator();
            egui::CollapsingHeader::new("Developer")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Add a local demo alert to preview how alerts look. Demo alerts are never saved.")
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
                            if let Some((name, sev)) = groups.get(pick.0).and_then(|g| g.items.get(pick.1)) {
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
                });
            ui.label(
                RichText::new(format!("Settings are stored on this computer in {}", app.store_dir()))
                    .size(11.5)
                    .color(c32a(p.text3)),
            );
        });
    app.settings_open = open;
    if config_changed {
        app.save_config();
    }
    if prefs_changed {
        app.save_prefs();
    }
}
