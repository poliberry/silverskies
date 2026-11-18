//! Terminal chrome around the panes: a numbered function-key row under the
//! command line (also bound to F1–F12) and a scrolling ticker tape of every
//! saved place at the bottom. Every action issues the same command you could
//! type at `:`.

use egui::{Align2, Color32, FontId, Key, Sense, Stroke, Ui};
use ss_core::command::{parse, Command};
use ss_core::weather::units::fmt_temp;
use ss_core::weather::wmo::wmo_label;
use ss_core::Weather;

use crate::state::Shared;
use crate::theme::{c32, c32a, PRO_BLUE, PRO_DOWN, PRO_UP};

pub const FKEY_H: f32 = 26.0;
pub const TICKER_H: f32 = 22.0;

/// (label, command line, key) for each function key.
pub const FUNCTION_KEYS: [(&str, &str, Key); 12] = [
    ("SAVE", "add", Key::F1),
    ("SYNC", "refresh", Key::F2),
    ("LIVE", "radar live", Key::F3),
    ("CENTER", "radar recenter", Key::F4),
    ("NEW RADR", "radar new", Key::F5),
    ("NOWCAST", "radar nowcast", Key::F6),
    ("SAT", "radar satellite", Key::F7),
    ("CELLS", "radar cells", Key::F8),
    ("ALRT", "alerts list", Key::F9),
    ("RADIO", "radio toggle", Key::F10),
    ("LAYOUT", "reset-layout", Key::F11),
    ("SETTINGS", "open settings", Key::F12),
];

fn push(cmds: &mut Vec<Command>, line: &str) {
    match parse(line) {
        Ok(c) => cmds.push(c),
        Err(e) => tracing::warn!("function key {line:?}: {e}"),
    }
}

/// A menu item that runs `line`; shows its F-key when it has one.
fn menu_item(ui: &mut Ui, label: &str, line: &str, cmds: &mut Vec<Command>) -> bool {
    let mut b = egui::Button::new(label);
    if let Some(i) = FUNCTION_KEYS.iter().position(|(_, l, _)| *l == line) {
        b = b.shortcut_text(format!("F{}", i + 1));
    }
    let clicked = ui.add(b).clicked();
    if clicked {
        push(cmds, line);
        ui.close();
    }
    clicked
}

/// The classic menu bar, revealed with Alt (or pinned from View). Returns
/// true when an item was chosen, so the caller can hide the bar again.
pub fn alt_menu(ui: &mut Ui, sh: &mut Shared, cmds: &mut Vec<Command>) -> bool {
    let p = sh.palette;
    let mut chose = false;
    egui::Panel::top("menubar")
        .resizable(false)
        .frame(
            egui::Frame::NONE
                .fill(c32(p.surface3))
                .inner_margin(egui::Margin::symmetric(8, 2)),
        )
        .show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    chose |= menu_item(ui, "Save Current Location", "add", cmds);
                    chose |= menu_item(ui, "Refresh All", "refresh", cmds);
                    ui.separator();
                    chose |= menu_item(ui, "Settings…", "open settings", cmds);
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("View", |ui| {
                    for k in ss_core::PanelKind::ALL {
                        let docked = sh.layout.is_docked(k);
                        let b = egui::Button::selectable(docked, k.title());
                        if ui.add(b).clicked() {
                            let verb = if docked { "hide" } else { "show" };
                            push(cmds, &format!("{verb} {}", k.id()));
                            ui.close();
                            chose = true;
                        }
                    }
                    ui.separator();
                    chose |= menu_item(ui, "Reset Layout", "reset-layout", cmds);
                    ui.separator();
                    if ui
                        .checkbox(&mut sh.config.menu_bar_always, "Always Show Menu Bar")
                        .changed()
                    {
                        sh.dirty_config = true;
                    }
                });
                ui.menu_button("Radar", |ui| {
                    chose |= menu_item(ui, "New Radar Window", "radar new", cmds);
                    chose |= menu_item(ui, "Pop Out Radar", "radar popout", cmds);
                    chose |= menu_item(ui, "Recenter", "radar recenter", cmds);
                    chose |= menu_item(ui, "Jump to Live", "radar live", cmds);
                    ui.separator();
                    chose |= menu_item(ui, "Nowcast", "radar nowcast", cmds);
                    chose |= menu_item(ui, "Satellite", "radar satellite", cmds);
                    chose |= menu_item(ui, "Storm Cells", "radar cells", cmds);
                    chose |= menu_item(ui, "Legend", "radar legend", cmds);
                });
                ui.menu_button("Alerts", |ui| {
                    chose |= menu_item(ui, "List Alerts", "alerts list", cmds);
                    chose |= menu_item(ui, "Clear Selection", "alerts clear", cmds);
                    chose |= menu_item(ui, "Read Top Alert Aloud", "speak", cmds);
                    chose |= menu_item(ui, "Audit Log Window", "open audit-log", cmds);
                });
                ui.menu_button("Radio", |ui| {
                    chose |= menu_item(ui, "Toggle Weather Radio", "radio toggle", cmds);
                    chose |= menu_item(ui, "Live Stream", "radio live", cmds);
                    chose |= menu_item(ui, "Simulated", "radio simulated", cmds);
                    chose |= menu_item(ui, "Stop", "radio stop", cmds);
                    chose |= menu_item(ui, "Mute All Audio", "mute", cmds);
                });
                ui.menu_button("AI", |ui| {
                    let b = egui::Button::new("Assistant…").shortcut_text("Ctrl+J");
                    if ui.add(b).clicked() {
                        push(cmds, "assistant");
                        ui.close();
                        chose = true;
                    }
                    chose |= menu_item(ui, "AI Settings…", "open settings", cmds);
                });
                ui.menu_button("Help", |ui| {
                    chose |= menu_item(ui, "Commands", "help", cmds);
                });
            });
        });
    chose
}

/// The numbered function-key row under the command line.
pub fn function_keys(ui: &mut Ui, sh: &Shared, cmds: &mut Vec<Command>) {
    let p = sh.palette;
    // F-keys work anywhere in the main window.
    for (_, line, key) in FUNCTION_KEYS {
        if ui.input(|i| i.key_pressed(key)) {
            push(cmds, line);
        }
    }
    egui::Panel::top("fkeys")
        .exact_size(FKEY_H)
        .resizable(false)
        .frame(
            egui::Frame::NONE
                .fill(c32(p.bg))
                .inner_margin(egui::Margin::symmetric(8, 2)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, (label, line, _)) in FUNCTION_KEYS.iter().enumerate() {
                    if fkey(ui, &p, i + 1, label) {
                        push(cmds, line);
                    }
                }
            });
        });
}

/// `1) SAVE`: amber number, white label; blue when hovered.
fn fkey(ui: &mut Ui, p: &ss_core::theme::Palette, n: usize, label: &str) -> bool {
    let font = FontId::monospace(11.5);
    let num = format!("{n})");
    let nw = ui
        .painter()
        .layout_no_wrap(num.clone(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let lw = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(nw + lw + 18.0, 20.0), Sense::click());
    let hot = resp.hovered();
    if hot {
        ui.painter().rect_filled(rect, 0.0, c32(PRO_BLUE));
    }
    let y = rect.center().y;
    ui.painter().text(
        egui::pos2(rect.left() + 6.0, y),
        Align2::LEFT_CENTER,
        num,
        font.clone(),
        if hot { Color32::WHITE } else { c32(p.a0) },
    );
    ui.painter().text(
        egui::pos2(rect.left() + 10.0 + nw, y),
        Align2::LEFT_CENTER,
        label,
        font,
        if hot { Color32::WHITE } else { c32(p.text) },
    );
    resp.on_hover_text(format!("F{n}")).clicked()
}

/// Temperature change over the next three hours, in °C.
pub fn three_hour_change(w: &Weather) -> Option<f64> {
    let now = match w.timezone.parse::<chrono_tz::Tz>() {
        Ok(tz) => chrono::Utc::now().with_timezone(&tz).naive_local(),
        Err(_) => chrono::Local::now().naive_local(),
    };
    let key = now.format("%Y-%m-%dT%H:00").to_string();
    let i = w.hourly.iter().position(|h| h.time == key)?;
    let later = w.hourly.get(i + 3)?;
    Some(later.temperature_c - w.hourly[i].temperature_c)
}

struct Item {
    parts: Vec<(String, Color32)>,
}

fn ticker_items(sh: &Shared) -> Vec<Item> {
    let p = sh.palette;
    let units = sh.config.units;
    sh.locations
        .saved_locations
        .iter()
        .map(|l| {
            let name = l
                .label
                .split(',')
                .next()
                .unwrap_or(&l.label)
                .trim()
                .to_uppercase();
            let mut parts = vec![(format!("{name} "), c32(p.a0))];
            if let Some(w) = sh.weather_for(&l.id) {
                parts.push((
                    format!("{} ", fmt_temp(w.current.temperature_c, units)),
                    c32(p.text),
                ));
                if let Some(d) = three_hour_change(w) {
                    let (glyph, col) = if d >= 0.0 {
                        ("▲", PRO_UP)
                    } else {
                        ("▼", PRO_DOWN)
                    };
                    let mag = match units {
                        ss_core::UnitPref::F => d.abs() * 9.0 / 5.0,
                        ss_core::UnitPref::C => d.abs(),
                    };
                    parts.push((format!("{glyph}{mag:.1} "), c32(col)));
                }
                parts.push((
                    wmo_label(w.current.weather_code).to_uppercase(),
                    c32a(p.text3),
                ));
            } else {
                parts.push(("—".into(), c32a(p.text3)));
            }
            let n = sh.alerts.get(&l.id).map_or(0, Vec::len);
            if n > 0 {
                parts.push((format!("  {n} ALRT"), c32(PRO_DOWN)));
            }
            Item { parts }
        })
        .collect()
}

/// Scrolling ticker tape: each saved place with temperature, the 3-hour
/// trend and alerts.
pub fn ticker(ui: &mut Ui, sh: &Shared) {
    let p = sh.palette;
    egui::Panel::bottom("ticker")
        .exact_size(TICKER_H)
        .resizable(false)
        .frame(egui::Frame::NONE.fill(c32(p.bg)))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            painter.hline(
                rect.x_range(),
                rect.top() + 0.5,
                Stroke::new(1.0, c32a(p.border)),
            );
            let items = ticker_items(sh);
            if items.is_empty() {
                return;
            }
            let font = FontId::monospace(11.5);
            // Lay the tape out once, then draw it twice for a seamless loop.
            let mut galleys = Vec::new();
            let mut width = 0.0;
            for it in &items {
                for (text, col) in &it.parts {
                    let g = painter.layout_no_wrap(text.clone(), font.clone(), *col);
                    width += g.size().x;
                    galleys.push(g);
                }
                let sep = painter.layout_no_wrap("   ·   ".into(), font.clone(), c32a(p.text3));
                width += sep.size().x;
                galleys.push(sep);
            }
            let width = width.max(rect.width());
            let t = ui.input(|i| i.time);
            let offset = ((t * 45.0) % f64::from(width)) as f32;
            let y = rect.center().y;
            for pass in 0..2 {
                let mut x = rect.left() - offset + pass as f32 * width;
                for g in &galleys {
                    let w = g.size().x;
                    if x + w >= rect.left() && x <= rect.right() {
                        painter.galley(
                            egui::pos2(x, y - g.size().y / 2.0),
                            g.clone(),
                            Color32::WHITE,
                        );
                    }
                    x += w;
                }
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_keys_parse_and_are_unique() {
        let mut keys = std::collections::HashSet::new();
        for (label, line, key) in FUNCTION_KEYS {
            assert!(parse(line).is_ok(), "{label}: {line:?} does not parse");
            assert!(keys.insert(key), "{label}: duplicate key");
        }
    }
}
