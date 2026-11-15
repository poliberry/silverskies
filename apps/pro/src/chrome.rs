//! Workstation chrome under the header: a menu bar and a toolbar of block
//! buttons. Every item just issues the same command you could type at `:`.

use egui::{Align2, Color32, FontId, Sense, Stroke, StrokeKind, Ui};
use ss_core::command::{parse, Command};
use ss_core::PanelKind;

use crate::state::Shared;
use crate::theme::{c32, c32a, on, PRO_PURPLE};

pub const MENU_H: f32 = 24.0;
pub const TOOLBAR_H: f32 = 40.0;

fn push(cmds: &mut Vec<Command>, line: &str) {
    match parse(line) {
        Ok(c) => cmds.push(c),
        Err(e) => tracing::warn!("menu command {line:?}: {e}"),
    }
}

/// A menu entry that runs `line` when clicked.
fn item(ui: &mut Ui, label: &str, line: &str, cmds: &mut Vec<Command>) {
    if ui.button(label).clicked() {
        push(cmds, line);
        ui.close();
    }
}

pub fn menu_and_toolbar(ui: &mut Ui, sh: &Shared, cmds: &mut Vec<Command>) {
    let p = sh.palette;
    egui::Panel::top("menubar")
        .exact_size(MENU_H)
        .resizable(false)
        .frame(
            egui::Frame::NONE
                .fill(c32(p.surface))
                .inner_margin(egui::Margin::symmetric(8, 2)),
        )
        .show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    item(ui, "Save current location", "add", cmds);
                    item(ui, "Refresh now", "refresh", cmds);
                    ui.separator();
                    item(ui, "Settings…", "open settings", cmds);
                });
                ui.menu_button("View", |ui| {
                    for k in PanelKind::ALL {
                        let docked = sh.layout.is_docked(k);
                        let label = format!("{} {}", if docked { "■" } else { "□" }, k.title());
                        let verb = if docked { "hide" } else { "show" };
                        item(ui, &label, &format!("{verb} {}", k.id()), cmds);
                    }
                    ui.separator();
                    item(ui, "Reset layout", "reset-layout", cmds);
                });
                ui.menu_button("Radar", |ui| {
                    item(ui, "New radar window", "radar new", cmds);
                    item(ui, "Pop out radar", "radar popout", cmds);
                    item(ui, "Recenter", "radar recenter", cmds);
                    item(ui, "Jump to live", "radar live", cmds);
                    ui.separator();
                    item(ui, "Toggle nowcast", "radar nowcast", cmds);
                    item(ui, "Toggle satellite", "radar satellite", cmds);
                    item(ui, "Toggle storm cells", "radar cells", cmds);
                    item(ui, "Toggle legend", "radar legend", cmds);
                });
                ui.menu_button("Alerts", |ui| {
                    item(ui, "List alerts", "alerts list", cmds);
                    item(ui, "Clear selection", "alerts clear", cmds);
                    item(ui, "Read top alert aloud", "speak", cmds);
                    item(ui, "Open audit log window", "open audit-log", cmds);
                });
                ui.menu_button("Radio", |ui| {
                    item(ui, "Toggle weather radio", "radio toggle", cmds);
                    item(ui, "Live stream", "radio live", cmds);
                    item(ui, "Simulated", "radio simulated", cmds);
                    item(ui, "Stop", "radio stop", cmds);
                    item(ui, "Mute all audio", "mute", cmds);
                });
                ui.menu_button("Help", |ui| {
                    item(ui, "Commands", "help", cmds);
                });
            });
        });

    egui::Panel::top("toolbar")
        .exact_size(TOOLBAR_H)
        .resizable(false)
        .frame(
            egui::Frame::NONE
                .fill(c32(p.bg))
                .inner_margin(egui::Margin::symmetric(8, 5)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                // (label, command, group break after)
                let tools: [(&str, &str, bool); 11] = [
                    ("+ SAVE", "add", false),
                    ("↻ SYNC", "refresh", true),
                    ("◂◂ LIVE", "radar live", false),
                    ("⊕ CENTER", "radar recenter", false),
                    ("▣ NEW RADAR", "radar new", true),
                    ("NOWCAST", "radar nowcast", false),
                    ("SAT", "radar satellite", false),
                    ("CELLS", "radar cells", true),
                    ("ALERTS", "alerts list", false),
                    ("RADIO", "radio toggle", true),
                    ("SETTINGS", "open settings", false),
                ];
                for (label, line, sep) in tools {
                    if tool_button(ui, &p, label) {
                        push(cmds, line);
                    }
                    if sep {
                        ui.add_space(6.0);
                        let (r, _) = ui.allocate_exact_size(egui::vec2(2.0, 22.0), Sense::hover());
                        ui.painter().rect_filled(r, 0.0, c32(PRO_PURPLE));
                        ui.add_space(6.0);
                    }
                }
            });
            let r = ui.max_rect();
            ui.painter().hline(
                r.x_range(),
                r.bottom() + 4.0,
                Stroke::new(1.0, c32a(p.border)),
            );
        });
}

/// A hard-edged toolbar block; returns true when clicked.
fn tool_button(ui: &mut Ui, p: &ss_core::theme::Palette, label: &str) -> bool {
    let font = FontId::monospace(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE);
    let size = egui::vec2(galley.size().x + 16.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (fill, fg) = if resp.is_pointer_button_down_on() {
        (PRO_PURPLE, on(PRO_PURPLE))
    } else if resp.hovered() {
        (p.a0, on(p.a0))
    } else {
        (p.surface3, c32(p.text))
    };
    ui.painter().rect_filled(rect, 0.0, c32(fill));
    if !resp.hovered() {
        ui.painter().rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, c32a(p.border)),
            StrokeKind::Inside,
        );
    }
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, label, font, fg);
    resp.clicked()
}
