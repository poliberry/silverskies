//! The places sidebar, the forecast bento grid and the alerts list, drawn
//! in the web design's style: square hairline tiles, spaced uppercase mono
//! captions, light Google Sans Flex numbers and an accent that follows the sky.

use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, Layout, Margin, Rect, RichText, ScrollArea,
    Sense, Stroke, Ui, UiBuilder,
};
use ss_core::alerts::colors::class_color;
use ss_core::theme::Palette;
use ss_core::weather::units::{
    compass_8, fmt_date_short, fmt_precip, fmt_temp, fmt_time, fmt_time_full, fmt_wind, uv_label,
};
use ss_core::weather::wmo::wmo_label;
use ss_core::{Alert, Severity, TimeFormat, UnitPref, Weather};

use crate::app::{Action, AiState, App, Tab};
use crate::search::{self, Intent};
use crate::summary;
use crate::theme::{
    accent2, alpha, c32, c32a, caps, card_frame, glow, is_dark, light_font, mono_font, paint_tile,
    section, semibold_font, temp_font, temp_font_medium, GAP,
};

/// Numeric temperature without the unit: `72`.
fn num(c: f64, u: UnitPref) -> String {
    fmt_temp(c, u)
        .trim_end_matches(['F', 'C'])
        .trim_end_matches('°')
        .to_string()
}

/// `72°F`.
fn temp(c: f64, u: UnitPref) -> String {
    fmt_temp(c, u)
}

fn unit_label(u: UnitPref) -> &'static str {
    match u {
        UnitPref::F => "°F",
        UnitPref::C => "°C",
    }
}

fn icon(ui: &Ui, painter: &egui::Painter, code: u8, is_day: bool, p: &Palette, rect: Rect) {
    let tex = ss_assets::weather_texture(ui.ctx(), code, is_day, is_dark(p), rect.height() as u32);
    painter.image(
        tex.id(),
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// Draw `add` inside a hairline tile filling `rect`.
fn cell(ui: &mut Ui, rect: Rect, p: &Palette, salt: &str, add: impl FnOnce(&mut Ui)) {
    paint_tile(ui, rect, p);
    let inner = rect.shrink(20.0);
    if inner.width() < 10.0 || inner.height() < 10.0 {
        return;
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt(("bento", salt))
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child);
}

// ---------------------------------------------------------------------------
// Places
// ---------------------------------------------------------------------------

pub fn places(ui: &mut Ui, app: &App, actions: &mut Vec<Action>) {
    let p = app.palette;
    section(ui, &p, "Places");
    ui.add_space(4.0);
    ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for l in &app.locations.saved_locations {
                let active = app.locations.active_location_id.as_deref() == Some(&l.id);
                let w = app.weather.get(&l.id);
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
                let hovered = resp.hovered();
                let fill = if active {
                    glow(&p)
                } else if hovered {
                    alpha(p.a0, 16)
                } else {
                    c32(p.surface)
                };
                let border = if active {
                    alpha(p.a0, 128)
                } else {
                    c32a(p.border)
                };
                ui.painter().rect_filled(rect, 0.0, fill);
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, border),
                    egui::StrokeKind::Inside,
                );
                if active {
                    ui.painter().rect_filled(
                        Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
                        0.0,
                        c32(p.a0),
                    );
                }
                let painter = ui.painter_at(rect);
                let name = l.label.split(',').next().unwrap_or(&l.label).trim();
                let region = l.label.split_once(',').map(|(_, r)| r.trim()).unwrap_or("");
                painter.text(
                    pos2(rect.left() + 48.0, rect.top() + 19.0),
                    Align2::LEFT_CENTER,
                    name,
                    semibold_font(14.5),
                    if active { accent2(&p) } else { c32(p.text) },
                );
                let region_g = painter.layout_no_wrap(
                    if region.is_empty() {
                        "—".into()
                    } else {
                        region.to_uppercase()
                    },
                    crate::theme::mono_medium_font(10.0),
                    c32a(p.text3),
                );
                painter.galley(
                    pos2(rect.left() + 48.0, rect.top() + 32.0),
                    region_g,
                    c32a(p.text3),
                );
                if let Some(w) = w {
                    icon(
                        ui,
                        &painter,
                        w.current.weather_code,
                        w.current.is_day,
                        &p,
                        Rect::from_center_size(
                            pos2(rect.left() + 25.0, rect.center().y),
                            vec2(28.0, 28.0),
                        ),
                    );
                    if !hovered {
                        painter.text(
                            pos2(rect.right() - 12.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            format!("{}°", num(w.current.temperature_c, app.config.units)),
                            temp_font_medium(19.0),
                            c32(p.text),
                        );
                    }
                } else if app.loading.contains(&l.id) {
                    painter.text(
                        pos2(rect.left() + 25.0, rect.center().y),
                        Align2::CENTER_CENTER,
                        "…",
                        mono_font(14.0),
                        c32a(p.text3),
                    );
                }
                if app.alerts.get(&l.id).is_some_and(|a| !a.is_empty()) && !hovered {
                    painter.rect_filled(
                        Rect::from_center_size(
                            pos2(rect.left() + 38.0, rect.top() + 12.0),
                            vec2(6.0, 6.0),
                        ),
                        0.0,
                        c32(p.danger),
                    );
                }
                let mut removed = false;
                if hovered {
                    let x = Rect::from_center_size(
                        pos2(rect.right() - 20.0, rect.center().y),
                        vec2(24.0, 24.0),
                    );
                    let xr = ui.interact(x, egui::Id::unique(("rm", &l.id)), Sense::click());
                    ui.painter().rect_filled(
                        x,
                        0.0,
                        if xr.hovered() {
                            alpha(p.danger, 70)
                        } else {
                            alpha(p.text, 14)
                        },
                    );
                    ui.painter().text(
                        x.center(),
                        Align2::CENTER_CENTER,
                        "✕",
                        mono_font(11.0),
                        c32(p.text),
                    );
                    if xr.on_hover_text("Remove place").clicked() {
                        actions.push(Action::Remove(l.id.clone()));
                        removed = true;
                    }
                }
                if resp.clicked() && !removed {
                    actions.push(Action::Select(l.id.clone()));
                    actions.push(Action::Tab(Tab::Forecast));
                }
            }
            ui.add_space(4.0);
            let label = if app.locating {
                "LOCATING…"
            } else {
                "+  USE MY LOCATION"
            };
            let btn = egui::Button::new(caps(label, 11.0, accent2(&p)))
                .min_size(vec2(ui.available_width(), 34.0));
            if ui.add_enabled(!app.locating, btn).clicked() {
                actions.push(Action::Intent(Intent::UseMyLocation));
            }
            if app.locations.saved_locations.is_empty() && !app.locating {
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Search for a city, airport or landmark above to add it here.",
                        )
                        .size(12.5)
                        .color(c32a(p.text3)),
                    )
                    .wrap(),
                );
            }
        });
}

// ---------------------------------------------------------------------------
// Forecast: the bento grid
// ---------------------------------------------------------------------------

/// What goes in one bento cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Hero,
    Summary,
    Humidity,
    Wind,
    WindDir,
    Precip,
    Hourly,
    Week,
    Uv,
    Cloud,
    Sun,
    Feels,
}

/// One row of the grid: a height and slots of `(span, cells stacked
/// vertically)` on a 3-unit grid.
struct Row {
    height: f32,
    slots: Vec<(u8, Vec<Cell>)>,
}

fn grid(daily: usize) -> Vec<Row> {
    let week_h = 64.0 + 46.0 * daily.clamp(3, 7) as f32;
    vec![
        Row {
            height: 300.0,
            slots: vec![(2, vec![Cell::Hero]), (1, vec![Cell::Summary])],
        },
        Row {
            height: 128.0,
            slots: vec![
                (0, vec![Cell::Humidity]),
                (0, vec![Cell::Wind]),
                (0, vec![Cell::WindDir]),
                (0, vec![Cell::Precip]),
            ],
        },
        Row {
            height: 156.0,
            slots: vec![(3, vec![Cell::Hourly])],
        },
        Row {
            height: week_h,
            slots: vec![(2, vec![Cell::Week]), (1, vec![Cell::Uv, Cell::Cloud])],
        },
        Row {
            height: 210.0,
            slots: vec![(2, vec![Cell::Sun]), (1, vec![Cell::Feels])],
        },
    ]
}

/// Lay a row out: span-0 slots share the width equally (stat tiles); in
/// narrow windows every other slot gets a full-width row of its own.
fn slot_rects(row: &Row, left: f32, top: f32, width: f32, wide: bool) -> Vec<(Rect, Vec<Cell>)> {
    let equal = row.slots.iter().all(|(s, _)| *s == 0);
    if equal {
        let n = row.slots.len();
        let per_row = if wide || n <= 2 { n } else { 2 };
        let w = (width - GAP * (per_row as f32 - 1.0)) / per_row as f32;
        return row
            .slots
            .iter()
            .enumerate()
            .map(|(i, (_, cells))| {
                let (r, c) = (i / per_row, i % per_row);
                let x = left + c as f32 * (w + GAP);
                let y = top + r as f32 * (row.height + GAP);
                (
                    Rect::from_min_size(pos2(x, y), vec2(w, row.height)),
                    cells.clone(),
                )
            })
            .collect();
    }
    if !wide {
        let mut y = top;
        return row
            .slots
            .iter()
            .map(|(_, cells)| {
                let h = if cells.len() > 1 {
                    row.height * 0.62
                } else {
                    row.height
                };
                let r = Rect::from_min_size(pos2(left, y), vec2(width, h));
                y += h + GAP;
                (r, cells.clone())
            })
            .collect();
    }
    let unit = (width - 2.0 * GAP) / 3.0;
    let mut x = left;
    row.slots
        .iter()
        .map(|(span, cells)| {
            let s = f32::from(*span);
            let w = s * unit + (s - 1.0) * GAP;
            let r = Rect::from_min_size(pos2(x, top), vec2(w, row.height));
            x += w + GAP;
            (r, cells.clone())
        })
        .collect()
}

pub fn forecast(ui: &mut Ui, app: &mut App, actions: &mut Vec<Action>) {
    let p = app.palette;
    let Some(loc) = app.active().cloned() else {
        empty_state(
            ui,
            &p,
            if app.locating {
                "Finding your location…"
            } else {
                "Search for a place to see its forecast."
            },
        );
        return;
    };
    let Some(w) = app.weather.get(&loc.id).cloned() else {
        match app.errors.get(&loc.id) {
            Some(e) => empty_state(ui, &p, e),
            None => {
                ui.add_space(80.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Spinner::new().size(26.0).color(c32(p.a0)));
                    ui.label(caps(&format!("Loading {}", loc.label), 11.0, c32a(p.text3)));
                });
            }
        }
        return;
    };
    let now = App::now_for(&w);
    let alerts = app.active_alerts();
    let scroll_to = app.scroll_to.take();
    let full_w = ui.available_width() - 12.0;
    let wide = full_w >= 860.0;
    let mut hero_rect = Rect::NOTHING;

    let out = ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(full_w);
            ui.set_max_width(full_w);
            ui.spacing_mut().item_spacing.y = GAP;

            if !alerts.is_empty() {
                alert_banner(ui, &p, &alerts, actions);
            }
            if let Some(ans) = app.answer.clone() {
                if ans.location_id == loc.id {
                    answer_card(
                        ui,
                        &p,
                        &w,
                        &ans,
                        app.config.units,
                        app.config.time_format,
                        now,
                        actions,
                    );
                }
            }

            for row in grid(w.daily.len()) {
                let rects = slot_rects(&row, 0.0, 0.0, full_w, wide);
                let total_h = rects.iter().map(|(r, _)| r.bottom()).fold(0.0, f32::max);
                let (area, _) = ui.allocate_exact_size(vec2(full_w, total_h), Sense::hover());
                for (r, cells) in rects {
                    let r = r.translate(area.min.to_vec2());
                    let n = cells.len() as f32;
                    let h = (r.height() - GAP * (n - 1.0)) / n;
                    for (i, c) in cells.into_iter().enumerate() {
                        let cr = Rect::from_min_size(
                            pos2(r.left(), r.top() + i as f32 * (h + GAP)),
                            vec2(r.width(), h),
                        );
                        if c == Cell::Hero {
                            hero_rect = cr;
                        }
                        let target = match c {
                            Cell::Hero => Some(search::View::Forecast),
                            Cell::Hourly => Some(search::View::Hourly),
                            Cell::Week => Some(search::View::Week),
                            _ => None,
                        };
                        if target.is_some() && target == scroll_to {
                            ui.scroll_to_rect(cr, Some(Align::TOP));
                        }
                        draw_cell(ui, app, &p, &loc.label, &w, &alerts, now, c, cr, actions);
                    }
                }
            }

            ui.horizontal(|ui| {
                ui.label(caps("Silver Skies", 11.0, accent2(&p)));
                ui.label(caps(
                    "· Open-Meteo · WMO via LibreWXR · Google weather icons",
                    10.0,
                    c32a(p.text3),
                ));
            });
            ui.add_space(14.0);
        });

    // Condensed bar once the hero has scrolled out of view.
    let top = out.inner_rect.top();
    if hero_rect.is_finite() && hero_rect.bottom() < top + 8.0 {
        condensed_bar(
            ui,
            &p,
            &loc.label,
            &w,
            app.config.units,
            out.inner_rect.left(),
            top,
            full_w,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_cell(
    ui: &mut Ui,
    app: &App,
    p: &Palette,
    place: &str,
    w: &Weather,
    alerts: &[Alert],
    now: chrono::NaiveDateTime,
    c: Cell,
    rect: Rect,
    actions: &mut Vec<Action>,
) {
    let units = app.config.units;
    let tf = app.config.time_format;
    let cur = &w.current;
    match c {
        Cell::Hero => cell(ui, rect, p, "hero", |ui| {
            hero(ui, p, place, w, units, now, tf)
        }),
        Cell::Summary => cell(ui, rect, p, "summary", |ui| {
            summary_tile(ui, app, p, w, alerts, now, actions)
        }),
        Cell::Humidity => stat(
            ui,
            rect,
            p,
            "Humidity",
            format!("{:.0}", cur.relative_humidity_pct),
            "%",
            None,
        ),
        Cell::Wind => {
            let s = fmt_wind(cur.wind_speed_kmh, units);
            let (v, u) = s.split_once(' ').unwrap_or((&s, ""));
            stat(ui, rect, p, "Wind", v.to_string(), u, None)
        }
        Cell::WindDir => stat(
            ui,
            rect,
            p,
            "Wind dir",
            compass_8(cur.wind_direction_deg).to_string(),
            "",
            Some(format!("{:.0}°", cur.wind_direction_deg)),
        ),
        Cell::Precip => {
            let s = fmt_precip(cur.precipitation_mm, units);
            let (v, u) = s.split_once(' ').unwrap_or((&s, ""));
            stat(
                ui,
                rect,
                p,
                "Precipitation",
                v.to_string(),
                u,
                Some("last hour".into()),
            )
        }
        Cell::Hourly => cell(ui, rect, p, "hourly", |ui| hourly(ui, p, w, units, tf, now)),
        Cell::Week => cell(ui, rect, p, "week", |ui| {
            week(ui, app, p, w, units, tf, actions)
        }),
        Cell::Uv => cell(ui, rect, p, "uv", |ui| uv(ui, p, cur.uv_index)),
        Cell::Cloud => cell(ui, rect, p, "cloud", |ui| {
            ui.label(caps("Cloud cover", 11.0, c32a(p.text3)));
            ui.label(RichText::new(format!("{:.0}%", cur.cloud_cover_pct)).font(light_font(38.0)));
            ui.label(
                RichText::new(cloud_desc(cur.cloud_cover_pct))
                    .size(15.0)
                    .color(c32a(p.text2)),
            );
        }),
        Cell::Feels => cell(ui, rect, p, "feels", |ui| {
            ui.label(caps("Apparent temp", 11.0, c32a(p.text3)));
            ui.label(RichText::new(temp(cur.apparent_temperature_c, units)).font(light_font(38.0)));
            ui.label(
                RichText::new("Feels like outside")
                    .size(15.0)
                    .color(c32a(p.text2)),
            );
            let diff = cur.apparent_temperature_c - cur.temperature_c;
            if diff.abs() >= 1.0 {
                ui.add_space(6.0);
                ui.label(caps(
                    &format!(
                        "{} than actual",
                        if diff > 0.0 { "Warmer" } else { "Colder" }
                    ),
                    10.0,
                    accent2(p),
                ));
            }
        }),
        Cell::Sun => cell(ui, rect, p, "sun", |ui| sun(ui, p, w, tf, now)),
    }
}

fn hero(
    ui: &mut Ui,
    p: &Palette,
    place: &str,
    w: &Weather,
    units: UnitPref,
    now: chrono::NaiveDateTime,
    tf: TimeFormat,
) {
    let rect = ui.max_rect();
    let cur = &w.current;
    ui.horizontal(|ui| {
        ui.label(caps("Current conditions", 11.0, c32a(p.text3)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let clock = match tf {
                TimeFormat::H12 => "%-I:%M %p",
                TimeFormat::H24 => "%H:%M",
            };
            ui.label(caps(
                &format!("Local {}", now.format(clock)),
                10.0,
                c32a(p.text3),
            ));
        });
    });
    ui.add(
        egui::Label::new(
            RichText::new(place)
                .font(light_font(20.0))
                .color(c32a(p.text2)),
        )
        .truncate(),
    );
    // Big thin temperature with a raised unit.
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(
            RichText::new(num(cur.temperature_c, units))
                .font(temp_font(92.0))
                .color(c32(p.text)),
        );
        ui.add_space(2.0);
        ui.vertical(|ui| {
            ui.add_space(14.0);
            ui.label(
                RichText::new(unit_label(units))
                    .font(light_font(40.0))
                    .color(c32a(p.text2)),
            );
        });
    });
    ui.label(
        RichText::new(wmo_label(cur.weather_code))
            .font(light_font(22.0))
            .color(c32a(p.text2)),
    );
    if let Some(d) = w.daily.first() {
        ui.label(caps(
            &format!(
                "Feels like {} · High {} · Low {}",
                temp(cur.apparent_temperature_c, units),
                temp(d.temp_max_c, units),
                temp(d.temp_min_c, units)
            ),
            11.0,
            c32a(p.text3),
        ));
    }
    // Icon on the right with a soft accent glow beneath it.
    let size = (rect.height() * 0.62).clamp(80.0, 150.0);
    let c = pos2(rect.right() - size / 2.0 - 4.0, rect.center().y - 6.0);
    let painter = ui.painter_at(rect.expand(20.0));
    let glow_r = Rect::from_center_size(c + vec2(0.0, size * 0.55), vec2(size * 0.9, size * 0.18));
    for i in 0..6 {
        let t = i as f32 / 6.0;
        painter.rect_filled(
            glow_r.shrink2(vec2(glow_r.width() * t * 0.4, glow_r.height() * t * 0.4)),
            CornerRadius::same(255),
            alpha(p.a0, 10),
        );
    }
    icon(
        ui,
        &painter,
        cur.weather_code,
        cur.is_day,
        p,
        Rect::from_center_size(c, vec2(size, size)),
    );
}

#[allow(clippy::too_many_arguments)]
fn stat(
    ui: &mut Ui,
    rect: Rect,
    p: &Palette,
    label: &str,
    value: String,
    unit: &str,
    sub: Option<String>,
) {
    let salt = label.to_string();
    cell(ui, rect, p, &salt, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(caps(label, 11.0, c32a(p.text3)));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.label(
                RichText::new(value)
                    .font(light_font(32.0))
                    .color(c32(p.text)),
            );
            if !unit.is_empty() {
                ui.vertical(|ui| {
                    ui.add_space(14.0);
                    ui.label(RichText::new(unit).size(13.0).color(c32a(p.text3)));
                });
            }
        });
        if let Some(s) = sub {
            ui.label(caps(&s, 10.0, c32a(p.text3)));
        }
    });
}

fn hourly(
    ui: &mut Ui,
    p: &Palette,
    w: &Weather,
    units: UnitPref,
    tf: TimeFormat,
    now: chrono::NaiveDateTime,
) {
    ui.label(caps("Hourly", 11.0, c32a(p.text3)));
    ui.add_space(2.0);
    ScrollArea::horizontal()
        .id_salt("hourly")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let i = summary::current_hour_index(&w.hourly, now);
                for (k, h) in w.hourly.iter().skip(i).take(24).enumerate() {
                    let (r, _) = ui.allocate_exact_size(vec2(78.0, 92.0), Sense::hover());
                    let is_now = k == 0;
                    let painter = ui.painter_at(r);
                    painter.rect_filled(r, 0.0, if is_now { glow(p) } else { c32(p.surface3) });
                    painter.rect_stroke(
                        r,
                        0.0,
                        Stroke::new(
                            1.0,
                            if is_now {
                                alpha(p.a0, 128)
                            } else {
                                c32a(p.border)
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    if is_now {
                        painter.rect_filled(
                            Rect::from_min_size(r.min, vec2(r.width(), 2.0)),
                            0.0,
                            c32(p.a0),
                        );
                    }
                    let t = if is_now {
                        "NOW".to_string()
                    } else {
                        fmt_time(&h.time, tf).to_uppercase()
                    };
                    painter.text(
                        pos2(r.center().x, r.top() + 15.0),
                        Align2::CENTER_CENTER,
                        t,
                        crate::theme::mono_medium_font(10.5),
                        if is_now { accent2(p) } else { c32a(p.text3) },
                    );
                    icon(
                        ui,
                        &painter,
                        h.weather_code,
                        h.is_day,
                        p,
                        Rect::from_center_size(
                            pos2(r.center().x, r.top() + 42.0),
                            vec2(28.0, 28.0),
                        ),
                    );
                    painter.text(
                        pos2(r.center().x, r.top() + 70.0),
                        Align2::CENTER_CENTER,
                        temp(h.temperature_c, units),
                        semibold_font(14.5),
                        if is_now { accent2(p) } else { c32(p.text) },
                    );
                    let pop = h.precipitation_probability_pct.round();
                    if pop >= 10.0 {
                        painter.text(
                            pos2(r.center().x, r.top() + 84.0),
                            Align2::CENTER_CENTER,
                            format!("{pop}%"),
                            mono_font(9.5),
                            accent2(p),
                        );
                    }
                }
            });
        });
}

/// Cold-to-warm colour for the range bars (the design's blue→orange).
fn temp_color(t: f32) -> Color32 {
    let cold = [0x6c, 0x8c, 0xff];
    let warm = [0xff, 0x9a, 0x4a];
    let t = t.clamp(0.0, 1.0);
    let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgb(
        m(cold[0], warm[0]),
        m(cold[1], warm[1]),
        m(cold[2], warm[2]),
    )
}

fn week(
    ui: &mut Ui,
    app: &App,
    p: &Palette,
    w: &Weather,
    units: UnitPref,
    tf: TimeFormat,
    actions: &mut Vec<Action>,
) {
    let n = w.daily.len().min(7);
    ui.label(caps(&format!("{n}-day forecast"), 11.0, c32a(p.text3)));
    ui.add_space(2.0);
    let lo = w
        .daily
        .iter()
        .map(|d| d.temp_min_c)
        .fold(f64::INFINITY, f64::min);
    let hi = w
        .daily
        .iter()
        .map(|d| d.temp_max_c)
        .fold(f64::NEG_INFINITY, f64::max);
    ui.spacing_mut().item_spacing.y = 6.0;
    for (i, d) in w.daily.iter().enumerate().take(7) {
        let focused = app.focus_day == Some(i);
        let today = i == 0;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
        let fill = if focused {
            alpha(p.a0, 46)
        } else if today {
            glow(p)
        } else if resp.hovered() {
            alpha(p.a0, 14)
        } else {
            c32(p.surface3)
        };
        ui.painter().rect_filled(rect, 0.0, fill);
        ui.painter().rect_stroke(
            rect,
            0.0,
            Stroke::new(
                1.0,
                if today || focused {
                    alpha(p.a0, 110)
                } else {
                    c32a(p.border)
                },
            ),
            egui::StrokeKind::Inside,
        );
        if resp.clicked() {
            actions.push(Action::FocusDay(if focused { None } else { Some(i) }));
        }
        let painter = ui.painter_at(rect);
        let y = rect.center().y;
        let name = if today {
            "Today".to_string()
        } else {
            fmt_date_short(&d.date)
                .split(' ')
                .next()
                .unwrap_or("")
                .to_string()
        };
        painter.text(
            pos2(rect.left() + 14.0, y),
            Align2::LEFT_CENTER,
            name,
            semibold_font(15.0),
            if today { accent2(p) } else { c32(p.text) },
        );
        icon(
            ui,
            &painter,
            d.weather_code,
            true,
            p,
            Rect::from_center_size(pos2(rect.left() + 104.0, y), vec2(26.0, 26.0)),
        );
        let pop = d.precipitation_probability_max_pct.round();
        painter.text(
            pos2(rect.right() - 12.0, y),
            Align2::RIGHT_CENTER,
            format!("{pop}%"),
            mono_font(11.5),
            accent2(p),
        );
        painter.text(
            pos2(rect.right() - 54.0, y),
            Align2::RIGHT_CENTER,
            temp(d.temp_max_c, units),
            light_font(19.0),
            c32(p.text),
        );
        // Range bar: track, then a cold→warm gradient segment, low label under it.
        let bar_l = rect.left() + 134.0;
        let bar_r = rect.right() - 120.0;
        if bar_r - bar_l > 40.0 && hi > lo {
            let x = |t: f64| bar_l + ((t - lo) / (hi - lo)) as f32 * (bar_r - bar_l);
            let by = y - 5.0;
            painter.rect_filled(
                Rect::from_min_max(pos2(bar_l, by - 1.0), pos2(bar_r, by + 1.0)),
                0.0,
                alpha(p.text, 18),
            );
            let (x0, x1) = (x(d.temp_min_c), x(d.temp_max_c).max(x(d.temp_min_c) + 4.0));
            let t0 = ((d.temp_min_c - lo) / (hi - lo)) as f32;
            let t1 = ((d.temp_max_c - lo) / (hi - lo)) as f32;
            let mut mesh = egui::Mesh::default();
            for (xx, c) in [(x0, temp_color(t0)), (x1, temp_color(t1))] {
                mesh.colored_vertex(pos2(xx, by - 1.5), c);
                mesh.colored_vertex(pos2(xx, by + 1.5), c);
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(1, 3, 2);
            painter.add(mesh);
            painter.text(
                pos2(bar_l, y + 8.0),
                Align2::LEFT_CENTER,
                temp(d.temp_min_c, units),
                mono_font(10.0),
                c32a(p.text3),
            );
        }
    }
    if let Some(i) = app.focus_day {
        if let Some(d) = w.daily.get(i) {
            ui.label(caps(
                &format!(
                    "{} · {} · sunrise {} · sunset {} · UV {:.0}",
                    summary::day_name(&w.daily, i),
                    wmo_label(d.weather_code),
                    fmt_time_full(&d.sunrise, tf),
                    fmt_time_full(&d.sunset, tf),
                    d.uv_index_max
                ),
                10.0,
                accent2(p),
            ));
        }
    }
}

fn uv(ui: &mut Ui, p: &Palette, uv: f64) {
    let (uv_text, uv_rgb) = uv_label(uv);
    ui.label(caps("UV index", 11.0, c32a(p.text3)));
    ui.label(
        RichText::new(format!("{uv:.1}"))
            .font(light_font(38.0))
            .color(c32(uv_rgb)),
    );
    ui.label(
        RichText::new(format!(
            "{uv_text} — {}",
            if uv > 5.0 {
                "SPF recommended"
            } else {
                "Low risk"
            }
        ))
        .size(15.0)
        .color(c32a(p.text2)),
    );
    ui.add_space(4.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
    let bar = Rect::from_min_max(
        pos2(r.left() + 4.0, r.center().y - 1.5),
        pos2(r.right() - 4.0, r.center().y + 1.5),
    );
    let stops: [[u8; 3]; 5] = [
        [0x64, 0xff, 0xb4],
        [0xf2, 0xe0, 0x2c],
        [0xff, 0x9a, 0x2a],
        [0xff, 0x55, 0x55],
        [0xff, 0x3d, 0xc8],
    ];
    let mut mesh = egui::Mesh::default();
    for (i, col) in stops.iter().enumerate() {
        let x = bar.left() + bar.width() * i as f32 / (stops.len() - 1) as f32;
        mesh.colored_vertex(pos2(x, bar.top()), c32(*col));
        mesh.colored_vertex(pos2(x, bar.bottom()), c32(*col));
    }
    for i in 0..(stops.len() as u32 - 1) {
        let b = i * 2;
        mesh.add_triangle(b, b + 1, b + 2);
        mesh.add_triangle(b + 1, b + 3, b + 2);
    }
    ui.painter().add(mesh);
    let mx = bar.left() + bar.width() * (uv / 11.0).clamp(0.0, 1.0) as f32;
    ui.painter().circle(
        pos2(mx, bar.center().y),
        5.5,
        Color32::WHITE,
        Stroke::new(1.0, c32a(p.border)),
    );
}

fn sun(ui: &mut Ui, p: &Palette, w: &Weather, tf: TimeFormat, now: chrono::NaiveDateTime) {
    ui.label(caps("Sun position", 11.0, c32a(p.text3)));
    let Some(d) = w.daily.first() else { return };
    let area = ui.available_rect_before_wrap();
    let side_w = 200.0_f32.min(area.width() * 0.4);
    let arc_rect = Rect::from_min_max(area.min, pos2(area.right() - side_w - 10.0, area.bottom()));
    let painter = ui.painter_at(area.expand(8.0));

    // The arc.
    let base_y = arc_rect.bottom() - 22.0;
    let radius = (arc_rect.width() / 2.0 - 16.0)
        .min(base_y - arc_rect.top() - 8.0)
        .max(10.0);
    let centre = pos2(arc_rect.center().x, base_y);
    let pt = |t: f64| {
        let a = std::f64::consts::PI * (1.0 - t);
        centre + vec2((a.cos() as f32) * radius, -(a.sin() as f32) * radius)
    };
    painter.add(egui::Shape::dashed_line(
        &[
            pos2(arc_rect.left() + 4.0, base_y),
            pos2(arc_rect.right() - 4.0, base_y),
        ],
        Stroke::new(1.0, c32a(p.border)),
        3.0,
        4.0,
    ));
    let arc: Vec<_> = (0..=60).map(|i| pt(f64::from(i) / 60.0)).collect();
    painter.add(egui::Shape::dashed_line(
        &arc,
        Stroke::new(1.0, alpha(p.text, 40)),
        3.0,
        4.0,
    ));
    let sun_yellow = Color32::from_rgb(0xf4, 0xc8, 0x2a);
    match summary_sun(d, now) {
        Some(t) => {
            let done: Vec<_> = (0..=60).map(|i| pt(t * f64::from(i) / 60.0)).collect();
            painter.add(egui::Shape::line(
                done,
                Stroke::new(1.5, alpha([0xf4, 0xc8, 0x2a], 160)),
            ));
            let dot = pt(t);
            painter.circle_filled(dot, 14.0, alpha([0xf4, 0xc8, 0x2a], 40));
            painter.circle_filled(dot, 7.0, sun_yellow);
        }
        None => {
            // Night: the sun waits on the horizon at the sunrise end.
            painter.circle_filled(pt(0.0), 6.0, alpha([0xf4, 0xc8, 0x2a], 110));
        }
    }
    for (x, align, t) in [
        (arc_rect.left() + 4.0, Align2::LEFT_TOP, &d.sunrise),
        (arc_rect.right() - 4.0, Align2::RIGHT_TOP, &d.sunset),
    ] {
        painter.text(
            pos2(x, base_y + 6.0),
            align,
            fmt_time_full(t, tf),
            mono_font(10.5),
            c32a(p.text3),
        );
    }

    // The times, on the right.
    let side = Rect::from_min_max(pos2(area.right() - side_w, area.top()), area.max);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(side)
            .layout(Layout::top_down(Align::Min)),
    );
    child.spacing_mut().item_spacing.y = 2.0;
    child.label(caps("Sunrise", 10.0, c32a(p.text3)));
    child.label(RichText::new(fmt_time_full(&d.sunrise, tf)).font(light_font(26.0)));
    child.add_space(8.0);
    child.label(caps("Sunset", 10.0, c32a(p.text3)));
    child.label(RichText::new(fmt_time_full(&d.sunset, tf)).font(light_font(26.0)));
    if let Some(dl) = daylight(&d.sunrise, &d.sunset) {
        child.add_space(8.0);
        child.label(caps(&format!("Daylight · {dl}"), 11.0, accent2(p)));
    }
}

fn summary_sun(d: &ss_core::DailyPoint, now: chrono::NaiveDateTime) -> Option<f64> {
    sun_progress(&d.sunrise, &d.sunset, now)
}

#[allow(clippy::too_many_arguments)]
fn condensed_bar(
    ui: &mut Ui,
    p: &Palette,
    place: &str,
    w: &Weather,
    units: UnitPref,
    left: f32,
    top: f32,
    width: f32,
) {
    let bar = Rect::from_min_size(pos2(left, top), vec2(width, 54.0));
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::unique("condensed_bar"),
    ));
    painter.rect_filled(bar, 0.0, c32(p.surface));
    painter.rect_stroke(
        bar,
        0.0,
        Stroke::new(1.0, c32a(p.border)),
        egui::StrokeKind::Inside,
    );
    painter.rect_filled(
        Rect::from_min_size(bar.left_bottom() - vec2(0.0, 2.0), vec2(width, 2.0)),
        0.0,
        c32(p.a0),
    );
    icon(
        ui,
        &painter,
        w.current.weather_code,
        w.current.is_day,
        p,
        Rect::from_center_size(pos2(bar.left() + 32.0, bar.center().y), vec2(32.0, 32.0)),
    );
    let g = painter.layout_no_wrap(
        place.split(',').next().unwrap_or(place).to_string(),
        semibold_font(16.0),
        c32(p.text),
    );
    let tw = g.size().x;
    painter.galley(
        pos2(bar.left() + 62.0, bar.center().y - g.size().y / 2.0),
        g,
        c32(p.text),
    );
    let t = painter.layout_no_wrap(
        temp(w.current.temperature_c, units),
        temp_font_medium(22.0),
        c32(p.text),
    );
    let ttw = t.size().x;
    painter.galley(
        pos2(bar.left() + 76.0 + tw, bar.center().y - t.size().y / 2.0),
        t,
        c32(p.text),
    );
    painter.text(
        pos2(bar.left() + 90.0 + tw + ttw, bar.center().y),
        Align2::LEFT_CENTER,
        wmo_label(w.current.weather_code),
        light_font(16.0),
        c32a(p.text2),
    );
}

fn alert_banner(ui: &mut Ui, p: &Palette, alerts: &[Alert], actions: &mut Vec<Action>) {
    let a = &alerts[0];
    let c = class_color(&a.class);
    let fill_a = if matches!(a.severity, Severity::Extreme | Severity::Severe) {
        let t = ui.input(|i| i.time);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(40));
        (40.0 + 26.0 * (t * 2.5).sin()) as u8
    } else {
        30
    };
    let r = egui::Frame::new()
        .fill(alpha(c, fill_a))
        .stroke(Stroke::new(1.0, alpha(c, 150)))
        .inner_margin(Margin::symmetric(18, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("⚠").size(16.0).color(c32(c)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(caps("View alerts →", 10.5, c32a(p.text2)));
                    if alerts.len() > 1 {
                        ui.label(caps(
                            &format!("+{} more", alerts.len() - 1),
                            10.5,
                            c32a(p.text2),
                        ));
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(a.display_event.to_uppercase())
                                    .font(semibold_font(13.5))
                                    .extra_letter_spacing(1.0)
                                    .color(c32(p.text)),
                            )
                            .truncate(),
                        );
                    });
                });
            });
        })
        .response
        .interact(Sense::click());
    if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        actions.push(Action::Tab(Tab::Alerts));
    }
}

fn empty_state(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(90.0);
    ui.vertical_centered(|ui| {
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .font(light_font(20.0))
                    .color(c32a(p.text2)),
            )
            .wrap(),
        );
    });
}

fn ai_block(ui: &mut Ui, p: &Palette, state: &AiState) {
    match state {
        AiState::Idle => {}
        AiState::Waiting | AiState::Loading(_) => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(13.0).color(c32(p.a0)));
                ui.label(caps("Asking the AI…", 10.0, c32a(p.text3)));
            });
        }
        AiState::Done(t) => {
            egui::Frame::new()
                .fill(glow(p))
                .stroke(Stroke::new(1.0, alpha(p.a0, 90)))
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(caps("✦ AI", 10.0, accent2(p)));
                    ui.add(egui::Label::new(RichText::new(t).color(c32(p.text))).wrap());
                });
        }
        AiState::Failed(e) => {
            ui.add(egui::Label::new(RichText::new(e).color(c32(p.danger))).wrap());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn answer_card(
    ui: &mut Ui,
    p: &Palette,
    w: &Weather,
    ans: &crate::app::Answer,
    units: UnitPref,
    tf: TimeFormat,
    now: chrono::NaiveDateTime,
    actions: &mut Vec<Action>,
) {
    card_frame(p)
        .fill(glow(p))
        .stroke(Stroke::new(1.0, alpha(p.a0, 120)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(caps("You asked", 10.5, accent2(p)));
                ui.label(RichText::new(format!("“{}”", ans.question)).color(c32a(p.text2)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Dismiss").clicked() {
                        actions.push(Action::DismissAnswer);
                    }
                });
            });
            let local = summary::answer(w, ans.topic, ans.day, units, tf, now);
            ui.add(
                egui::Label::new(
                    RichText::new(local)
                        .font(light_font(20.0))
                        .color(c32(p.text)),
                )
                .wrap(),
            );
            ai_block(ui, p, &ans.ai);
        });
}

fn summary_tile(
    ui: &mut Ui,
    app: &App,
    p: &Palette,
    w: &Weather,
    alerts: &[Alert],
    now: chrono::NaiveDateTime,
    actions: &mut Vec<Action>,
) {
    let s = summary::summarize(w, alerts, app.config.units, app.config.time_format, now);
    ui.horizontal(|ui| {
        ui.label(caps("Summary", 11.0, c32a(p.text3)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let id = app.active_id().unwrap_or_default();
            let state = app.ai_summary.get(&id);
            let busy = matches!(state, Some(AiState::Loading(_) | AiState::Waiting));
            let (label, action) = if app.prefs.ai_key().is_some() {
                (
                    if matches!(state, Some(AiState::Done(_))) {
                        "✦ Redo"
                    } else {
                        "✦ AI"
                    },
                    Action::AiSummary,
                )
            } else {
                ("✦ Turn on AI", Action::OpenSettings)
            };
            let btn = egui::Button::new(caps(label, 9.5, accent2(p))).min_size(vec2(0.0, 22.0));
            if ui.add_enabled(!busy, btn).clicked() {
                actions.push(action);
            }
        });
    });
    ScrollArea::vertical()
        .id_salt("summary_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(&s.headline)
                        .font(light_font(17.0))
                        .color(c32(p.text)),
                )
                .wrap(),
            );
            ui.spacing_mut().item_spacing.y = 5.0;
            for pt in &s.points {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("—").color(c32(p.a0)));
                    ui.add(
                        egui::Label::new(RichText::new(pt).size(13.5).color(c32a(p.text2))).wrap(),
                    );
                });
            }
            if let Some(state) = app.active_id().and_then(|id| app.ai_summary.get(&id)) {
                ui.add_space(6.0);
                ai_block(ui, p, state);
            }
        });
}

/// Cloud cover in words.
pub fn cloud_desc(pct: f64) -> &'static str {
    if pct < 20.0 {
        "Clear skies"
    } else if pct < 50.0 {
        "Partly cloudy"
    } else if pct < 85.0 {
        "Mostly cloudy"
    } else {
        "Overcast"
    }
}

/// Minutes since midnight of an ISO local time like `2026-08-10T06:12`.
fn minutes_of(iso: &str) -> Option<f64> {
    let t = chrono::NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M").ok()?;
    use chrono::Timelike;
    Some(f64::from(t.hour() * 60 + t.minute()))
}

/// Fraction of the way from sunrise to sunset (0..=1), or `None` at night.
pub fn sun_progress(sunrise: &str, sunset: &str, now: chrono::NaiveDateTime) -> Option<f64> {
    use chrono::Timelike;
    let (rise, set) = (minutes_of(sunrise)?, minutes_of(sunset)?);
    let m = f64::from(now.hour() * 60 + now.minute());
    (set > rise && (rise..=set).contains(&m)).then(|| (m - rise) / (set - rise))
}

/// Daylight length as `10h 12m`.
pub fn daylight(sunrise: &str, sunset: &str) -> Option<String> {
    let d = (minutes_of(sunset)? - minutes_of(sunrise)?).max(0.0) as i64;
    Some(format!("{}h {}m", d / 60, d % 60))
}

// ---------------------------------------------------------------------------
// Alerts
// ---------------------------------------------------------------------------

fn severity_text(s: Severity) -> &'static str {
    match s {
        Severity::Extreme => "Extreme",
        Severity::Severe => "Severe",
        Severity::Moderate => "Moderate",
        Severity::Minor => "Minor",
        Severity::Unknown => "Unknown",
    }
}

fn fmt_until(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%a %-d %b, %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| iso.to_string())
}

pub fn alerts(ui: &mut Ui, app: &App, _actions: &mut Vec<Action>) {
    let p = app.palette;
    let Some(loc) = app.active() else {
        empty_state(ui, &p, "Pick a place to see its alerts.");
        return;
    };
    let alerts = app.active_alerts();
    let full_w = ui.available_width() - 12.0;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(full_w);
            ui.set_max_width(full_w);
            ui.spacing_mut().item_spacing.y = 10.0;
            section(ui, &p, &format!("Active alerts · {}", alerts.len()));
            if alerts.is_empty() {
                card_frame(&p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("No active alerts").font(light_font(24.0)));
                    ui.label(
                        RichText::new(format!(
                            "There are no official warnings, watches or advisories for {} right now.",
                            loc.label
                        ))
                        .color(c32a(p.text2)),
                    );
                });
            }
            for a in &alerts {
                let c = class_color(&a.class);
                let frame = card_frame(&p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        egui::Frame::new()
                            .fill(c32(c))
                            .inner_margin(Margin::symmetric(8, 3))
                            .show(ui, |ui| {
                                ui.label(caps(
                                    severity_text(a.severity),
                                    10.0,
                                    if ss_core::theme::brightness(c) > 150.0 {
                                        Color32::BLACK
                                    } else {
                                        Color32::WHITE
                                    },
                                ));
                            });
                        ui.label(
                            RichText::new(a.display_event.to_uppercase())
                                .font(semibold_font(15.0))
                                .extra_letter_spacing(1.2)
                                .color(c32(c)),
                        );
                    });
                    if let Some(h) = &a.headline {
                        ui.add(egui::Label::new(RichText::new(h).color(c32a(p.text2))).wrap());
                    }
                    let mut meta = Vec::new();
                    if let Some(area) = &a.area_desc {
                        meta.push(area.clone());
                    }
                    if let Some(e) = &a.expires {
                        meta.push(format!("until {}", fmt_until(e)));
                    }
                    if let Some(o) = &a.issuing_office {
                        meta.push(o.clone());
                    }
                    if !meta.is_empty() {
                        ui.add(egui::Label::new(caps(&meta.join(" · "), 9.5, c32a(p.text3))).wrap());
                    }
                    if a.description.is_some() || a.instruction.is_some() {
                        egui::CollapsingHeader::new(caps("Details & safety info", 10.0, accent2(&p)))
                            .id_salt(("alert", &a.id))
                            .show(ui, |ui| {
                                if let Some(d) = &a.description {
                                    ui.add(egui::Label::new(RichText::new(d.trim()).color(c32(p.text))).wrap());
                                }
                                if let Some(i) = &a.instruction {
                                    ui.add_space(4.0);
                                    ui.label(caps("What to do", 10.0, c32a(p.text3)));
                                    ui.add(egui::Label::new(RichText::new(i.trim()).color(c32(p.text))).wrap());
                                }
                                if let Some(u) = &a.url {
                                    ui.hyperlink_to("Read the full alert", u);
                                }
                            });
                    }
                });
                let r = frame.response.rect;
                ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(3.0, r.height())), 0.0, c32(c));
            }
            ui.label(caps(
                "Alerts: WMO alerting network (official CAP feeds from national weather services) via LibreWXR",
                9.5,
                c32a(p.text3),
            ));
            ui.add_space(12.0);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> chrono::NaiveDateTime {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap()
    }

    #[test]
    fn sun_progress_and_daylight() {
        let (rise, set) = ("2026-08-10T06:00", "2026-08-10T20:00");
        assert_eq!(sun_progress(rise, set, at("2026-08-10T13:00")), Some(0.5));
        assert_eq!(sun_progress(rise, set, at("2026-08-10T05:00")), None);
        assert_eq!(sun_progress(rise, set, at("2026-08-10T21:00")), None);
        assert_eq!(
            daylight(rise, "2026-08-10T16:30").as_deref(),
            Some("10h 30m")
        );
        assert_eq!(daylight("bad", set), None);
    }

    #[test]
    fn cloud_words() {
        assert_eq!(cloud_desc(5.0), "Clear skies");
        assert_eq!(cloud_desc(40.0), "Partly cloudy");
        assert_eq!(cloud_desc(70.0), "Mostly cloudy");
        assert_eq!(cloud_desc(100.0), "Overcast");
    }

    #[test]
    fn bento_rows_tile_without_overlap() {
        for wide in [true, false] {
            let width = if wide { 1000.0 } else { 600.0 };
            for row in grid(7) {
                let rects = slot_rects(&row, 0.0, 0.0, width, wide);
                for (i, (a, _)) in rects.iter().enumerate() {
                    assert!(a.left() >= -0.01 && a.right() <= width + 0.01, "{a:?}");
                    for (b, _) in rects.iter().skip(i + 1) {
                        assert!(
                            !a.shrink(0.5).intersects(b.shrink(0.5)),
                            "{a:?} overlaps {b:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn wide_rows_span_the_full_width() {
        for row in grid(7) {
            let rects = slot_rects(&row, 0.0, 0.0, 900.0, true);
            let right = rects.iter().map(|(r, _)| r.right()).fold(0.0, f32::max);
            assert!((right - 900.0).abs() < 0.5, "row ends at {right}");
        }
    }

    #[test]
    fn temperatures_format_without_unit() {
        assert_eq!(num(20.0, UnitPref::C), "20");
        assert_eq!(unit_label(UnitPref::F), "°F");
    }
}
