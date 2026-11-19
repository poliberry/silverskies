//! Header row as a terminal command line: logomark, the active place as an
//! amber security field with `WX <GO>`, coordinates, date and clock.

use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Timelike};
use egui::{Align, FontId, Label, Layout, RichText, Sense, Ui};
use ss_core::weather::units::fmt_clock;

use crate::icons::logo_texture;
use crate::state::Shared;
use crate::theme::{c32, c32a, is_dark, PRO_BAR, PRO_UP, PRO_YELLOW};

#[derive(Default)]
pub struct HeaderAction {
    pub logo_clicked: bool,
    /// The UPDATE chip was clicked.
    pub update_clicked: bool,
}

const CLICKS_NEEDED: u8 = 10;
const CLICK_WINDOW: Duration = Duration::from_secs(3);

/// Register a logo click; returns true when the easter egg should fire
/// (state is reset in that case).
pub fn count_click(state: &mut (u8, Instant), now: Instant) -> bool {
    if state.0 == 0 || now.duration_since(state.1) > CLICK_WINDOW {
        state.0 = 0;
    }
    state.0 += 1;
    state.1 = now;
    if state.0 >= CLICKS_NEEDED {
        state.0 = 0;
        true
    } else {
        false
    }
}

fn coords(lat: f64, lon: f64) -> String {
    format!(
        "{:.2}°{}  {:.2}°{}",
        lat.abs(),
        if lat < 0.0 { "S" } else { "N" },
        lon.abs(),
        if lon < 0.0 { "W" } else { "E" }
    )
}

/// (hour, minute) of `now` in the IANA zone, falling back to local time.
fn clock_parts(now: DateTime<Local>, tz: Option<&str>) -> (u32, u32) {
    if let Some(z) = tz.and_then(|s| s.parse::<chrono_tz::Tz>().ok()) {
        let t = now.with_timezone(&z);
        return (t.hour(), t.minute());
    }
    (now.hour(), now.minute())
}

/// Paint a solid block with `text` centred in it; returns the block's width.
fn block(
    ui: &mut Ui,
    text: &str,
    font: FontId,
    fill: egui::Color32,
    fg: egui::Color32,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, fg);
    let size = egui::vec2(galley.size().x + 16.0, ui.available_height().min(26.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp
}

pub fn show(
    ui: &mut Ui,
    sh: &Shared,
    _pulse: Option<[u8; 3]>,
    now: DateTime<Local>,
) -> HeaderAction {
    let p = sh.palette;
    let mut action = HeaderAction::default();
    let ctx = ui.ctx().clone();
    let dark = is_dark(&p);
    let tex = logo_texture(&ctx, dark, 26);
    let ts = tex.size_vec2();
    let size = egui::vec2(26.0 * ts.x / ts.y.max(1.0), 26.0);
    let mono = FontId::monospace(12.0);
    let amber = c32(p.a0);

    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let img = egui::Image::from_texture(&tex).fit_to_exact_size(size);
        let r = ui.add(img.sense(Sense::click()));
        if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            action.logo_clicked = true;
        }
        ui.add_space(14.0);

        // The "security" field of a terminal command line: the active place
        // as a ticker, the WX function and a green <GO> key.
        let (ticker, coord) = match sh.active_location() {
            Some(l) => (l.label.to_uppercase(), coords(l.lat, l.lon)),
            None => ("LOCATING…".to_string(), String::new()),
        };
        block(
            ui,
            &format!("{ticker} "),
            FontId::monospace(13.0),
            c32(PRO_YELLOW),
            egui::Color32::BLACK,
        );
        block(ui, "WX", FontId::monospace(13.0), c32(PRO_BAR), amber);
        block(
            ui,
            "<GO>",
            FontId::monospace(12.0),
            c32(PRO_UP),
            egui::Color32::BLACK,
        );
        ui.add_space(12.0);
        if !coord.is_empty() {
            ui.add(
                Label::new(RichText::new(coord).font(mono.clone()).color(c32a(p.text3))).truncate(),
            );
        }

        let tz = sh.active_weather().map(|w| w.timezone.as_str());
        let (hh, mm) = clock_parts(now, tz);
        let clock = fmt_clock(hh, mm, sh.config.time_format);
        let refresh = sh.last_refresh.map(|t| {
            let ago = chrono::Duration::from_std(t.elapsed()).unwrap_or_default();
            let (h, m) = clock_parts(now - ago, tz);
            fmt_clock(h, m, sh.config.time_format)
        });
        let date = match tz.and_then(|z| z.parse::<chrono_tz::Tz>().ok()) {
            Some(z) => now.with_timezone(&z).format("%a %m/%d/%y").to_string(),
            None => now.format("%a %m/%d/%y").to_string(),
        }
        .to_uppercase();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(
                RichText::new(clock)
                    .font(FontId::monospace(14.0))
                    .color(c32(p.text)),
            );
            ui.label(RichText::new(date).font(mono.clone()).color(amber));
            ui.label(
                RichText::new(format!("SYNC {}", refresh.unwrap_or_else(|| "—".into())))
                    .font(mono.clone())
                    .color(c32a(p.text3)),
            );
            if let Some(r) = sh.update_available() {
                let chip = egui::Button::new(
                    RichText::new(format!("UPDATE {}", r.tag.trim_start_matches('v')))
                        .font(mono.clone())
                        .color(crate::theme::on(p.a0)),
                )
                .fill(amber)
                .corner_radius(0.0);
                if ui
                    .add(chip)
                    .on_hover_text(format!(
                        "Silver Skies Pro {} is available. Click to update.",
                        r.label()
                    ))
                    .clicked()
                {
                    action.update_clicked = true;
                }
            }
        });
    });

    // Thin amber rule under the command line.
    let r = ui.max_rect();
    ui.painter()
        .hline(r.x_range(), r.bottom() + 5.0, egui::Stroke::new(1.0, amber));
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss_core::Config;

    #[test]
    fn ten_clicks_within_window_fire() {
        let t0 = Instant::now();
        let mut st = (0u8, t0);
        for i in 0..9 {
            assert!(!count_click(&mut st, t0 + Duration::from_millis(i * 200)));
        }
        assert!(count_click(&mut st, t0 + Duration::from_millis(1900)));
        assert_eq!(st.0, 0);
    }

    #[test]
    fn slow_clicks_reset() {
        let t0 = Instant::now();
        let mut st = (0u8, t0);
        for i in 0..9 {
            count_click(&mut st, t0 + Duration::from_millis(i * 100));
        }
        assert!(!count_click(&mut st, t0 + Duration::from_secs(10)));
        assert_eq!(st.0, 1);
    }

    #[test]
    fn coords_format() {
        assert_eq!(coords(-33.9, 151.2), "33.90°S  151.20°E");
        assert_eq!(coords(40.714, -74.006), "40.71°N  74.01°W");
    }

    #[test]
    fn clock_uses_zone_and_falls_back() {
        let now = Local::now();
        let (h, _) = clock_parts(now, Some("UTC"));
        assert_eq!(h, now.with_timezone(&chrono::Utc).hour());
        let local = (now.hour(), now.minute());
        assert_eq!(clock_parts(now, Some("Nope/Nothing")), local);
        assert_eq!(clock_parts(now, None), local);
    }

    #[test]
    fn header_renders_headless() {
        let sh = Shared::new(Config::default(), Default::default(), Default::default());
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &sh, None, Local::now());
        });
        out.textures_delta.clear();
    }
}
