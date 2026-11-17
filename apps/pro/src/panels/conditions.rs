use egui::{RichText, Ui};
use ss_core::weather::outlook::today_outlook;
use ss_core::weather::units::{compass_8, fmt_precip, fmt_temp, fmt_time_full, fmt_wind, uv_label};
use ss_core::weather::wmo::wmo_label;
use ss_core::PanelKind;

use super::{error_text, placeholder, Panel, PanelCx};
use crate::icons::weather_image;
use crate::theme::{c32, c32a, on, PRO_BLUE};

pub struct ConditionsPanel;

impl Panel for ConditionsPanel {
    fn kind(&self) -> PanelKind {
        PanelKind::Conditions
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut PanelCx) {
        let p = cx.palette;
        let sh = &*cx.shared;
        let Some(loc) = cx.location.as_ref() else {
            placeholder(ui, &p, "no data");
            return;
        };
        let Some(w) = sh.weather_for(&loc.id) else {
            if let Some(e) = sh.weather_errors.get(&loc.id) {
                error_text(ui, &p, &format!("forecast failed: {e}"));
            } else if sh.refreshing {
                placeholder(ui, &p, "loading…");
            } else {
                placeholder(ui, &p, "no data");
            }
            return;
        };
        let (units, tf) = (sh.config.units, sh.config.time_format);
        let c = &w.current;

        egui::ScrollArea::vertical()
            .id_salt("conditions_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Digital readout: label block + big right-aligned value.
                ui.horizontal(|ui| {
                    ui.add(weather_image(
                        ui.ctx(),
                        c.weather_code,
                        c.is_day,
                        crate::theme::is_dark(&p),
                        34,
                    ));
                    ui.label(
                        RichText::new(wmo_label(c.weather_code).to_uppercase())
                            .monospace()
                            .color(c32(p.text)),
                    );
                });
                let wind_val = fmt_wind(c.wind_speed_kmh, units);
                let rows = [
                    ("T", fmt_temp(c.temperature_c, units), p.a0),
                    ("FL", fmt_temp(c.apparent_temperature_c, units), PRO_BLUE),
                    ("RH", format!("{:.0}%", c.relative_humidity_pct), p.a0),
                    (
                        "W",
                        format!("{wind_val} {}", compass_8(c.wind_direction_deg)),
                        PRO_BLUE,
                    ),
                ];
                for (label, value, tag) in rows {
                    readout_row(ui, &p, label, &value, tag);
                }
                ui.add_space(4.0);

                let (uv_text, uv_col) = uv_label(c.uv_index);
                let sun = w.daily.first();
                egui::Grid::new("conditions_grid")
                    .num_columns(2)
                    .spacing([10.0, 2.0])
                    .show(ui, |ui| {
                        let mut row = |k: &str, v: RichText| {
                            ui.label(RichText::new(k).color(c32a(p.text3)));
                            ui.label(v);
                            ui.end_row();
                        };
                        row(
                            "precip",
                            RichText::new(fmt_precip(c.precipitation_mm, units)),
                        );
                        row("cloud", RichText::new(format!("{:.0}%", c.cloud_cover_pct)));
                        row(
                            "uv",
                            RichText::new(format!("{:.0} {uv_text}", c.uv_index))
                                .color(c32(uv_col)),
                        );
                        if let Some(d) = sun {
                            row("sunrise", RichText::new(fmt_time_full(&d.sunrise, tf)));
                            row("sunset", RichText::new(fmt_time_full(&d.sunset, tf)));
                        }
                    });

                if let Some(text) = today_outlook(w, units) {
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(RichText::new(text).color(c32a(p.text2))).wrap());
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("{:?} · {}", w.provider, w.timezone))
                        .small()
                        .color(c32a(p.text3)),
                );
                if let Some(e) = sh.weather_errors.get(&loc.id) {
                    error_text(ui, &p, &format!("last refresh failed: {e}"));
                }
            });
    }
}

/// One DRO-style line: a coloured label block on the left and a large
/// monospace value right-aligned on black.
fn readout_row(ui: &mut Ui, p: &ss_core::theme::Palette, label: &str, value: &str, tag: [u8; 3]) {
    let h = 34.0;
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::BLACK);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, c32a(p.border)),
        egui::StrokeKind::Inside,
    );
    let tag_rect = egui::Rect::from_min_size(rect.min, egui::vec2(38.0, h));
    painter.rect_filled(tag_rect, 0.0, c32(tag));
    painter.text(
        tag_rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::monospace(16.0),
        on(tag),
    );
    painter.text(
        egui::pos2(rect.right() - 10.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        value,
        egui::FontId::monospace(22.0),
        egui::Color32::WHITE,
    );
}
