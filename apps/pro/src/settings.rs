//! The editable settings window: tabbed, terminal-styled, and wired to
//! `Shared.config`. Every edit goes through `apply_config_change`, which marks
//! the config dirty and requests the follow-up work (palette, radar layer,
//! radar host).

use std::collections::BTreeMap;

use egui::{RichText, Ui};
use ss_core::ai::{AiProvider, AiSettings, AI_DISCLAIMER};
use ss_core::alerts::colors::{class_color, effective_color, effective_visible, ALL_CLASSES};
use ss_core::alerts::demo::DEMO_GROUPS;
use ss_core::command::Command;
use ss_core::theme::{builtin_themes, parse_color, resolve_palette, Palette};
use ss_core::{
    AlertTypeOverride, Config, LiveFeedMode, RadarProduct, RadioMode, ThemeColors, ThemeDefinition,
    ThemePref, TimeFormat, TtsSeverityFilter, UnitPref, WeatherProviderId,
};

use crate::kit::{self, divider, Rows};
use crate::state::Shared;
use crate::theme::{c32, c32a, saira};
use crate::windows::config_text;

const REPO_URL: &str = "https://github.com/poliberry/silverskies-desktop";
/// Fixed accent for theme previews.
const PREVIEW_ACCENT: [u8; 3] = [0x7f, 0xb4, 0xff];
const REFRESH_CHOICES: [u32; 7] = [0, 5, 10, 15, 30, 60, 120];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    General,
    Sources,
    Radar,
    Alerts,
    Radio,
    Ai,
    Themes,
    Developer,
    About,
}

impl Tab {
    const ALL: [Tab; 9] = [
        Tab::General,
        Tab::Sources,
        Tab::Radar,
        Tab::Alerts,
        Tab::Radio,
        Tab::Ai,
        Tab::Themes,
        Tab::Developer,
        Tab::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Sources => "Data sources",
            Tab::Radar => "Radar",
            Tab::Alerts => "Alerts",
            Tab::Radio => "Radio & speech",
            Tab::Ai => "AI assistant",
            Tab::Themes => "Themes",
            Tab::Developer => "Developer",
            Tab::About => "About",
        }
    }

    /// Page title and description.
    fn header(self) -> (&'static str, &'static str) {
        match self {
            Tab::General => ("General", "Forecast source, units and how Pro behaves."),
            Tab::Sources => (
                "Data sources",
                "API keys and servers for optional data feeds.",
            ),
            Tab::Radar => ("Radar", "The radar loop and the layers drawn over it."),
            Tab::Alerts => (
                "Alerts",
                "Colour and visibility for each type of weather alert.",
            ),
            Tab::Radio => (
                "Radio & speech",
                "Weather radio and spoken alert announcements.",
            ),
            Tab::Ai => (
                "AI assistant",
                "Optional AI forecast summaries and chat, with your own provider.",
            ),
            Tab::Themes => ("Themes", "Colour themes, including your own."),
            Tab::Developer => ("Developer", "Tools for testing Silver Skies Pro."),
            Tab::About => ("About", "Version, files and key commands."),
        }
    }
}

/// Per-window UI state (not persisted).
#[derive(Default)]
pub struct SettingsState {
    pub tab: Tab,
    alert_filter: String,
    theme_json: String,
    theme_path: String,
    /// `(is_error, text)` from the last theme import.
    notice: Option<(bool, String)>,
    demo_group: usize,
    demo_item: usize,
    /// Pending text of the LibreWXR host box until it loses focus.
    host_buf: Option<String>,
    site_buf: Option<String>,
    show_accu: bool,
    show_willy: bool,
    show_owm: bool,
    show_ai_key: bool,
}

// ---------------------------------------------------------------------------
// Pure helpers (unit tested)
// ---------------------------------------------------------------------------

fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// Drop the entry when it carries no information.
fn prune_override(map: &mut BTreeMap<String, AlertTypeOverride>, class: &str) {
    let empty = map
        .get(class)
        .is_some_and(|o| o.color.is_none() && o.visible.unwrap_or(true));
    if empty {
        map.remove(class);
    }
}

/// Set a class color; the default color clears the override.
pub fn set_override_color(
    map: &mut BTreeMap<String, AlertTypeOverride>,
    class: &str,
    rgb: [u8; 3],
) {
    let color = (rgb != class_color(class)).then(|| hex(rgb));
    map.entry(class.to_string()).or_default().color = color;
    prune_override(map, class);
}

/// Show or hide a class; "visible" is the default so it clears the field.
pub fn set_override_visible(
    map: &mut BTreeMap<String, AlertTypeOverride>,
    class: &str,
    visible: bool,
) {
    map.entry(class.to_string()).or_default().visible = (!visible).then_some(false);
    prune_override(map, class);
}

pub fn reset_override(map: &mut BTreeMap<String, AlertTypeOverride>, class: &str) {
    map.remove(class);
}

/// Empty / whitespace-only strings are stored as `None`.
pub fn opt_string(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Whether the system/light/dark preference has any effect with this config.
pub fn theme_pref_applies(cfg: &Config) -> bool {
    match cfg.theme_id.as_str() {
        "custom" => cfg.custom_theme.is_none(),
        id => !builtin_themes().iter().any(|t| t.id == id),
    }
}

fn slots(c: &ThemeColors) -> [(&'static str, &Option<String>); 10] {
    [
        ("a0", &c.a0),
        ("bg", &c.bg),
        ("surface", &c.surface),
        ("surface3", &c.surface3),
        ("border", &c.border),
        ("text", &c.text),
        ("text2", &c.text2),
        ("text3", &c.text3),
        ("danger", &c.danger),
        ("safe", &c.safe),
    ]
}

/// Parse a pasted theme: `{ "name", "colors": {..} }`, or a bare colors
/// object. Unknown keys are ignored; every known slot must hold a parseable
/// color.
pub fn parse_theme_json(s: &str) -> Result<ThemeDefinition, String> {
    let v: serde_json::Value = serde_json::from_str(s).map_err(|e| format!("invalid JSON: {e}"))?;
    let obj = v.as_object().ok_or("expected a JSON object")?;
    let colors_val = match obj.get("colors") {
        Some(c) => c.clone(),
        None => v.clone(),
    };
    let colors: ThemeColors = serde_json::from_value(colors_val)
        .map_err(|e| format!("colors must map slot names to strings: {e}"))?;
    if slots(&colors).iter().all(|(_, v)| v.is_none()) {
        return Err("no colors found (expected slots like a0, bg, surface, text)".to_string());
    }
    for (slot, val) in slots(&colors) {
        if let Some(raw) = val {
            if parse_color(raw).is_none() {
                return Err(format!("invalid color for {slot}: {raw:?}"));
            }
        }
    }
    let name = obj
        .get("name")
        .and_then(|n| n.as_str())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or("Custom")
        .to_string();
    Ok(ThemeDefinition {
        id: String::new(),
        name,
        colors,
    })
}

/// A palette as an exportable theme definition.
pub fn palette_to_theme(p: &Palette) -> ThemeDefinition {
    let rgba = |c: [u8; 4]| {
        format!(
            "rgba({}, {}, {}, {:.3})",
            c[0],
            c[1],
            c[2],
            f32::from(c[3]) / 255.0
        )
    };
    ThemeDefinition {
        id: String::new(),
        name: "Exported".to_string(),
        colors: ThemeColors {
            a0: Some(format!("{}, {}, {}", p.a0[0], p.a0[1], p.a0[2])),
            bg: Some(hex(p.bg)),
            surface: Some(hex(p.surface)),
            surface3: Some(hex(p.surface3)),
            border: Some(rgba(p.border)),
            text: Some(hex(p.text)),
            text2: Some(rgba(p.text2)),
            text3: Some(rgba(p.text3)),
            danger: Some(hex(p.danger)),
            safe: Some(hex(p.safe)),
        },
    }
}

fn preview_palette(cfg: &Config, id: &str) -> Palette {
    let c = Config {
        theme_id: id.to_string(),
        theme: ThemePref::Dark,
        custom_theme: cfg.custom_theme.clone(),
        ..Config::default()
    };
    resolve_palette(&c, PREVIEW_ACCENT, true)
}

/// Follow-up work after a settings edit. Does nothing when `before` equals
/// the current config.
pub fn apply_config_change(sh: &mut Shared, before: &Config, emit: &mut Vec<Command>) {
    if sh.config == *before {
        return;
    }
    sh.dirty_config = true;
    let now = &sh.config;
    if now.theme != before.theme
        || now.theme_id != before.theme_id
        || now.custom_theme != before.custom_theme
    {
        sh.palette_dirty = true;
    }
    if now.alert_type_overrides != before.alert_type_overrides
        || now.storm_polygons_enabled != before.storm_polygons_enabled
    {
        sh.bump_alerts();
    }
    for (name, was, is) in [
        (
            "spc",
            before.spc_outlook_enabled,
            sh.config.spc_outlook_enabled,
        ),
        ("md", before.radar.md_polygons, sh.config.radar.md_polygons),
    ] {
        if is && !was {
            // Re-using `:layer ... on` kicks off the SPC fetch when there is no data yet.
            emit.push(Command::Layer {
                name: name.to_string(),
                on: Some(true),
            });
        }
    }
    // A new weather source applies at once; a new AccuWeather key once
    // editing pauses, so a key being typed or pasted isn't sent part-way.
    if sh.config.provider != before.provider {
        sh.refresh_due = None;
        emit.push(Command::Refresh);
    } else if sh.config.accu_weather_api_key != before.accu_weather_api_key
        && sh.config.provider == WeatherProviderId::AccuWeather
    {
        sh.refresh_due = Some(std::time::Instant::now() + std::time::Duration::from_millis(1500));
    }
    if sh.config.libre_wxr_host != before.libre_wxr_host {
        emit.push(Command::Set {
            key: "librewxrhost".to_string(),
            value: sh.config.libre_wxr_host.clone(),
        });
    }
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

/// Width reserved for a combo box / short control in a row.
const CTRL_W: f32 = 210.0;
/// Width reserved for a text field or key in a row.
const FIELD_W: f32 = 300.0;
/// Width reserved for a switch.
const SWITCH_W: f32 = 48.0;

fn toggle_row(
    ui: &mut Ui,
    p: &Palette,
    rows: &mut Rows,
    label: &str,
    help: Option<&str>,
    v: &mut bool,
) -> bool {
    rows.next(ui, p);
    kit::row(ui, p, label, help, SWITCH_W, |ui| {
        kit::toggle(ui, p, v).changed()
    })
}

#[allow(clippy::too_many_arguments)]
fn combo_row<T: Copy + PartialEq>(
    ui: &mut Ui,
    p: &Palette,
    rows: &mut Rows,
    label: &str,
    help: Option<&str>,
    id: &str,
    cur: &mut T,
    opts: &[(T, &str)],
) {
    rows.next(ui, p);
    kit::row(ui, p, label, help, CTRL_W, |ui| {
        kit::combo(ui, p, id, cur, opts, CTRL_W - 4.0);
    });
}

fn note(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(4.0);
    kit::callout(ui, p, c32a(p.text3), text);
    ui.add_space(14.0);
}

fn warn(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(4.0);
    kit::callout(ui, p, c32(p.a0), text);
    ui.add_space(14.0);
}

pub fn settings_ui(
    ui: &mut Ui,
    sh: &mut Shared,
    p: &Palette,
    st: &mut SettingsState,
    emit: &mut Vec<Command>,
) {
    let before = sh.config.clone();
    let area = ui.max_rect();
    let side_w = 196.0;
    let side = egui::Rect::from_min_max(area.min, egui::pos2(area.left() + side_w, area.bottom()));
    let content = egui::Rect::from_min_max(egui::pos2(side.right() + 1.0, area.top()), area.max);

    // Sidebar.
    ui.painter().rect_filled(side, 0.0, kit::raised(p));
    ui.painter().vline(
        side.right(),
        side.y_range(),
        egui::Stroke::new(1.0, c32a(p.border)),
    );
    let mut nav_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(side.shrink2(egui::vec2(10.0, 14.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    nav_ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(
            RichText::new("Settings")
                .font(saira(19.0, "semibold"))
                .color(c32(p.text)),
        );
    });
    nav_ui.add_space(12.0);
    let labels: Vec<&str> = Tab::ALL.iter().map(|t| t.label()).collect();
    let cur = Tab::ALL.iter().position(|t| *t == st.tab).unwrap_or(0);
    if let Some(i) = kit::nav(&mut nav_ui, p, &labels, cur) {
        st.tab = Tab::ALL[i];
    }
    nav_ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.label(
                RichText::new("Changes save automatically")
                    .font(saira(11.5, "light"))
                    .color(kit::muted(p)),
            );
        });
    });

    // Page.
    let mut page = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt(("settings_scroll", st.tab as u8))
        .auto_shrink([false, false])
        .show(&mut page, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 26,
                    right: 26,
                    top: 20,
                    bottom: 12,
                })
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(720.0));
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                    ui.spacing_mut().slider_width = CTRL_W - 70.0;
                    let (title, sub) = st.tab.header();
                    kit::page_header(ui, p, title, sub);
                    match st.tab {
                        Tab::General => general_tab(ui, &mut sh.config, p, st),
                        Tab::Sources => sources_tab(ui, &mut sh.config, p, st),
                        Tab::Radar => radar_tab(ui, sh, p, st),
                        Tab::Alerts => alerts_tab(ui, &mut sh.config, p, st),
                        Tab::Radio => radio_tab(ui, &mut sh.config, p),
                        Tab::Ai => ai_tab(ui, &mut sh.config.ai, p, st),
                        Tab::Themes => themes_tab(ui, sh, p, st),
                        Tab::Developer => developer_tab(ui, sh, p, st, emit),
                        Tab::About => about_tab(ui, sh, p),
                    }
                });
        });
    ui.allocate_rect(area, egui::Sense::hover());
    apply_config_change(sh, &before, emit);
}

fn general_tab(ui: &mut Ui, cfg: &mut Config, p: &Palette, st: &mut SettingsState) {
    kit::card(ui, p, "Forecast", |ui| {
        let mut rows = Rows::new();
        combo_row(
            ui,
            p,
            &mut rows,
            "Weather source",
            Some("Where forecasts and current conditions come from"),
            "provider",
            &mut cfg.provider,
            &[
                (WeatherProviderId::OpenMeteo, "Open-Meteo"),
                (WeatherProviderId::AccuWeather, "AccuWeather"),
            ],
        );
        if cfg.provider == WeatherProviderId::AccuWeather {
            rows.next(ui, p);
            kit::row(
                ui,
                p,
                "AccuWeather API key",
                Some("From developer.accuweather.com"),
                FIELD_W,
                |ui| {
                    kit::secret_field(
                        ui,
                        p,
                        &mut cfg.accu_weather_api_key,
                        &mut st.show_accu,
                        "required",
                        FIELD_W,
                    )
                },
            );
        }
        combo_row(
            ui,
            p,
            &mut rows,
            "Units",
            None,
            "units",
            &mut cfg.units,
            &[
                (UnitPref::F, "Fahrenheit (°F, mph)"),
                (UnitPref::C, "Celsius (°C, km/h)"),
            ],
        );
        combo_row(
            ui,
            p,
            &mut rows,
            "Time format",
            None,
            "time_format",
            &mut cfg.time_format,
            &[(TimeFormat::H12, "12-hour"), (TimeFormat::H24, "24-hour")],
        );
        let opts: Vec<(u32, String)> = REFRESH_CHOICES
            .iter()
            .map(|&m| {
                let l = if m == 0 {
                    "Off".to_string()
                } else {
                    format!("Every {m} min")
                };
                (m, l)
            })
            .collect();
        let opts: Vec<(u32, &str)> = opts.iter().map(|(m, l)| (*m, l.as_str())).collect();
        combo_row(
            ui,
            p,
            &mut rows,
            "Auto refresh",
            Some("How often forecasts and alerts reload"),
            "auto_refresh",
            &mut cfg.auto_refresh_minutes,
            &opts,
        );
    });
    if cfg.provider == WeatherProviderId::AccuWeather {
        if cfg.accu_weather_api_key.is_none() {
            warn(
                ui,
                p,
                "AccuWeather needs an API key. Until one is set, forecasts come from Open-Meteo.",
            );
        } else {
            note(
                ui,
                p,
                "Free AccuWeather keys allow 50 requests a day. Each place uses 3 per refresh, so forecasts are reused for an hour.",
            );
        }
    }
    let applies = theme_pref_applies(cfg);
    kit::card(ui, p, "Appearance", |ui| {
        let mut rows = Rows::new();
        ui.add_enabled_ui(applies, |ui| {
            combo_row(
                ui,
                p,
                &mut rows,
                "Light or dark",
                Some(if applies {
                    "Follow the system, or pick one"
                } else {
                    "Set by the theme chosen under Themes"
                }),
                "theme_pref",
                &mut cfg.theme,
                &[
                    (ThemePref::System, "System"),
                    (ThemePref::Light, "Light"),
                    (ThemePref::Dark, "Dark"),
                ],
            );
        });
        toggle_row(
            ui,
            p,
            &mut rows,
            "Top glow bar",
            Some("A coloured glow along the top edge for active alerts"),
            &mut cfg.top_glow_enabled,
        );
    });
    kit::card(ui, p, "Behaviour", |ui| {
        let mut rows = Rows::new();
        toggle_row(
            ui,
            p,
            &mut rows,
            "Desktop notifications",
            Some("Notify when a new alert is issued for a saved place"),
            &mut cfg.notifications_enabled,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "Emphasised storm polygons",
            Some("Draw warning polygons on the radar with a heavier outline"),
            &mut cfg.storm_polygons_enabled,
        );
    });
}

fn sources_tab(ui: &mut Ui, cfg: &mut Config, p: &Palette, st: &mut SettingsState) {
    kit::card(ui, p, "Radar server", |ui| {
        let mut rows = Rows::new();
        rows.next(ui, p);
        kit::row(
            ui,
            p,
            "LibreWXR host",
            Some("An http(s) URL; applied when you leave the field"),
            FIELD_W,
            |ui| {
                let mut buf = st
                    .host_buf
                    .clone()
                    .unwrap_or_else(|| cfg.libre_wxr_host.clone());
                let resp = kit::text_field(ui, p, &mut buf, "https://…", FIELD_W);
                if resp.changed() {
                    st.host_buf = Some(buf);
                }
                if resp.lost_focus() {
                    if let Some(b) = st.host_buf.take() {
                        let host = b.trim().trim_end_matches('/').to_string();
                        if host.starts_with("http://") || host.starts_with("https://") {
                            cfg.libre_wxr_host = host;
                        } else {
                            st.notice = Some((
                                true,
                                "The radar host must start with http:// or https://".into(),
                            ));
                        }
                    }
                }
            },
        );
    });
    if let Some((true, text)) = &st.notice {
        if text.starts_with("The radar host") {
            warn(ui, p, &text.clone());
        }
    }
    kit::card(ui, p, "API keys", |ui| {
        let mut rows = Rows::new();
        rows.next(ui, p);
        kit::row(
            ui,
            p,
            "AccuWeather",
            Some("Forecasts, when chosen as the weather source"),
            FIELD_W,
            |ui| {
                kit::secret_field(
                    ui,
                    p,
                    &mut cfg.accu_weather_api_key,
                    &mut st.show_accu,
                    "not set",
                    FIELD_W,
                )
            },
        );
        rows.next(ui, p);
        kit::row(
            ui,
            p,
            "WillyWeather",
            Some("Bureau of Meteorology (Australia) warnings"),
            FIELD_W,
            |ui| {
                kit::secret_field(
                    ui,
                    p,
                    &mut cfg.willy_weather_api_key,
                    &mut st.show_willy,
                    "not set",
                    FIELD_W,
                )
            },
        );
        rows.next(ui, p);
        kit::row(
            ui,
            p,
            "OpenWeatherMap",
            Some("Wind, temperature and precipitation overlays"),
            FIELD_W,
            |ui| {
                kit::secret_field(
                    ui,
                    p,
                    &mut cfg.open_weather_map_api_key,
                    &mut st.show_owm,
                    "not set",
                    FIELD_W,
                )
            },
        );
    });
    note(
        ui,
        p,
        "Keys are stored in your Silver Skies config file and only sent to their own service. \
         Storm cells, storm reports, tropical cyclones, soundings and terrain use free services that need no key.",
    );
}

fn radar_tab(ui: &mut Ui, sh: &mut Shared, p: &Palette, st: &mut SettingsState) {
    let schemes: Vec<(u8, String)> = match sh.weather_maps.as_ref() {
        Some(m) if !m.color_schemes.is_empty() => m
            .color_schemes
            .iter()
            .map(|s| (s.id, s.name.clone()))
            .collect(),
        _ => ss_net::librewxr::default_color_schemes()
            .into_iter()
            .map(|s| (s.id, s.name))
            .collect(),
    };
    let key_set = sh
        .config
        .open_weather_map_api_key
        .as_deref()
        .is_some_and(|k| !k.trim().is_empty());
    let cfg = &mut sh.config;

    kit::card(ui, p, "Composite radar", |ui| {
        let mut rows = Rows::new();
        let mut opts: Vec<(u8, &str)> = schemes.iter().map(|(i, n)| (*i, n.as_str())).collect();
        let cur_name = format!("Scheme {}", cfg.radar.color_scheme);
        if !opts.iter().any(|(i, _)| *i == cfg.radar.color_scheme) {
            opts.push((cfg.radar.color_scheme, cur_name.as_str()));
        }
        combo_row(
            ui,
            p,
            &mut rows,
            "Colour scheme",
            None,
            "radar_scheme",
            &mut cfg.radar.color_scheme,
            &opts,
        );
        rows.next(ui, p);
        kit::row(ui, p, "Opacity", None, CTRL_W, |ui| {
            kit::slider(
                ui,
                p,
                egui::Slider::new(&mut cfg.radar.opacity, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                    .custom_parser(|s| {
                        s.trim()
                            .trim_end_matches('%')
                            .parse::<f64>()
                            .ok()
                            .map(|v| v / 100.0)
                    }),
            );
        });
        toggle_row(
            ui,
            p,
            &mut rows,
            "Smooth tiles",
            None,
            &mut cfg.radar.smooth,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "Snow",
            Some("Show snow in its own colours"),
            &mut cfg.radar.snow,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "Nowcast frames",
            Some("Extend the loop with short-range forecast frames"),
            &mut cfg.radar.nowcast,
        );
        toggle_row(ui, p, &mut rows, "Legend", None, &mut cfg.radar.legend);
        toggle_row(
            ui,
            p,
            &mut rows,
            "Terrain",
            Some("Shaded relief under the radar"),
            &mut cfg.radar.terrain,
        );
    });

    kit::card(ui, p, "Severe weather", |ui| {
        let mut rows = Rows::new();
        toggle_row(
            ui,
            p,
            &mut rows,
            "Storm cells",
            Some("Tornado, mesocyclone and hail signatures with projected tracks"),
            &mut cfg.radar.storm_cells,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "Storm reports",
            Some("Tornado, hail, wind and flood reports from the last 24 hours (US)"),
            &mut cfg.radar.storm_reports,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "Tropical cyclones",
            Some("Centres, tracks and wind swaths worldwide"),
            &mut cfg.radar.tropical,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "Storm motion arrows",
            None,
            &mut cfg.radar.show_arrows,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "Mosaic cell markers",
            None,
            &mut cfg.radar.show_cells,
        );
    });

    kit::card(ui, p, "Overlays", |ui| {
        let mut rows = Rows::new();
        toggle_row(
            ui,
            p,
            &mut rows,
            "Alert polygons",
            None,
            &mut cfg.radar.show_polygons,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "Alerts in view",
            Some("Load alerts for whatever area the map shows"),
            &mut cfg.radar.viewport_alerts,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "IR satellite",
            None,
            &mut cfg.radar.satellite,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "SPC Day 1 outlook",
            None,
            &mut cfg.spc_outlook_enabled,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "SPC mesoscale discussions",
            None,
            &mut cfg.radar.md_polygons,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "Wind barbs",
            Some("From METAR observations"),
            &mut cfg.radar.wind_barbs,
        );
        toggle_row(
            ui,
            p,
            &mut rows,
            "Radar sites",
            None,
            &mut cfg.radar.show_stations,
        );
    });

    kit::card(ui, p, "NEXRAD single site", |ui| {
        let mut rows = Rows::new();
        rows.next(ui, p);
        kit::row(
            ui,
            p,
            "Site",
            Some("A WSR-88D id such as KFSX; blank uses the composite"),
            CTRL_W,
            |ui| {
                let mut buf = st
                    .site_buf
                    .clone()
                    .unwrap_or_else(|| cfg.radar.nexrad_site.clone().unwrap_or_default());
                let resp = kit::text_field(ui, p, &mut buf, "composite", CTRL_W - 4.0);
                if resp.changed() {
                    st.site_buf = Some(buf.clone());
                }
                if resp.lost_focus() {
                    st.site_buf = None;
                    cfg.radar.nexrad_site = opt_string(&buf).map(|s| s.to_ascii_uppercase());
                }
            },
        );
        let opts: Vec<(RadarProduct, &str)> =
            RadarProduct::ALL.iter().map(|p| (*p, p.label())).collect();
        combo_row(
            ui,
            p,
            &mut rows,
            "Product",
            None,
            "radar_product",
            &mut cfg.radar.nexrad_product,
            &opts,
        );
    });

    kit::card(ui, p, "OpenWeatherMap overlays", |ui| {
        ui.add_enabled_ui(key_set, |ui| {
            let mut rows = Rows::new();
            toggle_row(ui, p, &mut rows, "Wind", None, &mut cfg.radar.owm_wind);
            toggle_row(
                ui,
                p,
                &mut rows,
                "Temperature",
                None,
                &mut cfg.radar.owm_temp,
            );
            toggle_row(
                ui,
                p,
                &mut rows,
                "Precipitation",
                None,
                &mut cfg.radar.owm_precip,
            );
            toggle_row(
                ui,
                p,
                &mut rows,
                "Air quality badge",
                None,
                &mut cfg.radar.aqi_badge,
            );
        });
    });
    if !key_set {
        warn(
            ui,
            p,
            "Add an OpenWeatherMap API key under Data sources to use these overlays.",
        );
    }
}

fn alerts_tab(ui: &mut Ui, cfg: &mut Config, p: &Palette, st: &mut SettingsState) {
    ui.horizontal(|ui| {
        ui.set_min_height(kit::CONTROL_H);
        let w = (ui.available_width() - 120.0).clamp(160.0, 360.0);
        kit::text_field(ui, p, &mut st.alert_filter, "Filter alert types…", w);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let any = !cfg.alert_type_overrides.is_empty();
            if ui
                .add_enabled_ui(any, |ui| kit::button(ui, p, "Reset all"))
                .inner
                .clicked()
            {
                cfg.alert_type_overrides.clear();
            }
        });
    });
    ui.add_space(12.0);
    let f = st.alert_filter.trim().to_lowercase();
    let shown: Vec<_> = ALL_CLASSES
        .iter()
        .filter(|(class, name, _)| {
            f.is_empty() || class.contains(&f) || name.to_lowercase().contains(&f)
        })
        .collect();
    kit::card(ui, p, &format!("Alert types ({})", shown.len()), |ui| {
        let mut rows = Rows::new();
        for (class, name, _) in shown {
            let mut rgb = effective_color(class, &cfg.alert_type_overrides);
            let mut vis = effective_visible(class, &cfg.alert_type_overrides);
            rows.next(ui, p);
            kit::row(ui, p, name, Some(class), 170.0, |ui| {
                let has = cfg.alert_type_overrides.contains_key(*class);
                if ui
                    .add_enabled_ui(has, |ui| kit::button(ui, p, "Reset"))
                    .inner
                    .on_hover_text("Back to the default colour and visibility")
                    .clicked()
                {
                    reset_override(&mut cfg.alert_type_overrides, class);
                }
                if kit::toggle(ui, p, &mut vis)
                    .on_hover_text("Show this alert type")
                    .changed()
                {
                    set_override_visible(&mut cfg.alert_type_overrides, class, vis);
                }
                ui.spacing_mut().interact_size = egui::vec2(28.0, 22.0);
                if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                    set_override_color(&mut cfg.alert_type_overrides, class, rgb);
                }
            });
        }
    });
    note(
        ui,
        p,
        "Hidden alert types are dropped everywhere: lists, the radar and notifications.",
    );
}

fn radio_tab(ui: &mut Ui, cfg: &mut Config, p: &Palette) {
    kit::card(ui, p, "Weather radio", |ui| {
        let mut rows = Rows::new();
        toggle_row(
            ui,
            p,
            &mut rows,
            "Weather radio",
            Some("Play a NOAA Weather Radio-style broadcast"),
            &mut cfg.weather_radio_enabled,
        );
        let on_ = cfg.weather_radio_enabled;
        ui.add_enabled_ui(on_, |ui| {
            combo_row(
                ui,
                p,
                &mut rows,
                "Mode",
                None,
                "radio_mode",
                &mut cfg.weather_radio_mode,
                &[
                    (RadioMode::Simulated, "Simulated"),
                    (RadioMode::Live, "Live stream"),
                ],
            );
            combo_row(
                ui,
                p,
                &mut rows,
                "Live feed",
                Some("Pick the nearest transmitter, or give a stream URL"),
                "radio_feed",
                &mut cfg.weather_radio_live_feed_mode,
                &[
                    (LiveFeedMode::Auto, "Nearest (auto)"),
                    (LiveFeedMode::Manual, "Stream URL"),
                ],
            );
            if cfg.weather_radio_live_feed_mode == LiveFeedMode::Manual {
                rows.next(ui, p);
                kit::row(ui, p, "Stream URL", None, FIELD_W, |ui| {
                    let mut url = cfg
                        .weather_radio_live_stream_url
                        .clone()
                        .unwrap_or_default();
                    kit::text_field(ui, p, &mut url, "https://…", FIELD_W);
                    let new = opt_string(&url);
                    if new != cfg.weather_radio_live_stream_url {
                        cfg.weather_radio_live_stream_url = new;
                    }
                });
            }
        });
    });

    kit::card(ui, p, "Spoken alerts", |ui| {
        let mut rows = Rows::new();
        toggle_row(
            ui,
            p,
            &mut rows,
            "Read new alerts aloud",
            None,
            &mut cfg.tts_alerts_enabled,
        );
        let on_ = cfg.tts_alerts_enabled;
        ui.add_enabled_ui(on_, |ui| {
            combo_row(
                ui,
                p,
                &mut rows,
                "Which alerts",
                None,
                "tts_filter",
                &mut cfg.tts_severity_filter,
                &[
                    (TtsSeverityFilter::ExtremeOnly, "Extreme only"),
                    (TtsSeverityFilter::SevereAndUp, "Severe and up"),
                    (TtsSeverityFilter::ModerateAndUp, "Moderate and up"),
                    (TtsSeverityFilter::All, "All"),
                ],
            );
            rows.next(ui, p);
            kit::row(ui, p, "Speaking rate", None, CTRL_W, |ui| {
                let mut r = cfg.tts_rate;
                if kit::slider(
                    ui,
                    p,
                    egui::Slider::new(&mut r, 0.5..=2.0)
                        .step_by(0.05)
                        .custom_formatter(|v, _| format!("{v:.2}×")),
                )
                .changed()
                {
                    cfg.tts_rate = r.clamp(0.5, 2.0);
                }
            });
            toggle_row(
                ui,
                p,
                &mut rows,
                "Attention tone",
                Some("Play a tone before each alert"),
                &mut cfg.tts_attention_tone,
            );
            toggle_row(
                ui,
                p,
                &mut rows,
                "All saved places",
                Some("Announce alerts for every saved place, not just the active one"),
                &mut cfg.tts_all_saved_locations,
            );
        });
    });
}

fn ai_tab(ui: &mut Ui, ai: &mut AiSettings, p: &Palette, st: &mut SettingsState) {
    kit::card(ui, p, "Assistant", |ui| {
        let mut rows = Rows::new();
        toggle_row(
            ui,
            p,
            &mut rows,
            "Turn on AI",
            Some("AI summaries, answers and the assistant chat (Ctrl+J)"),
            &mut ai.enabled,
        );
        let on_ = ai.enabled;
        ui.add_enabled_ui(on_, |ui| {
            let before = ai.provider;
            let opts: Vec<(AiProvider, &str)> =
                AiProvider::ALL.iter().map(|p| (*p, p.label())).collect();
            combo_row(
                ui,
                p,
                &mut rows,
                "Provider",
                None,
                "ai_provider",
                &mut ai.provider,
                &opts,
            );
            if ai.provider != before {
                // Defaults follow the provider unless overridden.
                ai.base_url.clear();
                ai.model.clear();
            }
            rows.next(ui, p);
            let base_hint = match ai.provider {
                AiProvider::Custom => "https://…/v1".to_string(),
                pr => pr.default_base_url().to_string(),
            };
            kit::row(
                ui,
                p,
                "Base URL",
                Some("Leave blank for the provider's standard address"),
                FIELD_W,
                |ui| kit::text_field(ui, p, &mut ai.base_url, &base_hint, FIELD_W),
            );
            rows.next(ui, p);
            let needs = ai.provider.needs_key();
            kit::row(
                ui,
                p,
                "API key",
                Some(if needs {
                    "Required by this provider"
                } else {
                    "Optional for this provider"
                }),
                FIELD_W,
                |ui| {
                    kit::secret_field(
                        ui,
                        p,
                        &mut ai.api_key,
                        &mut st.show_ai_key,
                        if needs { "required" } else { "optional" },
                        FIELD_W,
                    )
                },
            );
            rows.next(ui, p);
            kit::row(ui, p, "Model", None, FIELD_W, |ui| {
                kit::text_field(ui, p, &mut ai.model, ai.provider.model_hint(), FIELD_W)
            });
        });
    });
    if ai.enabled {
        match ai.problem() {
            Some(problem) => warn(ui, p, problem),
            None => {
                let model = ai.endpoint().map(|e| e.model).unwrap_or_default();
                kit::status(
                    ui,
                    c32(p.safe),
                    kit::muted(p),
                    &format!("Ready: {} · {model}", ai.provider.label()),
                );
                ui.add_space(12.0);
            }
        }
    }
    if let Some(n) = ai.provider.note() {
        note(ui, p, n);
    }
    note(
        ui,
        p,
        "Uses your own account with an AI provider: Claude, OpenAI, OpenRouter, OpenCode Go or the \
         OpenCode Console, a local model in Ollama, or any OpenAI-compatible service (LM Studio, a \
         company gateway). The forecasts for your saved places are sent with each request; nothing else is.",
    );
    note(ui, p, AI_DISCLAIMER);
}

fn swatches(ui: &mut Ui, p: &Palette) {
    for c in [p.danger, p.text, p.a0, p.surface, p.bg] {
        let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 3.0, c32(c));
        ui.painter().rect_stroke(
            r,
            3.0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(90)),
            egui::StrokeKind::Inside,
        );
    }
}

fn themes_tab(ui: &mut Ui, sh: &mut Shared, p: &Palette, st: &mut SettingsState) {
    let mut ids: Vec<(String, String)> =
        vec![("default".into(), "Default (condition accent)".into())];
    ids.extend(builtin_themes().into_iter().map(|t| (t.id, t.name)));
    if let Some(c) = &sh.config.custom_theme {
        ids.push(("custom".into(), format!("Custom: {}", c.name)));
    }
    kit::card(ui, p, "Theme", |ui| {
        let mut rows = Rows::new();
        for (id, name) in &ids {
            let sel = sh.config.theme_id == *id;
            let pv = preview_palette(&sh.config, id);
            rows.next(ui, p);
            kit::row(ui, p, name, None, 210.0, |ui| {
                if sel {
                    ui.add_sized(
                        [76.0, kit::CONTROL_H],
                        egui::Label::new(
                            RichText::new("✓ In use")
                                .font(saira(13.0, "medium"))
                                .color(c32(p.a0)),
                        ),
                    );
                } else if kit::button(ui, p, "Use").clicked() {
                    sh.config.theme_id = id.clone();
                }
                ui.add_space(8.0);
                swatches(ui, &pv);
            });
        }
    });

    let mut parsed: Option<Result<ThemeDefinition, String>> = None;
    kit::card(ui, p, "Custom theme", |ui| {
        ui.add_space(10.0);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "Paste theme JSON, e.g. {\"name\": \"Neon\", \"colors\": {\"a0\": \"255, 45, 155\", \"bg\": \"#180510\"}}",
                )
                .font(saira(12.5, "light"))
                .color(kit::muted(p)),
            )
            .wrap(),
        );
        ui.add_space(6.0);
        ui.scope(|ui| {
            kit::field_style(ui, p);
            ui.add(
                egui::TextEdit::multiline(&mut st.theme_json)
                    .font(crate::theme::code(13.0))
                    .hint_text("{ \"name\": …, \"colors\": { … } }")
                    .margin(egui::Margin::same(8))
                    .desired_rows(6)
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if kit::primary_button(ui, p, "Apply JSON").clicked() {
                parsed = Some(parse_theme_json(&st.theme_json));
            }
            if kit::button(ui, p, "Copy current theme")
                .on_hover_text("Copy the theme in use as JSON")
                .clicked()
            {
                let json = serde_json::to_string_pretty(&palette_to_theme(&sh.palette))
                    .unwrap_or_default();
                ui.ctx().copy_text(json);
                st.notice = Some((false, "Current theme copied to the clipboard".to_string()));
            }
        });
        ui.add_space(10.0);
        divider(ui, p);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 100.0).max(160.0);
            kit::text_field(ui, p, &mut st.theme_path, "/path/to/theme.json", w);
            if kit::button(ui, p, "Load file").clicked() {
                match std::fs::read_to_string(st.theme_path.trim()) {
                    Ok(text) => {
                        st.theme_json = text;
                        parsed = Some(parse_theme_json(&st.theme_json));
                    }
                    Err(e) => st.notice = Some((true, format!("Cannot read the file: {e}"))),
                }
            }
        });
        ui.add_space(12.0);
    });
    match parsed {
        Some(Ok(def)) => {
            st.notice = Some((false, format!("Applied custom theme “{}”", def.name)));
            sh.config.custom_theme = Some(def);
            sh.config.theme_id = "custom".to_string();
        }
        Some(Err(e)) => st.notice = Some((true, e)),
        None => {}
    }
    if let Some((is_err, text)) = &st.notice {
        let c = if *is_err { c32(p.danger) } else { c32(p.safe) };
        kit::status(ui, c, kit::muted(p), text);
    }
}

fn developer_tab(
    ui: &mut Ui,
    sh: &mut Shared,
    p: &Palette,
    st: &mut SettingsState,
    emit: &mut Vec<Command>,
) {
    kit::card(ui, p, "", |ui| {
        let mut rows = Rows::new();
        toggle_row(
            ui,
            p,
            &mut rows,
            "Developer tools",
            Some("Demo alerts and other testing aids"),
            &mut sh.config.dev_tools_enabled,
        );
    });
    if !sh.config.dev_tools_enabled {
        return;
    }
    st.demo_group = st.demo_group.min(DEMO_GROUPS.len().saturating_sub(1));
    st.demo_item = st
        .demo_item
        .min(DEMO_GROUPS[st.demo_group].items.len().saturating_sub(1));
    kit::card(ui, p, "Demo alerts", |ui| {
        let mut rows = Rows::new();
        let groups: Vec<(usize, &str)> = DEMO_GROUPS
            .iter()
            .enumerate()
            .map(|(i, g)| (i, g.name))
            .collect();
        let before = st.demo_group;
        combo_row(
            ui,
            p,
            &mut rows,
            "Region",
            None,
            "demo_group",
            &mut st.demo_group,
            &groups,
        );
        if st.demo_group != before {
            st.demo_item = 0;
        }
        let group = &DEMO_GROUPS[st.demo_group];
        let items: Vec<(usize, &str)> = group
            .items
            .iter()
            .enumerate()
            .map(|(i, (e, _))| (i, *e))
            .collect();
        combo_row(
            ui,
            p,
            &mut rows,
            "Alert",
            None,
            "demo_item",
            &mut st.demo_item,
            &items,
        );
        let item = group.items.get(st.demo_item).map(|(e, _)| *e).unwrap_or("");
        rows.next(ui, p);
        let count = sh.demo_alerts.len();
        kit::row(
            ui,
            p,
            "Active demo alerts",
            Some(&count.to_string()),
            360.0,
            |ui| {
                if kit::button(ui, p, "Clear").clicked() {
                    sh.demo_alerts.clear();
                    sh.asteroid = None;
                    sh.bump_alerts();
                }
                let idle = sh.asteroid.is_none();
                if ui
                    .add_enabled_ui(idle, |ui| kit::button(ui, p, "Asteroid warning"))
                    .inner
                    .clicked()
                {
                    emit.push(Command::Asteroid);
                }
                if kit::primary_button(ui, p, "Add alert").clicked() && !item.is_empty() {
                    emit.push(Command::Demo {
                        group: Some(item.to_string()),
                    });
                }
            },
        );
    });
}

fn about_tab(ui: &mut Ui, sh: &Shared, p: &Palette) {
    kit::card(ui, p, "", |ui| {
        let mut rows = Rows::new();
        rows.next(ui, p);
        kit::row(ui, p, "Version", None, 300.0, |ui| {
            ui.label(
                RichText::new(format!(
                    "Silver Skies Pro {}",
                    ss_core::version::version_label()
                ))
                .color(c32(p.a0)),
            );
        });
        let dir = sh
            .data_dir
            .as_ref()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "(unknown)".to_string());
        rows.next(ui, p);
        kit::row(ui, p, "Data folder", Some(&dir), 120.0, |ui| {
            if kit::button(ui, p, "Copy path").clicked() {
                ui.ctx().copy_text(dir.clone());
            }
        });
        rows.next(ui, p);
        kit::row(ui, p, "Source code", Some(REPO_URL), 120.0, |ui| {
            if kit::button(ui, p, "Open").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(REPO_URL));
            }
        });
    });
    kit::card(ui, p, "Key commands", |ui| {
        let mut rows = Rows::new();
        for (cmd, what) in [
            (":help", "List all commands"),
            (":goto <place>", "Search for and select a place"),
            (":refresh", "Reload forecasts and alerts"),
            (":theme <id>", "Switch theme"),
            (":set <key> <value>", "Change a setting"),
            (":settings", "Open this window (Ctrl+,)"),
            (":assistant", "Open the AI assistant (Ctrl+J)"),
        ] {
            rows.next(ui, p);
            kit::row(ui, p, what, None, 220.0, |ui| {
                ui.label(
                    RichText::new(cmd)
                        .font(crate::theme::code(13.0))
                        .color(c32(p.a0)),
                );
            });
        }
    });
    egui::CollapsingHeader::new(
        RichText::new("Raw config (secrets masked)")
            .font(saira(13.0, "medium"))
            .color(kit::muted(p)),
    )
    .id_salt("raw_config")
    .show(ui, |ui| {
        egui::Frame::new()
            .fill(kit::raised(p))
            .corner_radius(kit::R)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(config_text(&sh.config))
                        .font(crate::theme::code(12.0))
                        .color(kit::muted(p)),
                );
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss_core::{LocationsFile, SessionFile};

    fn shared() -> Shared {
        Shared::new(
            Config::default(),
            LocationsFile::default(),
            SessionFile::default(),
        )
    }

    const CLASS: &str = "alert-tornado-warning";

    #[test]
    fn override_color_is_minimal() {
        let mut m = BTreeMap::new();
        set_override_color(&mut m, CLASS, [1, 2, 3]);
        assert_eq!(m[CLASS].color.as_deref(), Some("#010203"));
        assert_eq!(m[CLASS].visible, None);
        // Back to the default colour removes the entry entirely.
        set_override_color(&mut m, CLASS, class_color(CLASS));
        assert!(m.is_empty());
    }

    #[test]
    fn override_visible_is_minimal() {
        let mut m = BTreeMap::new();
        set_override_visible(&mut m, CLASS, false);
        assert_eq!(m[CLASS].visible, Some(false));
        assert_eq!(m[CLASS].color, None);
        set_override_color(&mut m, CLASS, [9, 9, 9]);
        set_override_visible(&mut m, CLASS, true);
        assert_eq!(m[CLASS].visible, None, "visible is the default");
        assert_eq!(m[CLASS].color.as_deref(), Some("#090909"));
        set_override_color(&mut m, CLASS, class_color(CLASS));
        assert!(m.is_empty());
        set_override_visible(&mut m, CLASS, true);
        assert!(m.is_empty());
    }

    #[test]
    fn reset_removes_only_that_class() {
        let mut m = BTreeMap::new();
        set_override_visible(&mut m, CLASS, false);
        set_override_visible(&mut m, "eccc-red", false);
        reset_override(&mut m, CLASS);
        assert_eq!(m.len(), 1);
        assert!(m.contains_key("eccc-red"));
    }

    #[test]
    fn empty_keys_become_none() {
        assert_eq!(opt_string("  "), None);
        assert_eq!(opt_string(" abc "), Some("abc".into()));
    }

    #[test]
    fn theme_pref_ignored_for_named_themes() {
        let mut c = Config::default();
        assert!(theme_pref_applies(&c));
        c.theme_id = builtin_themes()[0].id.clone();
        assert!(!theme_pref_applies(&c));
        c.theme_id = "custom".into();
        assert!(theme_pref_applies(&c), "custom without a theme falls back");
        c.custom_theme = Some(parse_theme_json(r##"{"bg":"#000"}"##).unwrap());
        assert!(!theme_pref_applies(&c));
    }

    #[test]
    fn theme_json_full_and_partial() {
        let t = parse_theme_json(
            r##"{"name":"Mine","colors":{"a0":"255, 45, 155","bg":"#180510","extra":"x"},"other":1}"##,
        )
        .unwrap();
        assert_eq!(t.name, "Mine");
        assert_eq!(t.colors.bg.as_deref(), Some("#180510"));
        assert_eq!(t.colors.text, None);
    }

    #[test]
    fn theme_json_without_name_or_wrapper() {
        let t = parse_theme_json(r##"{"colors":{"bg":"#000000"}}"##).unwrap();
        assert_eq!(t.name, "Custom");
        let t = parse_theme_json(r##"{"bg":"#000000","text":"rgb(1,2,3)"}"##).unwrap();
        assert_eq!(t.name, "Custom");
        assert_eq!(t.colors.text.as_deref(), Some("rgb(1,2,3)"));
    }

    #[test]
    fn theme_json_rejects_bad_input() {
        let e = parse_theme_json(r##"{"colors":{"bg":"#000","text":"nope"}}"##).unwrap_err();
        assert!(e.contains("text"), "{e}");
        assert!(parse_theme_json(r##"{"name":"x","colors":{}}"##).is_err());
        assert!(parse_theme_json(r##"{"name":"x"}"##).is_err());
        assert!(parse_theme_json("not json").is_err());
        assert!(parse_theme_json("[1]").is_err());
    }

    #[test]
    fn exported_theme_parses_back() {
        let p = ss_core::theme::dark_base();
        let json = serde_json::to_string_pretty(&palette_to_theme(&p)).unwrap();
        let def = parse_theme_json(&json).unwrap();
        assert_eq!(def.colors.bg.as_deref(), Some(hex(p.bg).as_str()));
    }

    #[test]
    fn unchanged_config_does_nothing() {
        let mut sh = shared();
        let before = sh.config.clone();
        let mut emit = Vec::new();
        let rev = sh.alerts_rev;
        apply_config_change(&mut sh, &before, &mut emit);
        assert!(!sh.dirty_config && !sh.palette_dirty);
        assert!(emit.is_empty());
        assert_eq!(sh.alerts_rev, rev);
    }

    #[test]
    fn plain_edit_only_marks_dirty() {
        let mut sh = shared();
        let before = sh.config.clone();
        sh.config.units = UnitPref::C;
        let mut emit = Vec::new();
        let rev = sh.alerts_rev;
        apply_config_change(&mut sh, &before, &mut emit);
        assert!(sh.dirty_config);
        assert!(!sh.palette_dirty);
        assert!(emit.is_empty());
        assert_eq!(sh.alerts_rev, rev);
    }

    #[test]
    fn theme_edit_marks_palette_dirty() {
        let mut sh = shared();
        let before = sh.config.clone();
        sh.config.theme_id = "custom".into();
        apply_config_change(&mut sh, &before, &mut Vec::new());
        assert!(sh.dirty_config && sh.palette_dirty);
    }

    #[test]
    fn override_and_host_edits_have_side_effects() {
        let mut sh = shared();
        let before = sh.config.clone();
        set_override_visible(&mut sh.config.alert_type_overrides, CLASS, false);
        sh.config.libre_wxr_host = "https://x.test".into();
        let mut emit = Vec::new();
        let rev = sh.alerts_rev;
        apply_config_change(&mut sh, &before, &mut emit);
        assert!(sh.dirty_config);
        assert_ne!(sh.alerts_rev, rev);
        assert_eq!(
            emit,
            vec![Command::Set {
                key: "librewxrhost".into(),
                value: "https://x.test".into()
            }]
        );
    }

    #[test]
    fn every_tab_renders_headless() {
        let mut sh = shared();
        sh.config.dev_tools_enabled = true;
        sh.config.provider = WeatherProviderId::AccuWeather;
        sh.config.weather_radio_live_feed_mode = LiveFeedMode::Manual;
        sh.config.custom_theme = Some(parse_theme_json(r##"{"bg":"#000"}"##).unwrap());
        sh.config.theme_id = "custom".into();
        let p = sh.palette;
        let ctx = egui::Context::default();
        for tab in Tab::ALL {
            let mut st = SettingsState {
                tab,
                ..Default::default()
            };
            let mut emit = Vec::new();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                settings_ui(ui, &mut sh, &p, &mut st, &mut emit);
            });
            out.textures_delta.clear();
            assert!(emit.is_empty(), "{tab:?} emitted {emit:?} without input");
        }
        assert!(
            !sh.dirty_config,
            "rendering alone must not dirty the config"
        );
    }
}

#[cfg(test)]
mod headless_probe {
    use super::*;
    #[test]
    fn every_tab_renders() {
        let mut sh = Shared::new(Config::default(), Default::default(), Default::default());
        let p = sh.palette;
        for t in Tab::ALL {
            let mut st = SettingsState {
                tab: t,
                ..Default::default()
            };
            let mut emit = Vec::new();
            let ctx = egui::Context::default();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                settings_ui(ui, &mut sh, &p, &mut st, &mut emit);
            });
            out.textures_delta.clear();
            eprintln!("tab {:?} ok", t);
        }
    }
}
