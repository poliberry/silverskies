//! The classic menu bar — File, Edit, View, Help — revealed with Alt like a
//! native Windows app (View → Always Show Menu Bar pins it), plus the
//! keyboard shortcuts it lists.

use egui::{Context, Key, KeyboardShortcut, Modifiers, RichText, Ui};
use ss_core::{ThemePref, TimeFormat, UnitPref};

use crate::app::{Action, App, Tab};
use crate::search::{Intent, Units};
use crate::theme::{c32, c32a};

const REFRESH: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F5);
const REFRESH_R: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::R);
const FIND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
const SETTINGS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma);
const ASSISTANT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::J);
const QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
const TAB_FORECAST: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1);
const TAB_RADAR: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num2);
const TAB_ALERTS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num3);

/// Handle the shortcuts the menus advertise.
pub fn shortcuts(ctx: &Context, app: &mut App, actions: &mut Vec<Action>) {
    let hit = |s: &KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(s));
    if hit(&REFRESH) || hit(&REFRESH_R) {
        actions.push(Action::Intent(Intent::Refresh));
    }
    if hit(&FIND) {
        app.focus_search();
    }
    if hit(&SETTINGS) {
        actions.push(Action::OpenSettings);
    }
    if hit(&QUIT) {
        app.quit();
    }
    if hit(&ASSISTANT) {
        actions.push(Action::OpenAssistant);
    }
    for (s, tab) in [
        (&TAB_FORECAST, Tab::Forecast),
        (&TAB_RADAR, Tab::Radar),
        (&TAB_ALERTS, Tab::Alerts),
    ] {
        if hit(s) {
            actions.push(Action::Tab(tab));
        }
    }
}

fn shortcut_text(ctx: &Context, s: &KeyboardShortcut) -> String {
    ctx.format_shortcut(s)
}

/// A menu item with its shortcut shown on the right, native style.
fn item(ui: &mut Ui, label: &str, shortcut: Option<&KeyboardShortcut>) -> bool {
    let mut b = egui::Button::new(label);
    if let Some(s) = shortcut {
        b = b.shortcut_text(shortcut_text(ui.ctx(), s));
    }
    let clicked = ui.add(b).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

pub fn bar(ui: &mut Ui, app: &mut App, actions: &mut Vec<Action>) {
    let p = app.palette;
    let mut chose = false;
    egui::Panel::top("menubar")
        .resizable(false)
        .frame(
            egui::Frame::NONE
                .fill(c32(p.surface))
                .stroke(egui::Stroke::new(1.0, c32a(p.border)))
                .inner_margin(egui::Margin::symmetric(8, 2)),
        )
        .show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if item(ui, "Use My Location", None) {
                        actions.push(Action::Intent(Intent::UseMyLocation));
                        chose = true;
                    }
                    let current = app.active_id();
                    if ui
                        .add_enabled(current.is_some(), egui::Button::new("Remove Current Place"))
                        .clicked()
                    {
                        if let Some(id) = current {
                            actions.push(Action::Remove(id));
                        }
                        ui.close();
                        chose = true;
                    }
                    ui.separator();
                    if item(ui, "Refresh", Some(&REFRESH)) {
                        actions.push(Action::Intent(Intent::Refresh));
                        chose = true;
                    }
                    if item(ui, "Settings…", Some(&SETTINGS)) {
                        actions.push(Action::OpenSettings);
                        chose = true;
                    }
                    ui.separator();
                    if item(ui, "Quit", Some(&QUIT)) {
                        app.quit();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if item(ui, "Find Place or Keyword…", Some(&FIND)) {
                        app.focus_search();
                        chose = true;
                    }
                    if let Some(w) = app.active_weather() {
                        if item(ui, "Copy Forecast Summary", None) {
                            let place = app.active().map(|l| l.label.clone()).unwrap_or_default();
                            let s = crate::summary::summarize(
                                w,
                                &app.active_alerts(),
                                app.config.units,
                                app.config.time_format,
                                App::now_for(w),
                            );
                            let text = format!("{place}: {} {}", s.headline, s.points.join(" "));
                            ui.ctx().copy_text(text);
                            chose = true;
                        }
                    }
                });
                ui.menu_button("View", |ui| {
                    for (tab, label, s) in [
                        (Tab::Forecast, "Forecast", &TAB_FORECAST),
                        (Tab::Radar, "Radar", &TAB_RADAR),
                        (Tab::Alerts, "Alerts", &TAB_ALERTS),
                    ] {
                        let b = egui::Button::selectable(app.tab == tab, label)
                            .shortcut_text(shortcut_text(ui.ctx(), s));
                        if ui.add(b).clicked() {
                            actions.push(Action::Tab(tab));
                            ui.close();
                            chose = true;
                        }
                    }
                    ui.separator();
                    if item(ui, "Assistant…", Some(&ASSISTANT)) {
                        actions.push(Action::OpenAssistant);
                        chose = true;
                    }
                    ui.separator();
                    let mut changed = false;
                    ui.menu_button("Units", |ui| {
                        for (u, label) in [(UnitPref::F, "°F, mph"), (UnitPref::C, "°C, km/h")] {
                            if ui.radio(app.config.units == u, label).clicked() {
                                actions.push(Action::Intent(Intent::Units(match u {
                                    UnitPref::F => Units::Fahrenheit,
                                    UnitPref::C => Units::Celsius,
                                })));
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button("Clock", |ui| {
                        for (t, label) in
                            [(TimeFormat::H12, "12-hour"), (TimeFormat::H24, "24-hour")]
                        {
                            if ui
                                .radio_value(&mut app.config.time_format, t, label)
                                .changed()
                            {
                                changed = true;
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button("Appearance", |ui| {
                        for (t, label) in [
                            (ThemePref::System, "Match System"),
                            (ThemePref::Light, "Light"),
                            (ThemePref::Dark, "Dark"),
                        ] {
                            if ui.radio_value(&mut app.config.theme, t, label).changed() {
                                changed = true;
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if ui
                        .checkbox(&mut app.config.top_glow_enabled, "Top Glow")
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .checkbox(&mut app.config.menu_bar_always, "Always Show Menu Bar")
                        .changed()
                    {
                        changed = true;
                    }
                    if changed {
                        app.save_config();
                    }
                });
                ui.menu_button("Help", |ui| {
                    if item(ui, "About Silver Skies", None) {
                        app.about_open = true;
                        app.blur_search();
                        chose = true;
                    }
                    if item(ui, "Silver Skies Pro…", None) {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(
                            "https://github.com/poliberry/silverskies/tree/main/apps/pro",
                        ));
                        chose = true;
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !app.config.menu_bar_always {
                        ui.label(RichText::new("Alt to hide").size(11.0).color(c32a(p.text3)));
                    }
                });
            });
        });
    if chose {
        app.menu.close();
    }
}

pub fn about(ctx: &Context, app: &mut App) {
    let p = app.palette;
    let mut open = app.about_open;
    egui::Window::new("About Silver Skies")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            let logo = crate::theme::logomark(ui, &p, 56.0);
            ui.add(logo);
            ui.label(crate::theme::label(
                &format!("Version {}", env!("CARGO_PKG_VERSION")),
                13.0,
                c32a(p.text3),
            ));
            ui.add_space(6.0);
            ui.label("A simple, calm weather forecast.");
            ui.label(
                RichText::new(
                    "Forecasts: Open-Meteo · Radar and WMO alerts: LibreWXR · Maps: Esri, \
                     OpenStreetMap · Weather icons: Google (V1 set) · Fonts: Google Sans, \
                     DejaVu.",
                )
                .size(12.5)
                .color(c32a(p.text2)),
            );
        });
    app.about_open = open;
}
