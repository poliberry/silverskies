//! SOUNDING: the forecast vertical profile at the tracked location (Open-Meteo
//! pressure levels), drawn as a Skew-T log-P diagram with the surface parcel,
//! a hodograph with the Bunkers right-mover, and the severe-weather indices.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{
    pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui,
};
use ss_core::sounding::{Indices, Sounding};
use ss_core::theme::Palette;
use ss_core::PanelKind;

use super::{error_text, placeholder, Panel, PanelCx};
use crate::theme::{c32, c32a, PRO_DOWN, PRO_UP, PRO_YELLOW};

const P_TOP: f64 = 200.0;
const T_LEFT: f64 = -40.0;
const T_RIGHT: f64 = 45.0;
/// Re-fetch soundings this often.
const REFRESH: Duration = Duration::from_secs(60 * 60);

#[derive(Default)]
pub struct SoundingPanel {
    /// Chosen forecast hour (index into the location's soundings).
    hour: Option<usize>,
    requested: HashMap<String, Instant>,
}

/// Skew-T coordinate mapping for a plot rect.
struct Skew {
    r: Rect,
    p_bot: f64,
}

impl Skew {
    fn y(&self, p: f64) -> f32 {
        let f = (self.p_bot / p).ln() / (self.p_bot / P_TOP).ln();
        self.r.bottom() - (f as f32) * self.r.height()
    }
    /// Isotherms lean right by one plot width over the full height (45°-ish).
    fn x(&self, t: f64, p: f64) -> f32 {
        let base = ((t - T_LEFT) / (T_RIGHT - T_LEFT)) as f32 * self.r.width();
        let lean = (self.r.bottom() - self.y(p)) * 0.9;
        self.r.left() + base + lean
    }
    fn pt(&self, t: f64, p: f64) -> Pos2 {
        pos2(self.x(t, p), self.y(p))
    }
}

fn polyline(painter: &Painter, pts: Vec<Pos2>, stroke: Stroke) {
    if pts.len() >= 2 {
        painter.add(Shape::line(pts, stroke));
    }
}

fn skew_t(ui: &mut Ui, rect: Rect, s: &Sounding, ix: &Indices, p: &Palette) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, c32(p.bg));
    let plot = Rect::from_min_max(rect.min + vec2(34.0, 6.0), rect.max - vec2(34.0, 18.0));
    let sk = Skew {
        r: plot,
        p_bot: s
            .levels
            .first()
            .map(|l| l.pressure_hpa)
            .unwrap_or(1000.0)
            .max(850.0)
            + 15.0,
    };
    let clip = painter.with_clip_rect(plot);
    let grid = c32a(p.border);
    let font = FontId::monospace(10.0);
    // Isobars.
    for pr in [
        1000.0, 925.0, 850.0, 700.0, 500.0, 400.0, 300.0, 250.0, 200.0,
    ] {
        if pr > sk.p_bot {
            continue;
        }
        let y = sk.y(pr);
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0, grid),
        );
        painter.text(
            pos2(plot.left() - 4.0, y),
            Align2::RIGHT_CENTER,
            format!("{pr:.0}"),
            font.clone(),
            c32a(p.text3),
        );
    }
    // Isotherms (0 °C highlighted).
    for t in (-110..=50).step_by(10) {
        let t = f64::from(t);
        let col = if t == 0.0 {
            Color32::from_rgb(0x3d, 0x8b, 0xff)
        } else {
            grid
        };
        clip.line_segment([sk.pt(t, sk.p_bot), sk.pt(t, P_TOP)], Stroke::new(1.0, col));
        let bx = sk.x(t, sk.p_bot);
        if (plot.left()..=plot.right()).contains(&bx) && t >= T_LEFT {
            painter.text(
                pos2(bx, plot.bottom() + 2.0),
                Align2::CENTER_TOP,
                format!("{t:.0}"),
                font.clone(),
                c32a(p.text3),
            );
        }
    }
    // Dry adiabats (θ every 10 K), faint dashes.
    for theta in (250..=450).step_by(10) {
        let th = f64::from(theta);
        let pts: Vec<Pos2> = (0..=30)
            .map(|i| {
                let pr = sk.p_bot - (sk.p_bot - P_TOP) * f64::from(i) / 30.0;
                sk.pt(th * (pr / 1000.0).powf(0.2857) - 273.15, pr)
            })
            .collect();
        clip.add(Shape::dashed_line(
            &pts,
            Stroke::new(1.0, grid.gamma_multiply(0.7)),
            3.0,
            5.0,
        ));
    }
    // Parcel (dashed), then environment: temperature red, dewpoint green.
    let parcel: Vec<Pos2> = s.parcel().iter().map(|(pr, t)| sk.pt(*t, *pr)).collect();
    clip.add(Shape::dashed_line(
        &parcel,
        Stroke::new(1.5, c32(PRO_YELLOW)),
        5.0,
        4.0,
    ));
    let t_line: Vec<Pos2> = s
        .levels
        .iter()
        .map(|l| sk.pt(l.temp_c, l.pressure_hpa))
        .collect();
    let td_line: Vec<Pos2> = s
        .levels
        .iter()
        .map(|l| sk.pt(l.dewpoint_c, l.pressure_hpa))
        .collect();
    polyline(&clip, t_line, Stroke::new(2.5, c32(PRO_DOWN)));
    polyline(&clip, td_line, Stroke::new(2.5, c32(PRO_UP)));
    // LCL tick.
    if ix.lcl_pressure_hpa > P_TOP {
        let y = sk.y(ix.lcl_pressure_hpa);
        painter.line_segment(
            [pos2(plot.right() - 30.0, y), pos2(plot.right() - 6.0, y)],
            Stroke::new(1.5, c32(p.text)),
        );
        painter.text(
            pos2(plot.right() - 32.0, y),
            Align2::RIGHT_CENTER,
            "LCL",
            font.clone(),
            c32(p.text),
        );
    }
    // Wind barbs down the right edge.
    let bx = rect.right() - 16.0;
    for l in &s.levels {
        if l.pressure_hpa < P_TOP {
            continue;
        }
        barb(
            &painter,
            pos2(bx, sk.y(l.pressure_hpa)),
            l.wind_kt,
            l.wind_dir_deg,
            c32a(p.text2),
        );
    }
    painter.rect_stroke(
        plot,
        0.0,
        Stroke::new(1.0, c32a(p.border)),
        egui::StrokeKind::Inside,
    );
}

/// A standard wind barb (staff toward where the wind comes from).
fn barb(painter: &Painter, at: Pos2, kt: f64, from_deg: f64, col: Color32) {
    let stroke = Stroke::new(1.3, col);
    if kt < 2.5 {
        painter.circle_stroke(at, 3.0, stroke);
        return;
    }
    let a = (from_deg as f32).to_radians();
    let dir = vec2(a.sin(), -a.cos()); // toward the source
    let perp = vec2(-dir.y, dir.x);
    let len = 22.0;
    let tip = at + dir * len;
    painter.line_segment([at, tip], stroke);
    let (pennants, full, half) = ss_map::barb_components(kt as f32);
    let mut pos = 0.0;
    for _ in 0..pennants {
        let b = tip - dir * pos;
        let c = tip - dir * (pos + 5.0);
        painter.add(Shape::convex_polygon(
            vec![b, b + perp * 9.0, c],
            col,
            Stroke::NONE,
        ));
        pos += 6.0;
    }
    for _ in 0..full {
        let b = tip - dir * pos;
        painter.line_segment([b, b + perp * 9.0 + dir * 3.0], stroke);
        pos += 4.0;
    }
    for _ in 0..half {
        let b = tip - dir * pos.max(3.0);
        painter.line_segment([b, b + perp * 5.0 + dir * 1.5], stroke);
    }
}

fn hodograph(ui: &mut Ui, rect: Rect, s: &Sounding, ix: &Indices, p: &Palette) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, c32(p.bg));
    let c = rect.center();
    let max_kt = 80.0_f32;
    let scale = rect.width().min(rect.height()) * 0.46 / max_kt;
    let font = FontId::monospace(9.5);
    for ring in [20.0, 40.0, 60.0, 80.0] {
        painter.circle_stroke(c, ring * scale, Stroke::new(1.0, c32a(p.border)));
        painter.text(
            c + vec2(ring * scale + 2.0, -2.0),
            Align2::LEFT_BOTTOM,
            format!("{ring:.0}"),
            font.clone(),
            c32a(p.text3),
        );
    }
    painter.line_segment(
        [pos2(rect.left(), c.y), pos2(rect.right(), c.y)],
        Stroke::new(1.0, c32a(p.border)),
    );
    painter.line_segment(
        [pos2(c.x, rect.top()), pos2(c.x, rect.bottom())],
        Stroke::new(1.0, c32a(p.border)),
    );
    let to_screen = |u: f64, v: f64| -> Pos2 {
        // m/s → kt, v up.
        c + vec2(
            (u / 0.514_444) as f32 * scale,
            -(v / 0.514_444) as f32 * scale,
        )
    };
    // Trace coloured by layer: 0–3 km red, 3–6 green, 6–9 yellow, 9+ cyan.
    let colors = [
        (3000.0, Color32::from_rgb(0xff, 0x45, 0x45)),
        (6000.0, Color32::from_rgb(0x2e, 0xd5, 0x73)),
        (9000.0, Color32::from_rgb(0xff, 0xd4, 0x00)),
        (12000.0, Color32::from_rgb(0x14, 0xc8, 0xc8)),
    ];
    let mut prev: Option<(Pos2, f64)> = None;
    let mut h = 0.0;
    while h <= 12000.0 {
        if let Some((u, v)) = s.wind_at(h) {
            let pt = to_screen(u, v);
            if let Some((pp, ph)) = prev {
                let col = colors
                    .iter()
                    .find(|(top, _)| ph < *top)
                    .map(|(_, c)| *c)
                    .unwrap_or(colors[3].1);
                painter.line_segment([pp, pt], Stroke::new(2.5, col));
            }
            for km in [1.0, 3.0, 6.0] {
                if (h - km * 1000.0).abs() < 1.0 {
                    painter.circle_filled(pt, 2.5, c32(p.text));
                    painter.text(
                        pt + vec2(4.0, -3.0),
                        Align2::LEFT_BOTTOM,
                        format!("{km:.0}"),
                        font.clone(),
                        c32(p.text),
                    );
                }
            }
            prev = Some((pt, h));
        }
        h += 250.0;
    }
    // Bunkers right mover.
    let (dir, spd) = ix.bunkers_rm;
    if spd > 0.0 {
        let a = dir.to_radians();
        let (u, v) = (-spd * 0.514_444 * a.sin(), -spd * 0.514_444 * a.cos());
        let rm = to_screen(u, v);
        painter.circle_stroke(rm, 4.0, Stroke::new(1.5, c32(p.text)));
        painter.text(
            rm + vec2(6.0, 0.0),
            Align2::LEFT_CENTER,
            "RM",
            font.clone(),
            c32(p.text),
        );
    }
    painter.text(
        rect.left_top() + vec2(4.0, 2.0),
        Align2::LEFT_TOP,
        "HODOGRAPH (kt)",
        font,
        c32(p.a0),
    );
}

fn indices_table(ui: &mut Ui, ix: &Indices, p: &Palette) {
    let fmt_m = |m: Option<f64>| m.map(|v| format!("{v:.0} m")).unwrap_or_else(|| "—".into());
    let warn = |v: f64, lo: f64, hi: f64| -> Color32 {
        if v >= hi {
            c32(PRO_DOWN)
        } else if v >= lo {
            c32(PRO_YELLOW)
        } else {
            c32(p.text)
        }
    };
    let rows: Vec<(&str, String, Color32)> = vec![
        (
            "SBCAPE",
            format!("{:.0} J/kg", ix.cape),
            warn(ix.cape, 1000.0, 2500.0),
        ),
        ("SBCIN", format!("{:.0} J/kg", ix.cin), c32(p.text)),
        (
            "LI",
            format!("{:+.1} °C", ix.lifted_index),
            warn(-ix.lifted_index, 3.0, 6.0),
        ),
        ("LCL", format!("{:.0} m", ix.lcl_m), c32(p.text)),
        ("LFC", fmt_m(ix.lfc_m), c32(p.text)),
        ("EL", fmt_m(ix.el_m), c32(p.text)),
        ("FRZ LVL", fmt_m(ix.freezing_m), c32(p.text)),
        (
            "700-500 LR",
            ix.lapse_700_500
                .map(|v| format!("{v:.1} °C/km"))
                .unwrap_or_else(|| "—".into()),
            warn(ix.lapse_700_500.unwrap_or(0.0), 7.0, 8.0),
        ),
        ("PWAT", format!("{:.0} mm", ix.pwat_mm), c32(p.text)),
        (
            "0-1 SHEAR",
            format!("{:.0} kt", ix.shear_01_kt),
            warn(ix.shear_01_kt, 15.0, 25.0),
        ),
        (
            "0-6 SHEAR",
            format!("{:.0} kt", ix.shear_06_kt),
            warn(ix.shear_06_kt, 35.0, 50.0),
        ),
        (
            "0-1 SRH",
            format!("{:.0} m²/s²", ix.srh_01),
            warn(ix.srh_01, 100.0, 250.0),
        ),
        (
            "0-3 SRH",
            format!("{:.0} m²/s²", ix.srh_03),
            warn(ix.srh_03, 150.0, 300.0),
        ),
        (
            "BUNKERS RM",
            format!("{:03.0}/{:.0} kt", ix.bunkers_rm.0, ix.bunkers_rm.1),
            c32(p.text),
        ),
        ("SCP", format!("{:.1}", ix.scp), warn(ix.scp, 1.0, 4.0)),
        ("STP", format!("{:.1}", ix.stp), warn(ix.stp, 0.5, 1.0)),
    ];
    egui::Grid::new("sounding_indices")
        .num_columns(2)
        .spacing([10.0, 1.0])
        .show(ui, |ui| {
            for (k, v, col) in rows {
                ui.label(RichText::new(k).monospace().color(c32a(p.text2)));
                ui.label(RichText::new(v).monospace().color(col));
                ui.end_row();
            }
        });
}

impl Panel for SoundingPanel {
    fn kind(&self) -> PanelKind {
        PanelKind::Sounding
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut PanelCx) {
        let p = cx.palette;
        let Some(loc) = cx.location.clone() else {
            placeholder(ui, &p, "no location");
            return;
        };
        // Fetch (and hourly re-fetch) the location's soundings.
        // Failures (e.g. a busy server) retry after a couple of minutes.
        let failed = cx.shared.sounding_errors.contains_key(&loc.id);
        let stale = self.requested.get(&loc.id).is_none_or(|t| {
            t.elapsed() >= REFRESH || (failed && t.elapsed() >= Duration::from_secs(120))
        });
        if stale && !cx.shared.soundings_pending.contains(&loc.id) {
            self.requested.insert(loc.id.clone(), Instant::now());
            cx.shared.soundings_pending.insert(loc.id.clone());
            cx.tasks.soundings(&loc);
        }
        let Some(list) = cx.shared.soundings.get(&loc.id).filter(|l| !l.is_empty()) else {
            if let Some(e) = cx.shared.sounding_errors.get(&loc.id) {
                error_text(ui, &p, &format!("sounding failed: {e}"));
            } else {
                placeholder(ui, &p, "loading sounding…");
            }
            return;
        };
        // Default to the current hour at the location.
        let now_hour = chrono::Local::now().format("%Y-%m-%dT%H:00").to_string();
        let tz_now = cx
            .shared
            .weather_for(&loc.id)
            .and_then(|w| w.timezone.parse::<chrono_tz::Tz>().ok())
            .map(|tz| {
                chrono::Utc::now()
                    .with_timezone(&tz)
                    .format("%Y-%m-%dT%H:00")
                    .to_string()
            })
            .unwrap_or(now_hour);
        let default = list.iter().position(|s| s.time >= tz_now).unwrap_or(0);
        let mut i = self.hour.unwrap_or(default).min(list.len() - 1);
        ui.horizontal(|ui| {
            ui.label(RichText::new("valid").monospace().color(c32a(p.text3)));
            ui.label(
                RichText::new(list[i].time.replace('T', " "))
                    .monospace()
                    .color(c32(p.a0)),
            );
            ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(60.0);
            if ui
                .add(egui::Slider::new(&mut i, 0..=list.len() - 1).show_value(false))
                .changed()
            {
                self.hour = Some(i);
            }
            if ui.small_button("now").clicked() {
                self.hour = None;
            }
        });
        let s = &list[i];
        let ix = s.indices();
        let avail = ui.available_rect_before_wrap();
        let wide = avail.width() >= 620.0;
        if wide {
            let skew_w = (avail.width() * 0.58).max(300.0);
            let skew_r = Rect::from_min_size(avail.min, vec2(skew_w, avail.height()));
            let side = Rect::from_min_max(pos2(skew_r.right() + 6.0, avail.top()), avail.max);
            let hodo_h = side.width().min(side.height() * 0.55);
            let hodo_r = Rect::from_min_size(side.min, vec2(side.width(), hodo_h));
            ui.allocate_rect(avail, Sense::hover());
            skew_t(ui, skew_r, s, &ix, &p);
            hodograph(ui, hodo_r, s, &ix, &p);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(
                pos2(side.left(), hodo_r.bottom() + 6.0),
                side.max,
            )));
            egui::ScrollArea::vertical()
                .id_salt("sounding_ix")
                .show(&mut child, |ui| indices_table(ui, &ix, &p));
        } else {
            // Compact: the Skew-T fills the pane, the hodograph is an inset
            // in its top-right corner and the key indices are overlaid.
            ui.allocate_rect(avail, Sense::hover());
            skew_t(ui, avail, s, &ix, &p);
            let inset = (avail.width().min(avail.height()) * 0.42).clamp(90.0, 220.0);
            let hodo_r = Rect::from_min_size(
                pos2(avail.right() - 40.0 - inset, avail.top() + 8.0),
                vec2(inset, inset),
            );
            hodograph(ui, hodo_r, s, &ix, &p);
            ui.painter().rect_stroke(
                hodo_r,
                0.0,
                Stroke::new(1.0, c32a(p.border)),
                egui::StrokeKind::Inside,
            );
            let key = [
                format!("CAPE {:.0}", ix.cape),
                format!("CIN {:.0}", ix.cin),
                format!("SRH1 {:.0}", ix.srh_01),
                format!("SHR6 {:.0}kt", ix.shear_06_kt),
                format!("SCP {:.1}", ix.scp),
                format!("STP {:.1}", ix.stp),
            ];
            let painter = ui.painter_at(avail);
            for (k, line) in key.iter().enumerate() {
                let pos = pos2(avail.left() + 40.0, avail.top() + 10.0 + k as f32 * 13.0);
                painter.text(
                    pos + vec2(1.0, 1.0),
                    Align2::LEFT_TOP,
                    line,
                    FontId::monospace(11.0),
                    Color32::from_black_alpha(200),
                );
                painter.text(
                    pos,
                    Align2::LEFT_TOP,
                    line,
                    FontId::monospace(11.0),
                    c32(p.text),
                );
            }
        }
    }
}
