//! The places sidebar, the forecast bento grid and the alerts list, drawn
//! in Material 3 Expressive: tonal containers with large rounded corners,
//! pill-shaped items, expressive shapes behind the weather icons, wavy and
//! thick rounded indicators, and rounded Google Sans Flex.

use egui::{
    pos2, vec2, Align, Align2, Color32, Layout, Margin, Painter, Rect, RichText, ScrollArea, Sense,
    Stroke, Ui, UiBuilder,
};
use ss_core::alerts::colors::class_color;
use ss_core::theme::Palette;
use ss_core::weather::units::{
    compass_8, fmt_date_short, fmt_precip, fmt_temp, fmt_time, fmt_time_full, fmt_wind, uv_label,
};
use ss_core::weather::wmo::wmo_label;
use ss_core::{Alert, Severity, TimeFormat, UnitPref, Weather};

use crate::app::{Action, AiState, App, Tab};
use crate::m3::{self, corner, Scheme};
use crate::search::{self, Intent};
use crate::summary;
use crate::theme::{
    body_font, c32, card_frame, display_font, is_dark, label, medium_font, scheme, GAP, RADIUS,
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

fn icon(ui: &Ui, painter: &Painter, code: u8, is_day: bool, p: &Palette, rect: Rect) {
    let tex = ss_assets::weather_texture(ui.ctx(), code, is_day, is_dark(p), rect.height() as u32);
    painter.image(
        tex.id(),
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// Black or white, whichever reads better on `bg`.
fn ink_on(bg: Color32) -> Color32 {
    if ss_core::theme::brightness([bg.r(), bg.g(), bg.b()]) > 150.0 {
        Color32::from_gray(20)
    } else {
        Color32::WHITE
    }
}

/// A Material state layer: `base` washed with `content` at hover/press.
fn state(base: Color32, content: Color32, resp: &egui::Response) -> Color32 {
    if resp.is_pointer_button_down_on() {
        base.lerp_to_gamma(content, 0.14)
    } else if resp.hovered() {
        base.lerp_to_gamma(content, 0.08)
    } else {
        base
    }
}

/// Draw `add` inside a rounded tonal tile filling `rect`.
fn cell(ui: &mut Ui, rect: Rect, fill: Color32, salt: &str, add: impl FnOnce(&mut Ui)) {
    ui.painter().rect_filled(rect, RADIUS, fill);
    let inner = rect.shrink(22.0);
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

/// A small Material chip (label on a full-round container).
fn chip(ui: &mut Ui, text: &str, fill: Color32, ink: Color32) {
    egui::Frame::new()
        .fill(fill)
        .corner_radius(corner::FULL)
        .inner_margin(Margin::symmetric(12, 5))
        .show(ui, |ui| {
            ui.label(label(text, 13.0, ink));
        });
}

/// A circular icon button with a glyph; returns the response.
fn icon_button(
    ui: &mut Ui,
    glyph: &str,
    size: f32,
    fill: Color32,
    ink: Color32,
    tip: &str,
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    ui.painter()
        .circle_filled(r.center(), size / 2.0, state(fill, ink, &resp));
    ui.painter().text(
        r.center(),
        Align2::CENTER_CENTER,
        glyph,
        medium_font(size * 0.42),
        ink,
    );
    resp.on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A Material circular progress ring (indicator + track with gaps).
fn ring(
    painter: &Painter,
    c: egui::Pos2,
    r: f32,
    t: f32,
    width: f32,
    ind: Color32,
    track: Color32,
) {
    use std::f32::consts::{FRAC_PI_2, TAU};
    let t = t.clamp(0.0, 1.0);
    let gap = (width * 1.6) / r; // radians
    let arc = |a0: f32, a1: f32| -> Vec<egui::Pos2> {
        let n = ((a1 - a0).abs() * r / 3.0).ceil().max(2.0) as usize;
        (0..=n)
            .map(|i| m3::polar(c, r, a0 + (a1 - a0) * i as f32 / n as f32))
            .collect()
    };
    let start = -FRAC_PI_2;
    let end = start + TAU * t;
    if t > 0.01 {
        m3::polyline(painter, arc(start, end), width, ind);
    }
    if t < 0.99 {
        let (a0, a1) = (end + gap, start + TAU - gap);
        if a1 > a0 {
            m3::polyline(painter, arc(a0, a1), width, track);
        }
    }
}

/// Material 3 Expressive connected button group: pills joined with small
/// inner corners; the selected one turns fully round and filled. Returns the
/// index clicked.
pub fn connected_group(
    ui: &mut Ui,
    s: &Scheme,
    items: &[(String, bool, Option<Color32>)],
    height: f32,
    min_w: f32,
) -> Option<usize> {
    // Measure first, then take one slot from the parent layout.
    let n = items.len();
    let gap = 2.0;
    let widths = group_widths(ui, items, min_w);
    let total = widths.iter().sum::<f32>() + gap * (n.saturating_sub(1)) as f32;
    let (area, _) = ui.allocate_exact_size(vec2(total, height), Sense::hover());
    connected_group_at(ui, s, items, area, &widths)
}

/// Total width of a connected group.
pub fn connected_group_width(
    ui: &Ui,
    items: &[(String, bool, Option<Color32>)],
    min_w: f32,
) -> f32 {
    group_widths(ui, items, min_w).iter().sum::<f32>()
        + 2.0 * (items.len().saturating_sub(1)) as f32
}

fn group_widths(ui: &Ui, items: &[(String, bool, Option<Color32>)], min_w: f32) -> Vec<f32> {
    items
        .iter()
        .map(|(text, _, _)| {
            let g = ui
                .painter()
                .layout_no_wrap(text.clone(), medium_font(14.0), Color32::WHITE);
            (g.size().x + 40.0).max(min_w)
        })
        .collect()
}

/// Draw a connected group into `area` (already allocated by the caller).
pub fn connected_group_in(
    ui: &mut Ui,
    s: &Scheme,
    items: &[(String, bool, Option<Color32>)],
    area: Rect,
    min_w: f32,
) -> Option<usize> {
    let widths = group_widths(ui, items, min_w);
    connected_group_at(ui, s, items, area, &widths)
}

fn connected_group_at(
    ui: &mut Ui,
    s: &Scheme,
    items: &[(String, bool, Option<Color32>)],
    area: Rect,
    widths: &[f32],
) -> Option<usize> {
    let mut clicked = None;
    let n = items.len();
    let height = area.height();
    let full = (height / 2.0).round() as u8;
    let inner = corner::SMALL;
    let gap = 2.0;
    let font = medium_font(14.0);
    let mut x = area.left();
    for (i, ((text, on, badge), w)) in items.iter().zip(widths).enumerate() {
        let r = Rect::from_min_size(pos2(x, area.top()), vec2(*w, height));
        x += w + gap;
        let resp = ui.interact(r, ui.scope_id().with(("group", i, text)), Sense::click());
        let (l, rr) = if *on {
            (full, full)
        } else {
            (
                if i == 0 { full } else { inner },
                if i + 1 == n { full } else { inner },
            )
        };
        let cr = egui::CornerRadius {
            nw: l,
            sw: l,
            ne: rr,
            se: rr,
        };
        let (base, ink) = if *on {
            (s.primary, s.on_primary)
        } else {
            (s.surface_container_highest, s.on_surface_variant)
        };
        ui.painter().rect_filled(r, cr, state(base, ink, &resp));
        let label_c = match (on, badge) {
            (false, Some(c)) => *c,
            _ => ink,
        };
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            text,
            font.clone(),
            label_c,
        );
        if resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            clicked = Some(i);
        }
    }
    clicked
}

/// A round icon button at an explicit rect.
pub fn round_button_at(
    ui: &mut Ui,
    s: &Scheme,
    rect: Rect,
    glyph: &str,
    tip: &str,
) -> egui::Response {
    let resp = ui.interact(rect, ui.scope_id().with(("round", glyph)), Sense::click());
    let (fill, ink) = (s.surface_container_highest, s.on_surface_variant);
    ui.painter()
        .circle_filled(rect.center(), rect.height() / 2.0, state(fill, ink, &resp));
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        medium_font(rect.height() * 0.42),
        ink,
    );
    resp.on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---------------------------------------------------------------------------
// Places
// ---------------------------------------------------------------------------

pub fn places(ui: &mut Ui, app: &App, actions: &mut Vec<Action>) {
    let p = app.palette;
    let s = scheme(&p);
    ui.add_space(6.0);
    ui.label(label("Places", 15.0, s.on_surface_variant));
    ui.add_space(2.0);
    ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for l in &app.locations.saved_locations {
                let active = app.locations.active_location_id.as_deref() == Some(&l.id);
                let w = app.weather.get(&l.id);
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::click());
                let hovered = resp.hovered();
                let base = if active {
                    s.secondary_container
                } else {
                    s.surface
                };
                let ink = if active {
                    s.on_secondary_container
                } else {
                    s.on_surface
                };
                ui.painter()
                    .rect_filled(rect, corner::FULL, state(base, ink, &resp));
                let painter = ui.painter_at(rect);
                let name = l.label.split(',').next().unwrap_or(&l.label).trim();
                let region = l.label.split_once(',').map(|(_, r)| r.trim()).unwrap_or("");
                // Leading icon in a round container.
                let ic = pos2(rect.left() + 32.0, rect.center().y);
                painter.circle_filled(
                    ic,
                    20.0,
                    if active {
                        s.primary_container
                    } else {
                        s.surface_container_high
                    },
                );
                if let Some(w) = w {
                    icon(
                        ui,
                        &painter,
                        w.current.weather_code,
                        w.current.is_day,
                        &p,
                        Rect::from_center_size(ic, vec2(28.0, 28.0)),
                    );
                } else if app.loading.contains(&l.id) {
                    m3::loading_indicator(
                        &painter,
                        ic,
                        30.0,
                        ui.input(|i| i.time),
                        s.primary,
                        None,
                    );
                }
                painter.text(
                    pos2(rect.left() + 62.0, rect.top() + 21.0),
                    Align2::LEFT_CENTER,
                    name,
                    medium_font(15.0),
                    ink,
                );
                painter.text(
                    pos2(rect.left() + 62.0, rect.top() + 40.0),
                    Align2::LEFT_CENTER,
                    if region.is_empty() { "—" } else { region },
                    body_font(12.5),
                    if active {
                        s.on_secondary_container.gamma_multiply(0.8)
                    } else {
                        s.on_surface_variant
                    },
                );
                if app.alerts.get(&l.id).is_some_and(|a| !a.is_empty()) {
                    painter.circle_filled(ic + vec2(15.0, -15.0), 5.0, s.error);
                }
                let mut removed = false;
                if hovered {
                    let x = Rect::from_center_size(
                        pos2(rect.right() - 26.0, rect.center().y),
                        vec2(32.0, 32.0),
                    );
                    let xr = ui.interact(x, egui::Id::unique(("rm", &l.id)), Sense::click());
                    let fill = if xr.hovered() {
                        s.error_container
                    } else {
                        base.lerp_to_gamma(ink, 0.1)
                    };
                    ui.painter().circle_filled(x.center(), 16.0, fill);
                    ui.painter().text(
                        x.center(),
                        Align2::CENTER_CENTER,
                        "✕",
                        medium_font(13.0),
                        if xr.hovered() {
                            s.on_error_container
                        } else {
                            ink
                        },
                    );
                    if xr.on_hover_text("Remove place").clicked() {
                        actions.push(Action::Remove(l.id.clone()));
                        removed = true;
                    }
                } else if let Some(w) = w {
                    painter.text(
                        pos2(rect.right() - 18.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        format!("{}°", num(w.current.temperature_c, app.config.units)),
                        display_font(22.0),
                        ink,
                    );
                }
                if resp.clicked() && !removed {
                    actions.push(Action::Select(l.id.clone()));
                    actions.push(Action::Tab(Tab::Forecast));
                }
            }
            ui.add_space(10.0);
            // An extended FAB: the primary action of the sidebar.
            let text = if app.locating {
                "Locating…"
            } else {
                "+  Use my location"
            };
            let (r, resp) =
                ui.allocate_exact_size(vec2(ui.available_width().min(220.0), 56.0), Sense::click());
            let fill = state(s.primary_container, s.on_primary_container, &resp);
            ui.painter().rect_filled(
                r.translate(vec2(0.0, 2.0)),
                corner::LARGE,
                Color32::from_black_alpha(if s.dark { 60 } else { 22 }),
            );
            ui.painter().rect_filled(r, corner::LARGE, fill);
            ui.painter().text(
                r.center(),
                Align2::CENTER_CENTER,
                text,
                medium_font(15.0),
                s.on_primary_container,
            );
            if !app.locating
                && resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
            {
                actions.push(Action::Intent(Intent::UseMyLocation));
            }
            if app.locations.saved_locations.is_empty() && !app.locating {
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Search for a city, airport or landmark above to add it here.",
                        )
                        .size(13.0)
                        .color(s.on_surface_variant),
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
    let week_h = 70.0 + 46.0 * daily.clamp(3, 7) as f32;
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
            height: 186.0,
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
            app.locating,
        );
        return;
    };
    let Some(w) = app.weather.get(&loc.id).cloned() else {
        match app.errors.get(&loc.id) {
            Some(e) => empty_state(ui, &p, e, false),
            None => empty_state(ui, &p, &format!("Loading {}…", loc.label), true),
        }
        return;
    };
    let now = App::now_for(&w);
    let alerts = app.active_alerts();
    let scroll_to = app.scroll_to.take();
    let full_w = ui.available_width() - 12.0;
    let wide = full_w >= 860.0;
    let mut hero_rect = Rect::NOTHING;
    let mut answered = false;

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
                    let r = answer_card(
                        ui,
                        &p,
                        &w,
                        &ans,
                        app.config.units,
                        app.config.time_format,
                        now,
                        actions,
                    );
                    // A just-asked question: show its answer, not the hero.
                    if scroll_to == Some(search::View::Forecast) {
                        ui.scroll_to_rect(r, Some(Align::TOP));
                        answered = true;
                    }
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
                        if target.is_some() && target == scroll_to && !answered {
                            ui.scroll_to_rect(cr, Some(Align::TOP));
                        }
                        draw_cell(ui, app, &p, &loc.label, &w, &alerts, now, c, cr, actions);
                    }
                }
            }

            let s = scheme(&p);
            ui.horizontal(|ui| {
                ui.label(label("Silver Skies", 12.5, s.primary));
                ui.label(
                    RichText::new("· Open-Meteo · WMO alerts via LibreWXR · Google weather icons")
                        .size(12.0)
                        .color(s.outline),
                );
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
    let s = scheme(p);
    let units = app.config.units;
    let tf = app.config.time_format;
    let cur = &w.current;
    match c {
        Cell::Hero => cell(ui, rect, s.primary_container, "hero", |ui| {
            hero(ui, p, &s, place, w, units, now, tf)
        }),
        Cell::Summary => cell(ui, rect, s.surface_container, "summary", |ui| {
            summary_tile(ui, app, p, &s, w, alerts, now, actions)
        }),
        Cell::Humidity => stat(
            ui,
            rect,
            &s,
            "Humidity",
            format!("{:.0}", cur.relative_humidity_pct),
            "%",
            None,
            Badge::Ring((cur.relative_humidity_pct / 100.0) as f32),
        ),
        Cell::Wind => {
            let st = fmt_wind(cur.wind_speed_kmh, units);
            let (v, u) = st.split_once(' ').unwrap_or((&st, ""));
            stat(
                ui,
                rect,
                &s,
                "Wind",
                v.to_string(),
                u,
                None,
                Badge::Wave((cur.wind_speed_kmh / 60.0).clamp(0.0, 1.0) as f32),
            )
        }
        Cell::WindDir => stat(
            ui,
            rect,
            &s,
            "Wind direction",
            compass_8(cur.wind_direction_deg).to_string(),
            "",
            Some(format!("{:.0}°", cur.wind_direction_deg)),
            Badge::Compass(cur.wind_direction_deg as f32),
        ),
        Cell::Precip => {
            let st = fmt_precip(cur.precipitation_mm, units);
            let (v, u) = st.split_once(' ').unwrap_or((&st, ""));
            stat(
                ui,
                rect,
                &s,
                "Precipitation",
                v.to_string(),
                u,
                Some("Last hour".into()),
                Badge::Drop((cur.precipitation_mm / 5.0).clamp(0.0, 1.0) as f32),
            )
        }
        Cell::Hourly => cell(ui, rect, s.surface_container, "hourly", |ui| {
            hourly(ui, p, &s, w, units, tf, now)
        }),
        Cell::Week => cell(ui, rect, s.surface_container, "week", |ui| {
            week(ui, app, p, &s, w, units, tf, actions)
        }),
        Cell::Uv => cell(ui, rect, s.surface_container, "uv", |ui| {
            uv(ui, &s, cur.uv_index)
        }),
        Cell::Cloud => cell(ui, rect, s.surface_container, "cloud", |ui| {
            tile_title(ui, &s, "Cloud cover");
            ui.label(
                RichText::new(format!("{:.0}%", cur.cloud_cover_pct))
                    .font(display_font(40.0))
                    .color(s.on_surface),
            );
            ui.label(
                RichText::new(cloud_desc(cur.cloud_cover_pct))
                    .size(15.0)
                    .color(s.on_surface_variant),
            );
            let r = ui.max_rect();
            let c = pos2(r.right() - 30.0, r.top() + 30.0);
            ring(
                ui.painter(),
                c,
                22.0,
                (cur.cloud_cover_pct / 100.0) as f32,
                6.0,
                s.secondary,
                s.surface_container_highest,
            );
        }),
        Cell::Feels => cell(ui, rect, s.tertiary_container, "feels", |ui| {
            tile_title_on(ui, s.on_tertiary_container, "Feels like");
            ui.label(
                RichText::new(temp(cur.apparent_temperature_c, units))
                    .font(display_font(44.0))
                    .color(s.on_tertiary_container),
            );
            ui.label(
                RichText::new("Apparent temperature outside")
                    .size(15.0)
                    .color(s.on_tertiary_container.gamma_multiply(0.85)),
            );
            let diff = cur.apparent_temperature_c - cur.temperature_c;
            if diff.abs() >= 1.0 {
                ui.add_space(8.0);
                chip(
                    ui,
                    &format!(
                        "{} than actual",
                        if diff > 0.0 { "Warmer" } else { "Colder" }
                    ),
                    s.tertiary,
                    if s.dark {
                        s.tertiary_container
                    } else {
                        Color32::WHITE
                    },
                );
            }
            // A soft flower in the corner.
            let r = ui.max_rect().expand(22.0);
            let painter = ui.painter_at(r);
            m3::cookie(
                &painter,
                pos2(r.right() - 60.0, r.bottom() - 58.0),
                38.0,
                8,
                0.2,
                m3::with_alpha(s.tertiary, 40),
            );
        }),
        Cell::Sun => cell(ui, rect, s.surface_container, "sun", |ui| {
            sun(ui, &s, w, tf, now)
        }),
    }
}

fn tile_title(ui: &mut Ui, s: &Scheme, text: &str) {
    tile_title_on(ui, s.on_surface_variant, text);
}

fn tile_title_on(ui: &mut Ui, color: Color32, text: &str) {
    ui.label(label(text, 14.0, color));
}

#[allow(clippy::too_many_arguments)]
fn hero(
    ui: &mut Ui,
    p: &Palette,
    s: &Scheme,
    place: &str,
    w: &Weather,
    units: UnitPref,
    now: chrono::NaiveDateTime,
    tf: TimeFormat,
) {
    let rect = ui.max_rect();
    let cur = &w.current;
    let ink = s.on_primary_container;
    let soft = ink.gamma_multiply(0.78);

    // The weather icon on layered expressive shapes, right-hand side.
    let size = (rect.height() * 0.66).clamp(96.0, 176.0);
    let c = pos2(rect.right() - size / 2.0 - 6.0, rect.center().y + 4.0);
    let painter = ui.painter_at(rect.expand(22.0));
    m3::sunny(
        &painter,
        c + vec2(-size * 0.28, -size * 0.22),
        size * 0.36,
        0.3,
        m3::with_alpha(s.tertiary, if s.dark { 150 } else { 120 }),
    );
    m3::cookie(&painter, c, size * 0.58, 9, 0.0, s.surface_container_low);
    icon(
        ui,
        &painter,
        cur.weather_code,
        cur.is_day,
        p,
        Rect::from_center_size(c, vec2(size * 0.82, size * 0.82)),
    );

    let text_w = (rect.width() - size - 24.0).max(120.0);
    ui.allocate_ui_with_layout(
        vec2(text_w, rect.height()),
        Layout::top_down(Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                let clock = match tf {
                    TimeFormat::H12 => "%-I:%M %p",
                    TimeFormat::H24 => "%H:%M",
                };
                chip(
                    ui,
                    &format!("Now · {}", now.format(clock)),
                    s.primary,
                    s.on_primary,
                );
            });
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(RichText::new(place).font(medium_font(20.0)).color(ink))
                    .truncate(),
            );
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.label(
                    RichText::new(num(cur.temperature_c, units))
                        .font(display_font(104.0))
                        .color(ink),
                );
                ui.vertical(|ui| {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new(unit_label(units))
                            .font(display_font(36.0))
                            .color(soft),
                    );
                });
            });
            ui.label(
                RichText::new(wmo_label(cur.weather_code))
                    .font(medium_font(22.0))
                    .color(ink),
            );
            if let Some(d) = w.daily.first() {
                ui.label(
                    RichText::new(format!(
                        "Feels like {}  ·  High {}  ·  Low {}",
                        temp(cur.apparent_temperature_c, units),
                        temp(d.temp_max_c, units),
                        temp(d.temp_min_c, units)
                    ))
                    .size(14.0)
                    .color(soft),
                );
            }
        },
    );
}

/// The small visual in a stat tile's corner.
enum Badge {
    /// Circular progress, 0..=1.
    Ring(f32),
    /// Wavy line whose amplitude follows the value, 0..=1.
    Wave(f32),
    /// Compass needle on a burst shape, degrees (the direction the wind is from).
    Compass(f32),
    /// Fill level in a soft pill, 0..=1.
    Drop(f32),
}

#[allow(clippy::too_many_arguments)]
fn stat(
    ui: &mut Ui,
    rect: Rect,
    s: &Scheme,
    title: &str,
    value: String,
    unit: &str,
    sub: Option<String>,
    badge: Badge,
) {
    let salt = title.to_string();
    cell(ui, rect, s.surface_container, &salt, |ui| {
        let r = ui.max_rect();
        let painter = ui.painter_at(r.expand(16.0));
        let bc = pos2(r.right() - 22.0, r.top() + 22.0);
        match badge {
            Badge::Ring(t) => ring(
                &painter,
                bc,
                20.0,
                t,
                6.0,
                s.primary,
                s.surface_container_highest,
            ),
            Badge::Wave(t) => {
                let a = pos2(r.right() - 64.0, bc.y);
                let b = pos2(r.right() + 2.0, bc.y);
                let pts = m3::wavy(&[a, b], 1.5 + 4.0 * t, 16.0);
                m3::polyline(&painter, pts, 5.0, s.primary);
            }
            Badge::Compass(deg) => {
                m3::sunny(&painter, bc, 26.0, 0.0, s.secondary_container);
                // Arrow points where the wind blows to.
                let a = (deg + 180.0 - 90.0).to_radians();
                let tip = m3::polar(bc, 15.0, a);
                let tail = m3::polar(bc, -11.0, a);
                m3::rounded_line(&painter, tail, tip, 4.0, s.on_secondary_container);
                let l = m3::polar(tip, -7.0, a + 0.6);
                let rr = m3::polar(tip, -7.0, a - 0.6);
                m3::rounded_line(&painter, tip, l, 4.0, s.on_secondary_container);
                m3::rounded_line(&painter, tip, rr, 4.0, s.on_secondary_container);
            }
            Badge::Drop(t) => {
                let pill = Rect::from_center_size(bc, vec2(16.0, 42.0));
                painter.rect_filled(pill, corner::FULL, s.surface_container_highest);
                let h = (pill.height() * t).max(if t > 0.0 { 16.0 } else { 0.0 });
                if h > 0.0 {
                    let f = Rect::from_min_max(pos2(pill.left(), pill.bottom() - h), pill.max);
                    painter.rect_filled(f, corner::FULL, s.primary);
                }
            }
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        tile_title(ui, s, title);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(
                RichText::new(value)
                    .font(display_font(36.0))
                    .color(s.on_surface),
            );
            if !unit.is_empty() {
                ui.vertical(|ui| {
                    ui.add_space(17.0);
                    ui.label(RichText::new(unit).size(14.0).color(s.on_surface_variant));
                });
            }
        });
        if let Some(sub) = sub {
            ui.label(RichText::new(sub).size(13.0).color(s.on_surface_variant));
        }
    });
}

fn hourly(
    ui: &mut Ui,
    p: &Palette,
    s: &Scheme,
    w: &Weather,
    units: UnitPref,
    tf: TimeFormat,
    now: chrono::NaiveDateTime,
) {
    tile_title(ui, s, "Hourly forecast");
    ui.add_space(4.0);
    ScrollArea::horizontal()
        .id_salt("hourly")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let i = summary::current_hour_index(&w.hourly, now);
                for (k, h) in w.hourly.iter().skip(i).take(24).enumerate() {
                    let (r, _) = ui.allocate_exact_size(vec2(66.0, 118.0), Sense::hover());
                    let is_now = k == 0;
                    let painter = ui.painter_at(r);
                    let (fill, ink, sub) = if is_now {
                        (s.primary, s.on_primary, s.on_primary)
                    } else {
                        (s.surface_container_high, s.on_surface, s.on_surface_variant)
                    };
                    painter.rect_filled(r, corner::FULL, fill);
                    let t = if is_now {
                        "Now".to_string()
                    } else {
                        fmt_time(&h.time, tf)
                    };
                    painter.text(
                        pos2(r.center().x, r.top() + 20.0),
                        Align2::CENTER_CENTER,
                        t,
                        medium_font(12.5),
                        sub,
                    );
                    let ic = pos2(r.center().x, r.top() + 50.0);
                    if is_now {
                        painter.circle_filled(ic, 20.0, s.surface_container_low);
                    }
                    icon(
                        ui,
                        &painter,
                        h.weather_code,
                        h.is_day,
                        p,
                        Rect::from_center_size(ic, vec2(30.0, 30.0)),
                    );
                    painter.text(
                        pos2(r.center().x, r.top() + 82.0),
                        Align2::CENTER_CENTER,
                        format!("{}°", num(h.temperature_c, units)),
                        medium_font(17.0),
                        ink,
                    );
                    let pop = h.precipitation_probability_pct.round();
                    if pop >= 10.0 {
                        painter.text(
                            pos2(r.center().x, r.top() + 102.0),
                            Align2::CENTER_CENTER,
                            format!("{pop}%"),
                            medium_font(11.5),
                            if is_now { s.on_primary } else { s.primary },
                        );
                    }
                }
            });
        });
}

/// Cold-to-warm colour for the range bars.
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

/// A thick rounded bar from `x0` to `x1` with a horizontal gradient.
fn gradient_pill(painter: &Painter, x0: f32, x1: f32, y: f32, h: f32, c0: Color32, c1: Color32) {
    let r = h / 2.0;
    painter.circle_filled(pos2(x0, y), r, c0);
    painter.circle_filled(pos2(x1, y), r, c1);
    let mut mesh = egui::Mesh::default();
    for (xx, c) in [(x0, c0), (x1, c1)] {
        mesh.colored_vertex(pos2(xx, y - r), c);
        mesh.colored_vertex(pos2(xx, y + r), c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
}

#[allow(clippy::too_many_arguments)]
fn week(
    ui: &mut Ui,
    app: &App,
    p: &Palette,
    s: &Scheme,
    w: &Weather,
    units: UnitPref,
    tf: TimeFormat,
    actions: &mut Vec<Action>,
) {
    let n = w.daily.len().min(7);
    tile_title(ui, s, &format!("{n}-day forecast"));
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
        let (base, ink) = if focused {
            (s.primary_container, s.on_primary_container)
        } else if today {
            (s.secondary_container, s.on_secondary_container)
        } else {
            (s.surface_container_high, s.on_surface)
        };
        ui.painter()
            .rect_filled(rect, corner::FULL, state(base, ink, &resp));
        if resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
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
            pos2(rect.left() + 18.0, y),
            Align2::LEFT_CENTER,
            name,
            medium_font(15.0),
            ink,
        );
        icon(
            ui,
            &painter,
            d.weather_code,
            true,
            p,
            Rect::from_center_size(pos2(rect.left() + 98.0, y), vec2(28.0, 28.0)),
        );
        let pop = d.precipitation_probability_max_pct.round();
        painter.text(
            pos2(rect.right() - 18.0, y),
            Align2::RIGHT_CENTER,
            format!("{pop}%"),
            medium_font(12.5),
            if focused || today { ink } else { s.primary },
        );
        // low  ━━━━  high
        let bar_l = rect.left() + 172.0;
        let bar_r = rect.right() - 118.0;
        painter.text(
            pos2(bar_l - 12.0, y),
            Align2::RIGHT_CENTER,
            format!("{}°", num(d.temp_min_c, units)),
            body_font(14.0),
            ink.gamma_multiply(0.75),
        );
        painter.text(
            pos2(bar_r + 12.0, y),
            Align2::LEFT_CENTER,
            format!("{}°", num(d.temp_max_c, units)),
            medium_font(15.0),
            ink,
        );
        if bar_r - bar_l > 40.0 && hi > lo {
            let x = |t: f64| bar_l + ((t - lo) / (hi - lo)) as f32 * (bar_r - bar_l);
            m3::rounded_line(
                &painter,
                pos2(bar_l, y),
                pos2(bar_r, y),
                6.0,
                ink.gamma_multiply(0.14),
            );
            let (x0, x1) = (x(d.temp_min_c), x(d.temp_max_c).max(x(d.temp_min_c) + 1.0));
            let t0 = ((d.temp_min_c - lo) / (hi - lo)) as f32;
            let t1 = ((d.temp_max_c - lo) / (hi - lo)) as f32;
            gradient_pill(&painter, x0, x1, y, 6.0, temp_color(t0), temp_color(t1));
        }
    }
    if let Some(i) = app.focus_day {
        if let Some(d) = w.daily.get(i) {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                chip(ui, &summary::day_name(&w.daily, i), s.primary, s.on_primary);
                ui.label(
                    RichText::new(format!(
                        "{} · sunrise {} · sunset {} · UV {:.0}",
                        wmo_label(d.weather_code),
                        fmt_time_full(&d.sunrise, tf),
                        fmt_time_full(&d.sunset, tf),
                        d.uv_index_max
                    ))
                    .size(13.5)
                    .color(s.on_surface_variant),
                );
            });
        }
    }
}

fn uv(ui: &mut Ui, s: &Scheme, uv: f64) {
    let (uv_text, uv_rgb) = uv_label(uv);
    tile_title(ui, s, "UV index");
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{uv:.0}"))
                .font(display_font(40.0))
                .color(s.on_surface),
        );
        ui.vertical(|ui| {
            ui.add_space(14.0);
            chip(ui, uv_text, c32(uv_rgb), ink_on(c32(uv_rgb)));
        });
    });
    ui.label(
        RichText::new(if uv > 5.0 {
            "Sun protection recommended"
        } else {
            "Low risk for most people"
        })
        .size(14.0)
        .color(s.on_surface_variant),
    );
    ui.add_space(6.0);
    // An expressive slider-style scale: thick rounded track with a pill handle.
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    let y = r.center().y;
    let (l, rr) = (r.left() + 5.0, r.right() - 5.0);
    let stops: [[u8; 3]; 5] = [
        [0x5d, 0xd8, 0x9c],
        [0xf2, 0xd0, 0x3c],
        [0xff, 0x9a, 0x2a],
        [0xff, 0x5f, 0x55],
        [0xe8, 0x4a, 0xc0],
    ];
    let mx = l + (rr - l) * (uv / 11.0).clamp(0.0, 1.0) as f32;
    let painter = ui.painter();
    for i in 0..stops.len() - 1 {
        let x0 = l + (rr - l) * i as f32 / 4.0;
        let x1 = l + (rr - l) * (i + 1) as f32 / 4.0;
        gradient_pill(painter, x0, x1, y, 10.0, c32(stops[i]), c32(stops[i + 1]));
    }
    // Gap around the handle, as in Material's expressive slider.
    painter.rect_filled(
        Rect::from_center_size(pos2(mx, y), vec2(12.0, 14.0)),
        0.0,
        s.surface_container,
    );
    painter.rect_filled(
        Rect::from_center_size(pos2(mx, y), vec2(4.0, 26.0)),
        corner::FULL,
        s.on_surface,
    );
}

fn sun(ui: &mut Ui, s: &Scheme, w: &Weather, tf: TimeFormat, now: chrono::NaiveDateTime) {
    tile_title(ui, s, "Sun");
    let Some(d) = w.daily.first() else { return };
    let area = ui.available_rect_before_wrap();
    let side_w = 200.0_f32.min(area.width() * 0.4);
    let arc_rect = Rect::from_min_max(area.min, pos2(area.right() - side_w - 10.0, area.bottom()));
    let painter = ui.painter_at(area.expand(12.0));

    let base_y = arc_rect.bottom() - 24.0;
    let radius = (arc_rect.width() / 2.0 - 18.0)
        .min(base_y - arc_rect.top() - 14.0)
        .max(10.0);
    let centre = pos2(arc_rect.center().x, base_y);
    let pt = |t: f64| {
        let a = std::f64::consts::PI * (1.0 - t);
        centre + vec2((a.cos() as f32) * radius, -(a.sin() as f32) * radius)
    };
    // Horizon.
    painter.line_segment(
        [
            pos2(arc_rect.left() + 4.0, base_y),
            pos2(arc_rect.right() - 4.0, base_y),
        ],
        Stroke::new(2.0, s.outline_variant),
    );
    let arc_pts = |t0: f64, t1: f64| -> Vec<egui::Pos2> {
        (0..=90)
            .map(|i| pt(t0 + (t1 - t0) * f64::from(i) / 90.0))
            .collect()
    };
    match summary_sun(d, now) {
        Some(t) => {
            // Material's wavy progress for the part of the day gone by,
            // then a flat track for the rest.
            if t > 0.02 {
                m3::polyline(
                    &painter,
                    m3::wavy(&arc_pts(0.0, t), 3.0, 22.0),
                    5.0,
                    s.primary,
                );
            }
            if t < 0.97 {
                m3::polyline(
                    &painter,
                    arc_pts((t + 0.035).min(1.0), 1.0),
                    5.0,
                    s.surface_container_highest,
                );
            }
            let dot = pt(t);
            m3::sunny(&painter, dot, 17.0, 0.0, s.tertiary);
        }
        None => {
            m3::polyline(
                &painter,
                arc_pts(0.0, 1.0),
                5.0,
                s.surface_container_highest,
            );
            painter.circle_filled(pt(0.0), 8.0, s.outline);
        }
    }
    for (x, align, t) in [
        (arc_rect.left() + 4.0, Align2::LEFT_TOP, &d.sunrise),
        (arc_rect.right() - 4.0, Align2::RIGHT_TOP, &d.sunset),
    ] {
        painter.text(
            pos2(x, base_y + 7.0),
            align,
            fmt_time_full(t, tf),
            body_font(12.5),
            s.on_surface_variant,
        );
    }

    let side = Rect::from_min_max(pos2(area.right() - side_w, area.top()), area.max);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(side)
            .layout(Layout::top_down(Align::Min)),
    );
    child.spacing_mut().item_spacing.y = 0.0;
    for (name, t) in [("Sunrise", &d.sunrise), ("Sunset", &d.sunset)] {
        child.label(label(name, 13.0, s.on_surface_variant));
        child.label(
            RichText::new(fmt_time_full(t, tf))
                .font(display_font(28.0))
                .color(s.on_surface),
        );
        child.add_space(10.0);
    }
    if let Some(dl) = daylight(&d.sunrise, &d.sunset) {
        chip(
            &mut child,
            &format!("{dl} of daylight"),
            s.secondary_container,
            s.on_secondary_container,
        );
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
    let s = scheme(p);
    let bar = Rect::from_min_size(pos2(left + 8.0, top + 8.0), vec2(width - 16.0, 60.0));
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::unique("condensed_bar"),
    ));
    painter.rect_filled(
        bar.translate(vec2(0.0, 3.0)).expand(1.0),
        corner::FULL,
        Color32::from_black_alpha(if s.dark { 90 } else { 30 }),
    );
    painter.rect_filled(bar, corner::FULL, s.primary_container);
    let ic = pos2(bar.left() + 32.0, bar.center().y);
    painter.circle_filled(ic, 22.0, s.surface_container_low);
    icon(
        ui,
        &painter,
        w.current.weather_code,
        w.current.is_day,
        p,
        Rect::from_center_size(ic, vec2(32.0, 32.0)),
    );
    let ink = s.on_primary_container;
    let g = painter.layout_no_wrap(
        place.split(',').next().unwrap_or(place).to_string(),
        medium_font(17.0),
        ink,
    );
    let tw = g.size().x;
    painter.galley(
        pos2(bar.left() + 66.0, bar.center().y - g.size().y / 2.0),
        g,
        ink,
    );
    let t = painter.layout_no_wrap(
        temp(w.current.temperature_c, units),
        display_font(26.0),
        ink,
    );
    let ttw = t.size().x;
    painter.galley(
        pos2(bar.left() + 80.0 + tw, bar.center().y - t.size().y / 2.0),
        t,
        ink,
    );
    painter.text(
        pos2(bar.left() + 94.0 + tw + ttw, bar.center().y),
        Align2::LEFT_CENTER,
        wmo_label(w.current.weather_code),
        body_font(15.5),
        ink.gamma_multiply(0.8),
    );
}

fn alert_banner(ui: &mut Ui, p: &Palette, alerts: &[Alert], actions: &mut Vec<Action>) {
    let s = scheme(p);
    let a = &alerts[0];
    let c = c32(class_color(&a.class));
    let mut fill = s.error_container;
    if matches!(a.severity, Severity::Extreme | Severity::Severe) {
        let t = ui.input(|i| i.time);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(40));
        fill = fill.lerp_to_gamma(c, (0.12 + 0.1 * (t * 2.5).sin()) as f32);
    }
    let r = egui::Frame::new()
        .fill(fill)
        .corner_radius(corner::LARGE_INCREASED)
        .inner_margin(Margin {
            left: 12,
            right: 20,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (b, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                m3::cookie(ui.painter(), b.center(), 20.0, 4, 0.4, c);
                ui.painter().text(
                    b.center(),
                    Align2::CENTER_CENTER,
                    "!",
                    medium_font(20.0),
                    ink_on(c),
                );
                ui.add_space(4.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(label("View alerts  →", 14.0, s.on_error_container));
                    if alerts.len() > 1 {
                        ui.add_space(6.0);
                        chip(
                            ui,
                            &format!("+{} more", alerts.len() - 1),
                            s.on_error_container,
                            s.error_container,
                        );
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&a.display_event)
                                        .font(medium_font(16.0))
                                        .color(s.on_error_container),
                                )
                                .truncate(),
                            );
                            ui.label(
                                RichText::new(format!("{} alert", severity_text(a.severity)))
                                    .size(12.5)
                                    .color(s.on_error_container.gamma_multiply(0.8)),
                            );
                        });
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

fn empty_state(ui: &mut Ui, p: &Palette, text: &str, loading: bool) {
    let s = scheme(p);
    ui.add_space(90.0);
    ui.vertical_centered(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(96.0, 96.0), Sense::hover());
        if loading {
            m3::loading_indicator(
                ui.painter(),
                r.center(),
                72.0,
                ui.input(|i| i.time),
                s.primary,
                Some(s.primary_container),
            );
        } else {
            m3::cookie(
                ui.painter(),
                r.center(),
                44.0,
                9,
                0.0,
                s.secondary_container,
            );
            m3::sunny(
                ui.painter(),
                r.center(),
                18.0,
                0.0,
                s.on_secondary_container,
            );
        }
        ui.add_space(10.0);
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .font(medium_font(20.0))
                    .color(s.on_surface_variant),
            )
            .wrap(),
        );
    });
}

fn ai_block(ui: &mut Ui, s: &Scheme, state: &AiState) {
    match state {
        AiState::Idle => {}
        AiState::Waiting | AiState::Loading(_) => {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                m3::loading_indicator(
                    ui.painter(),
                    r.center(),
                    26.0,
                    ui.input(|i| i.time),
                    s.tertiary,
                    None,
                );
                ui.label(label("Asking the AI…", 13.0, s.on_surface_variant));
            });
        }
        AiState::Done(t) => {
            egui::Frame::new()
                .fill(s.tertiary_container)
                .corner_radius(corner::LARGE_INCREASED)
                .inner_margin(Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.label(label("✦ AI summary", 13.0, s.on_tertiary_container));
                    ui.add(
                        egui::Label::new(
                            RichText::new(t).size(14.5).color(s.on_tertiary_container),
                        )
                        .wrap(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "AI can make mistakes. Check official forecasts and warnings.",
                            )
                            .size(12.0)
                            .color(s.on_tertiary_container.gamma_multiply(0.75)),
                        )
                        .wrap(),
                    );
                });
        }
        AiState::Failed(e) => {
            ui.add(egui::Label::new(RichText::new(e).color(s.error)).wrap());
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
) -> Rect {
    let s = scheme(p);
    card_frame(p)
        .fill(s.secondary_container)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(label(
                    "You asked",
                    13.0,
                    s.on_secondary_container.gamma_multiply(0.8),
                ));
                ui.label(
                    RichText::new(format!("“{}”", ans.question))
                        .size(14.5)
                        .color(s.on_secondary_container),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(
                        ui,
                        "✕",
                        32.0,
                        s.secondary_container
                            .lerp_to_gamma(s.on_secondary_container, 0.1),
                        s.on_secondary_container,
                        "Dismiss",
                    )
                    .clicked()
                    {
                        actions.push(Action::DismissAnswer);
                    }
                });
            });
            let local = summary::answer(w, ans.topic, ans.day, units, tf, now);
            ui.add(
                egui::Label::new(
                    RichText::new(local)
                        .font(medium_font(21.0))
                        .color(s.on_secondary_container),
                )
                .wrap(),
            );
            ai_block(ui, &s, &ans.ai);
        })
        .response
        .rect
}

#[allow(clippy::too_many_arguments)]
fn summary_tile(
    ui: &mut Ui,
    app: &App,
    _p: &Palette,
    s: &Scheme,
    w: &Weather,
    alerts: &[Alert],
    now: chrono::NaiveDateTime,
    actions: &mut Vec<Action>,
) {
    let sm = summary::summarize(w, alerts, app.config.units, app.config.time_format, now);
    ui.horizontal(|ui| {
        tile_title(ui, s, "Summary");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let id = app.active_id().unwrap_or_default();
            let state = app.ai_summary.get(&id);
            let busy = matches!(state, Some(AiState::Loading(_) | AiState::Waiting));
            let (text, action) = if app.ai_ready() {
                (
                    if matches!(state, Some(AiState::Done(_))) {
                        "✦ Redo"
                    } else {
                        "✦ Summarize"
                    },
                    Action::AiSummary,
                )
            } else {
                ("✦ Turn on AI", Action::OpenSettings)
            };
            let btn = egui::Button::new(label(text, 13.0, s.on_tertiary_container))
                .fill(s.tertiary_container)
                .min_size(vec2(0.0, 32.0));
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
                    RichText::new(&sm.headline)
                        .font(medium_font(18.0))
                        .color(s.on_surface),
                )
                .wrap(),
            );
            ui.add_space(2.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            for pt in &sm.points {
                ui.horizontal_wrapped(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 16.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 3.5, s.primary);
                    ui.add(
                        egui::Label::new(RichText::new(pt).size(14.0).color(s.on_surface_variant))
                            .wrap(),
                    );
                });
            }
            if let Some(state) = app.active_id().and_then(|id| app.ai_summary.get(&id)) {
                ui.add_space(6.0);
                ai_block(ui, s, state);
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
    let s = scheme(&p);
    let Some(loc) = app.active() else {
        empty_state(ui, &p, "Pick a place to see its alerts.", false);
        return;
    };
    let alerts = app.active_alerts();
    let full_w = ui.available_width() - 12.0;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(full_w);
            ui.set_max_width(full_w);
            ui.spacing_mut().item_spacing.y = GAP;
            ui.label(label(
                &format!(
                    "{} active alert{}",
                    alerts.len(),
                    if alerts.len() == 1 { "" } else { "s" }
                ),
                15.0,
                s.on_surface_variant,
            ));
            if alerts.is_empty() {
                card_frame(&p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
                        m3::cookie(ui.painter(), r.center(), 30.0, 9, 0.0, s.secondary_container);
                        ui.painter().text(
                            r.center(),
                            Align2::CENTER_CENTER,
                            "✓",
                            medium_font(24.0),
                            s.on_secondary_container,
                        );
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("No active alerts")
                                    .font(medium_font(22.0))
                                    .color(s.on_surface),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "There are no official warnings, watches or advisories for {} right now.",
                                        loc.label
                                    ))
                                    .size(14.5)
                                    .color(s.on_surface_variant),
                                )
                                .wrap(),
                            );
                        });
                    });
                });
            }
            for a in &alerts {
                let c = c32(class_color(&a.class));
                card_frame(&p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                        m3::cookie(ui.painter(), r.center(), 24.0, 4, 0.4, c);
                        ui.painter().text(
                            r.center(),
                            Align2::CENTER_CENTER,
                            "!",
                            medium_font(22.0),
                            ink_on(c),
                        );
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            ui.label(
                                RichText::new(&a.display_event)
                                    .font(medium_font(19.0))
                                    .color(s.on_surface),
                            );
                            ui.horizontal(|ui| {
                                chip(ui, severity_text(a.severity), c, ink_on(c));
                                if let Some(e) = &a.expires {
                                    chip(
                                        ui,
                                        &format!("Until {}", fmt_until(e)),
                                        s.surface_container_highest,
                                        s.on_surface_variant,
                                    );
                                }
                            });
                        });
                    });
                    if let Some(h) = &a.headline {
                        ui.add(
                            egui::Label::new(RichText::new(h).size(14.5).color(s.on_surface))
                                .wrap(),
                        );
                    }
                    let mut meta = Vec::new();
                    if let Some(area) = &a.area_desc {
                        meta.push(area.clone());
                    }
                    if let Some(o) = &a.issuing_office {
                        meta.push(o.clone());
                    }
                    if !meta.is_empty() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(meta.join(" · "))
                                    .size(12.5)
                                    .color(s.on_surface_variant),
                            )
                            .wrap(),
                        );
                    }
                    if a.description.is_some() || a.instruction.is_some() {
                        egui::CollapsingHeader::new(label("Details & safety info", 14.0, s.primary))
                            .id_salt(("alert", &a.id))
                            .show(ui, |ui| {
                                if let Some(d) = &a.description {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(d.trim()).color(s.on_surface),
                                        )
                                        .wrap(),
                                    );
                                }
                                if let Some(i) = &a.instruction {
                                    ui.add_space(4.0);
                                    ui.label(label("What to do", 14.0, s.on_surface_variant));
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(i.trim()).color(s.on_surface),
                                        )
                                        .wrap(),
                                    );
                                }
                                if let Some(u) = &a.url {
                                    ui.hyperlink_to("Read the full alert", u);
                                }
                            });
                    }
                });
            }
            ui.label(
                RichText::new(
                    "Alerts: WMO alerting network (official CAP feeds from national weather services) via LibreWXR",
                )
                .size(12.0)
                .color(s.outline),
            );
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
