//! The places sidebar, the forecast dashboard and the alerts list.

use egui::{Align, CornerRadius, Layout, Margin, RichText, ScrollArea, Sense, Ui};
use ss_assets::weather_image;
use ss_core::alerts::colors::class_color;
use ss_core::theme::Palette;
use ss_core::weather::icons::pixel_icon;
use ss_core::weather::units::{
    compass_8, fmt_date_short, fmt_precip, fmt_temp, fmt_time, fmt_time_full, fmt_wind, uv_label,
};
use ss_core::weather::wmo::wmo_label;
use ss_core::{Alert, Severity, UnitPref, Weather};

use crate::app::{Action, AiState, App, Tab};
use crate::search::{self, Intent};
use crate::summary;
use crate::theme::{alpha, c32, c32a, card_frame, RADIUS, RADIUS_SMALL};

/// "72°" without the unit letter, for big numbers.
fn deg(c: f64, u: UnitPref) -> String {
    fmt_temp(c, u).trim_end_matches(['F', 'C']).to_string()
}

fn section_title(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(text).size(13.0).strong().color(c32a(p.text2)));
}

// ---------------------------------------------------------------------------
// Places
// ---------------------------------------------------------------------------

pub fn places(ui: &mut Ui, app: &App, actions: &mut Vec<Action>) {
    let p = app.palette;
    section_title(ui, &p, "Places");
    ui.add_space(2.0);
    ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for l in &app.locations.saved_locations {
                let active = app.locations.active_location_id.as_deref() == Some(&l.id);
                let w = app.weather.get(&l.id);
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 52.0), Sense::click());
                let hovered = resp.hovered();
                let fill = if active {
                    alpha(p.a0, 70)
                } else if hovered {
                    alpha(p.a0, 28)
                } else {
                    egui::Color32::TRANSPARENT
                };
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(RADIUS_SMALL + 3), fill);
                let painter = ui.painter_at(rect);
                let name = l.label.split(',').next().unwrap_or(&l.label).trim();
                let region = l.label.split_once(',').map(|(_, r)| r.trim()).unwrap_or("");
                painter.text(
                    egui::pos2(rect.left() + 46.0, rect.top() + 17.0),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::proportional(14.5),
                    c32(p.text),
                );
                painter.text(
                    egui::pos2(rect.left() + 46.0, rect.top() + 35.0),
                    egui::Align2::LEFT_CENTER,
                    if region.is_empty() { "—" } else { region },
                    egui::FontId::proportional(11.5),
                    c32a(p.text3),
                );
                if let Some(w) = w {
                    let tex = ss_assets::weather_texture(
                        ui.ctx(),
                        pixel_icon(w.current.weather_code, w.current.is_day),
                        &p,
                        28,
                    );
                    painter.image(
                        tex.id(),
                        egui::Rect::from_center_size(
                            egui::pos2(rect.left() + 24.0, rect.center().y),
                            egui::vec2(28.0, 28.0),
                        ),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    if !hovered {
                        painter.text(
                            egui::pos2(rect.right() - 12.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            deg(w.current.temperature_c, app.config.units),
                            egui::FontId::proportional(18.0),
                            c32(p.text),
                        );
                    }
                } else if app.loading.contains(&l.id) {
                    painter.text(
                        egui::pos2(rect.left() + 24.0, rect.center().y),
                        egui::Align2::CENTER_CENTER,
                        "…",
                        egui::FontId::proportional(16.0),
                        c32a(p.text3),
                    );
                }
                let n_alerts = app.alerts.get(&l.id).map_or(0, Vec::len);
                if n_alerts > 0 && !hovered {
                    painter.circle_filled(
                        egui::pos2(rect.left() + 36.0, rect.top() + 12.0),
                        4.0,
                        c32(p.danger),
                    );
                }
                let mut removed = false;
                if hovered {
                    let x = egui::Rect::from_center_size(
                        egui::pos2(rect.right() - 20.0, rect.center().y),
                        egui::vec2(26.0, 26.0),
                    );
                    let xr = ui.interact(x, egui::Id::unique(("rm", &l.id)), Sense::click());
                    ui.painter().rect_filled(
                        x,
                        CornerRadius::same(8),
                        if xr.hovered() {
                            alpha(p.danger, 70)
                        } else {
                            alpha(p.text, 18)
                        },
                    );
                    ui.painter().text(
                        x.center(),
                        egui::Align2::CENTER_CENTER,
                        "✕",
                        egui::FontId::proportional(12.0),
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
            ui.add_space(6.0);
            let label = if app.locating {
                "Locating…"
            } else {
                "+  Use my location"
            };
            if ui
                .add_enabled(
                    !app.locating,
                    egui::Button::new(RichText::new(label).color(c32a(p.text2)))
                        .fill(egui::Color32::TRANSPARENT),
                )
                .clicked()
            {
                actions.push(Action::Intent(Intent::UseMyLocation));
            }
            if app.locations.saved_locations.is_empty() && !app.locating {
                ui.label(
                    RichText::new("Search for a city, airport or landmark above to add it here.")
                        .size(12.5)
                        .color(c32a(p.text3)),
                );
            }
        });
}

// ---------------------------------------------------------------------------
// Forecast
// ---------------------------------------------------------------------------

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
            Some(e) => empty_state(ui, &p, &format!("Couldn't load the forecast: {e}")),
            None => {
                ui.add_space(60.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Spinner::new().size(28.0).color(c32(p.a0)));
                    ui.label(RichText::new(format!("Loading {}…", loc.label)).color(c32a(p.text2)));
                });
            }
        }
        return;
    };
    let units = app.config.units;
    let tf = app.config.time_format;
    let now = App::now_for(&w);
    let alerts = app.active_alerts();
    let scroll_to = app.scroll_to.take();
    let full_w = ui.available_width() - 14.0;
    let mut hero_rect = egui::Rect::NOTHING;

    let out = ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(full_w);
            ui.set_max_width(full_w);
            ui.spacing_mut().item_spacing.y = 14.0;

            // --- alert banner ---
            if let Some(a) = alerts.first() {
                let c = class_color(&a.class);
                // Severe and extreme alerts gently pulse.
                let fill_a = if matches!(a.severity, Severity::Extreme | Severity::Severe) {
                    let t = ui.input(|i| i.time);
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
                    (58.0 + 30.0 * (t * 2.5).sin()) as u8
                } else {
                    46
                };
                let r = egui::Frame::new()
                    .fill(alpha(c, fill_a))
                    .corner_radius(CornerRadius::same(RADIUS))
                    .inner_margin(Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("⚠").size(18.0).color(c32(c)));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new("View alerts →").color(c32a(p.text2)));
                                if alerts.len() > 1 {
                                    ui.label(
                                        RichText::new(format!("+{} more", alerts.len() - 1))
                                            .color(c32a(p.text2)),
                                    );
                                }
                                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&a.display_event).strong().color(c32(p.text)),
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

            // --- hero ---
            let hero = card_frame(&p).inner_margin(Margin::same(22)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(weather_image(
                        ui.ctx(),
                        pixel_icon(w.current.weather_code, w.current.is_day),
                        &p,
                        104,
                    ));
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(RichText::new(&loc.label).size(15.0).color(c32a(p.text2)));
                        ui.label(
                            RichText::new(deg(w.current.temperature_c, units))
                                .size(64.0)
                                .color(c32(p.text)),
                        );
                        ui.label(
                            RichText::new(wmo_label(w.current.weather_code))
                                .size(18.0)
                                .color(c32(p.text)),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            let line = |ui: &mut Ui, k: &str, v: String| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(k).color(c32a(p.text3)));
                                    ui.label(RichText::new(v).color(c32(p.text)));
                                });
                            };
                            line(ui, "Feels like", deg(w.current.apparent_temperature_c, units));
                            if let Some(d) = w.daily.first() {
                                line(ui, "High", deg(d.temp_max_c, units));
                                line(ui, "Low", deg(d.temp_min_c, units));
                            }
                            line(ui, "Local time", now.format(match tf {
                                ss_core::TimeFormat::H12 => "%-I:%M %p",
                                ss_core::TimeFormat::H24 => "%H:%M",
                            }).to_string());
                        });
                    });
                });
            });
            hero_rect = hero.response.rect;
            if scroll_to == Some(search::View::Forecast) {
                ui.scroll_to_rect(hero.response.rect, Some(Align::TOP));
            }

            // --- answer to a question ---
            if let Some(ans) = app.answer.clone() {
                if ans.location_id == loc.id {
                    answer_card(ui, &p, &w, &ans, units, tf, now, actions);
                }
            }

            // --- summary ---
            summary_card(ui, app, &p, &w, &alerts, now, actions);

            // --- hourly ---
            let hourly = card_frame(&p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                section_title(ui, &p, "Next 24 hours");
                ScrollArea::horizontal()
                    .id_salt("hourly")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let i = summary::current_hour_index(&w.hourly, now);
                            for (k, h) in w.hourly.iter().skip(i).take(24).enumerate() {
                                ui.allocate_ui(egui::vec2(62.0, 112.0), |ui| {
                                    ui.vertical_centered(|ui| {
                                        ui.spacing_mut().item_spacing.y = 4.0;
                                        let t = if k == 0 { "Now".to_string() } else { fmt_time(&h.time, tf) };
                                        ui.label(RichText::new(t).size(12.5).color(c32a(p.text2)));
                                        ui.add(weather_image(
                                            ui.ctx(),
                                            pixel_icon(h.weather_code, h.is_day),
                                            &p,
                                            32,
                                        ));
                                        ui.label(RichText::new(deg(h.temperature_c, units)).size(16.0));
                                        let pop = h.precipitation_probability_pct.round();
                                        ui.label(
                                            RichText::new(if pop >= 10.0 { format!("☂ {pop}%") } else { " ".into() })
                                                .size(11.5)
                                                .color(c32(ss_assets::lighten(p.a0, 0.25))),
                                        );
                                    });
                                });
                            }
                        });
                    });
            });
            if scroll_to == Some(search::View::Hourly) {
                ui.scroll_to_rect(hourly.response.rect, Some(Align::TOP));
            }

            // --- 7 day ---
            let week = card_frame(&p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                section_title(ui, &p, "7-day forecast");
                let lo = w.daily.iter().map(|d| d.temp_min_c).fold(f64::INFINITY, f64::min);
                let hi = w.daily.iter().map(|d| d.temp_max_c).fold(f64::NEG_INFINITY, f64::max);
                for (i, d) in w.daily.iter().enumerate().take(7) {
                    let focused = app.focus_day == Some(i);
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 44.0),
                        Sense::click(),
                    );
                    if focused || resp.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(RADIUS_SMALL),
                            alpha(p.a0, if focused { 60 } else { 22 }),
                        );
                    }
                    if resp.clicked() {
                        actions.push(Action::FocusDay(if focused { None } else { Some(i) }));
                    }
                    let painter = ui.painter_at(rect);
                    let y = rect.center().y;
                    let name = if i == 0 { "Today".to_string() } else { fmt_date_short(&d.date) };
                    painter.text(
                        egui::pos2(rect.left() + 10.0, y),
                        egui::Align2::LEFT_CENTER,
                        name,
                        egui::FontId::proportional(14.5),
                        c32(p.text),
                    );
                    let tex = ss_assets::weather_texture(ui.ctx(), pixel_icon(d.weather_code, true), &p, 28);
                    painter.image(
                        tex.id(),
                        egui::Rect::from_center_size(egui::pos2(rect.left() + 104.0, y), egui::vec2(28.0, 28.0)),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    let pop = d.precipitation_probability_max_pct.round();
                    if pop >= 10.0 {
                        painter.text(
                            egui::pos2(rect.left() + 128.0, y),
                            egui::Align2::LEFT_CENTER,
                            format!("{pop}%"),
                            egui::FontId::proportional(12.5),
                            c32(ss_assets::lighten(p.a0, 0.25)),
                        );
                    }
                    // Temperature range bar across the week's span.
                    let bar_l = rect.left() + 230.0;
                    let bar_r = rect.right() - 60.0;
                    if bar_r - bar_l > 40.0 && hi > lo {
                        let x = |t: f64| bar_l + ((t - lo) / (hi - lo)) as f32 * (bar_r - bar_l);
                        painter.text(
                            egui::pos2(bar_l - 12.0, y),
                            egui::Align2::RIGHT_CENTER,
                            deg(d.temp_min_c, units),
                            egui::FontId::proportional(14.0),
                            c32a(p.text2),
                        );
                        let track = egui::Rect::from_min_max(egui::pos2(bar_l, y - 3.0), egui::pos2(bar_r, y + 3.0));
                        painter.rect_filled(track, CornerRadius::same(3), alpha(p.text, 22));
                        let fill = egui::Rect::from_min_max(
                            egui::pos2(x(d.temp_min_c), y - 3.0),
                            egui::pos2(x(d.temp_max_c).max(x(d.temp_min_c) + 6.0), y + 3.0),
                        );
                        painter.rect_filled(fill, CornerRadius::same(3), c32(p.a0));
                        painter.text(
                            egui::pos2(bar_r + 12.0, y),
                            egui::Align2::LEFT_CENTER,
                            deg(d.temp_max_c, units),
                            egui::FontId::proportional(14.0),
                            c32(p.text),
                        );
                    }
                }
                if let Some(i) = app.focus_day {
                    if let Some(d) = w.daily.get(i) {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!(
                                "{}: {} · sunrise {} · sunset {} · UV {:.0}",
                                summary::day_name(&w.daily, i),
                                wmo_label(d.weather_code),
                                fmt_time_full(&d.sunrise, tf),
                                fmt_time_full(&d.sunset, tf),
                                d.uv_index_max
                            ))
                            .color(c32a(p.text2)),
                        );
                    }
                }
            });
            if scroll_to == Some(search::View::Week) {
                ui.scroll_to_rect(week.response.rect, Some(Align::TOP));
            }

            // --- details ---
            details(ui, &p, &w, units, tf, now);
            ui.add_space(8.0);
            ui.label(
                RichText::new("Silver Skies · Forecast: Open-Meteo · Alerts: WMO / national weather services via LibreWXR")
                    .size(11.5)
                    .color(c32a(p.text3)),
            );
            ui.add_space(12.0);
        });

    // Condensed bar once the big temperature has scrolled out of view.
    let top = out.inner_rect.top();
    if hero_rect.is_finite() && hero_rect.bottom() < top + 8.0 {
        let bar = egui::Rect::from_min_size(
            egui::pos2(out.inner_rect.left(), top),
            egui::vec2(full_w, 52.0),
        );
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::unique("condensed_bar"),
        ));
        painter.rect_filled(bar, CornerRadius::same(RADIUS), crate::theme::card(&p));
        painter.rect_stroke(
            bar,
            CornerRadius::same(RADIUS),
            egui::Stroke::new(1.0, alpha(p.a0, 90)),
            egui::StrokeKind::Inside,
        );
        let tex = ss_assets::weather_texture(
            ui.ctx(),
            pixel_icon(w.current.weather_code, w.current.is_day),
            &p,
            30,
        );
        painter.image(
            tex.id(),
            egui::Rect::from_center_size(
                egui::pos2(bar.left() + 30.0, bar.center().y),
                egui::vec2(30.0, 30.0),
            ),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        let g = painter.layout_no_wrap(
            deg(w.current.temperature_c, units),
            egui::FontId::proportional(22.0),
            c32(p.text),
        );
        let tw = g.size().x;
        painter.galley(
            egui::pos2(bar.left() + 56.0, bar.center().y - g.size().y / 2.0),
            g,
            c32(p.text),
        );
        painter.text(
            egui::pos2(bar.left() + 66.0 + tw, bar.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{} · {}", loc.label, wmo_label(w.current.weather_code)),
            egui::FontId::proportional(14.0),
            c32a(p.text2),
        );
    }
}

fn empty_state(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(80.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(text).size(16.0).color(c32a(p.text2)));
    });
}

fn ai_block(ui: &mut Ui, p: &Palette, state: &AiState) {
    match state {
        AiState::Idle => {}
        AiState::Waiting | AiState::Loading(_) => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0).color(c32(p.a0)));
                ui.label(RichText::new("Asking the AI…").color(c32a(p.text2)));
            });
        }
        AiState::Done(t) => {
            egui::Frame::new()
                .fill(alpha(p.a0, 26))
                .corner_radius(CornerRadius::same(RADIUS_SMALL))
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("✦ AI").size(11.5).strong().color(c32(p.a0)));
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
    tf: ss_core::TimeFormat,
    now: chrono::NaiveDateTime,
    actions: &mut Vec<Action>,
) {
    card_frame(p).fill(alpha(p.a0, 40)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("“{}”", ans.question)).color(c32a(p.text2)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("✕").on_hover_text("Dismiss").clicked() {
                    actions.push(Action::DismissAnswer);
                }
            });
        });
        let local = summary::answer(w, ans.topic, ans.day, units, tf, now);
        ui.add(egui::Label::new(RichText::new(local).size(17.0).color(c32(p.text))).wrap());
        ai_block(ui, p, &ans.ai);
    });
}

fn summary_card(
    ui: &mut Ui,
    app: &App,
    p: &Palette,
    w: &Weather,
    alerts: &[Alert],
    now: chrono::NaiveDateTime,
    actions: &mut Vec<Action>,
) {
    let s = summary::summarize(w, alerts, app.config.units, app.config.time_format, now);
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            section_title(ui, p, "Summary");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let id = app.active_id().unwrap_or_default();
                let state = app.ai_summary.get(&id);
                let busy = matches!(state, Some(AiState::Loading(_) | AiState::Waiting));
                if app.prefs.ai_key().is_some() {
                    let label = if matches!(state, Some(AiState::Done(_))) {
                        "✦ Refresh AI summary"
                    } else {
                        "✦ AI summary"
                    };
                    if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                        actions.push(Action::AiSummary);
                    }
                } else if ui
                    .add(
                        egui::Button::new(
                            RichText::new("✦ Turn on AI summaries").color(c32a(p.text3)),
                        )
                        .fill(egui::Color32::TRANSPARENT),
                    )
                    .clicked()
                {
                    actions.push(Action::OpenSettings);
                }
            });
        });
        ui.add(egui::Label::new(RichText::new(&s.headline).size(16.5).color(c32(p.text))).wrap());
        ui.spacing_mut().item_spacing.y = 4.0;
        for pt in &s.points {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("•").color(c32(p.a0)));
                ui.add(egui::Label::new(RichText::new(pt).color(c32a(p.text2))).wrap());
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

fn tile(
    ui: &mut Ui,
    p: &Palette,
    w: f32,
    k: &str,
    v: String,
    sub: String,
    sub_color: egui::Color32,
) {
    ui.allocate_ui_with_layout(egui::vec2(w, 96.0), Layout::top_down(Align::Min), |ui| {
        card_frame(p).inner_margin(Margin::same(14)).show(ui, |ui| {
            ui.set_width(w - 28.0);
            ui.set_height(64.0);
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(RichText::new(k).size(12.0).color(c32a(p.text3)));
            ui.label(RichText::new(v).size(20.0).color(c32(p.text)));
            ui.label(RichText::new(sub).size(12.0).color(sub_color));
        });
    });
}

/// Conditions detail: quick tiles, then the UV gauge and the sun's arc.
fn details(
    ui: &mut Ui,
    p: &Palette,
    w: &Weather,
    units: UnitPref,
    tf: ss_core::TimeFormat,
    now: chrono::NaiveDateTime,
) {
    let c = &w.current;
    section_title(ui, p, "Conditions detail");
    let tiles: Vec<(&str, String, String)> = vec![
        (
            "Humidity",
            format!("{:.0}%", c.relative_humidity_pct),
            String::new(),
        ),
        (
            "Wind",
            fmt_wind(c.wind_speed_kmh, units),
            format!("from the {}", compass_8(c.wind_direction_deg)),
        ),
        (
            "Apparent temp",
            deg(c.apparent_temperature_c, units),
            "Feels like outside".into(),
        ),
        (
            "Precipitation",
            fmt_precip(c.precipitation_mm, units),
            "last hour".into(),
        ),
        (
            "Cloud cover",
            format!("{:.0}%", c.cloud_cover_pct),
            cloud_desc(c.cloud_cover_pct).into(),
        ),
    ];
    let gap = 12.0;
    let cols = ((ui.available_width() / 180.0).floor() as usize).clamp(2, 5);
    let tile_w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
    for row in tiles.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (k, v, sub) in row {
                tile(ui, p, tile_w, k, v.clone(), sub.clone(), c32a(p.text2));
            }
        });
    }

    let half = (ui.available_width() - gap) / 2.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        // --- UV gauge ---
        ui.allocate_ui_with_layout(
            egui::vec2(half, 170.0),
            Layout::top_down(Align::Min),
            |ui| {
                card_frame(p).show(ui, |ui| {
                    ui.set_width(half - 32.0);
                    ui.set_height(138.0);
                    let (uv_text, uv_rgb) = uv_label(c.uv_index);
                    ui.label(RichText::new("UV index").size(12.0).color(c32a(p.text3)));
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{:.0}", c.uv_index))
                                .size(30.0)
                                .color(c32(p.text)),
                        );
                        ui.label(RichText::new(uv_text).size(15.0).color(c32(uv_rgb)));
                    });
                    let (r, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 18.0),
                        Sense::hover(),
                    );
                    let bar = egui::Rect::from_min_max(
                        egui::pos2(r.left(), r.center().y - 4.0),
                        egui::pos2(r.right(), r.center().y + 4.0),
                    );
                    let stops: [[u8; 3]; 5] = [
                        [0x3e, 0xb8, 0x5f],
                        [0xf2, 0xd0, 0x2c],
                        [0xf5, 0x8a, 0x1f],
                        [0xe5, 0x3b, 0x3b],
                        [0x9b, 0x4d, 0xd6],
                    ];
                    let mut mesh = egui::Mesh::default();
                    for (i, col) in stops.iter().enumerate() {
                        let x = bar.left() + bar.width() * i as f32 / (stops.len() - 1) as f32;
                        mesh.colored_vertex(egui::pos2(x, bar.top()), c32(*col));
                        mesh.colored_vertex(egui::pos2(x, bar.bottom()), c32(*col));
                    }
                    for i in 0..(stops.len() as u32 - 1) {
                        let b = i * 2;
                        mesh.add_triangle(b, b + 1, b + 2);
                        mesh.add_triangle(b + 1, b + 3, b + 2);
                    }
                    ui.painter().add(mesh);
                    let mx = bar.left() + bar.width() * (c.uv_index / 11.0).clamp(0.0, 1.0) as f32;
                    ui.painter().circle(
                        egui::pos2(mx, bar.center().y),
                        7.0,
                        c32(p.surface),
                        egui::Stroke::new(2.5, c32(p.text)),
                    );
                    ui.label(
                        RichText::new(if c.uv_index > 5.0 {
                            "SPF recommended"
                        } else {
                            "Low risk"
                        })
                        .size(12.5)
                        .color(c32a(p.text2)),
                    );
                });
            },
        );
        // --- sun position ---
        ui.allocate_ui_with_layout(
            egui::vec2(half, 170.0),
            Layout::top_down(Align::Min),
            |ui| {
                card_frame(p).show(ui, |ui| {
                    ui.set_width(half - 32.0);
                    ui.set_height(138.0);
                    ui.label(
                        RichText::new("Sun position")
                            .size(12.0)
                            .color(c32a(p.text3)),
                    );
                    let Some(d) = w.daily.first() else { return };
                    let (r, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 84.0),
                        Sense::hover(),
                    );
                    let base_y = r.bottom() - 16.0;
                    let radius = (r.width() / 2.0 - 20.0)
                        .min(base_y - r.top() - 4.0)
                        .max(10.0);
                    let centre = egui::pos2(r.center().x, base_y);
                    let arc = |t0: f64, t1: f64| -> Vec<egui::Pos2> {
                        (0..=48)
                            .map(|i| {
                                let t = t0 + (t1 - t0) * f64::from(i) / 48.0;
                                let ang = std::f64::consts::PI * (1.0 - t);
                                centre
                                    + egui::vec2(
                                        (ang.cos() as f32) * radius,
                                        -(ang.sin() as f32) * radius,
                                    )
                            })
                            .collect()
                    };
                    ui.painter().add(egui::Shape::dashed_line(
                        &[
                            egui::pos2(r.left() + 6.0, base_y),
                            egui::pos2(r.right() - 6.0, base_y),
                        ],
                        egui::Stroke::new(1.0, c32a(p.border)),
                        4.0,
                        4.0,
                    ));
                    ui.painter().add(egui::Shape::line(
                        arc(0.0, 1.0),
                        egui::Stroke::new(1.5, alpha(p.text, 40)),
                    ));
                    if let Some(t) = sun_progress(&d.sunrise, &d.sunset, now) {
                        ui.painter().add(egui::Shape::line(
                            arc(0.0, t),
                            egui::Stroke::new(2.5, c32(p.a0)),
                        ));
                        let ang = std::f64::consts::PI * (1.0 - t);
                        let dot = centre
                            + egui::vec2((ang.cos() as f32) * radius, -(ang.sin() as f32) * radius);
                        ui.painter().circle_filled(dot, 11.0, alpha(p.a0, 60));
                        ui.painter()
                            .circle_filled(dot, 6.0, c32(ss_assets::lighten(p.a0, 0.4)));
                    }
                    ui.painter().text(
                        egui::pos2(r.left() + 6.0, r.bottom()),
                        egui::Align2::LEFT_BOTTOM,
                        format!("↑ {}", fmt_time_full(&d.sunrise, tf)),
                        egui::FontId::proportional(12.0),
                        c32a(p.text2),
                    );
                    ui.painter().text(
                        egui::pos2(r.right() - 6.0, r.bottom()),
                        egui::Align2::RIGHT_BOTTOM,
                        format!("{} ↓", fmt_time_full(&d.sunset, tf)),
                        egui::FontId::proportional(12.0),
                        c32a(p.text2),
                    );
                    if let Some(dl) = daylight(&d.sunrise, &d.sunset) {
                        ui.label(
                            RichText::new(format!("Daylight · {dl}"))
                                .size(12.5)
                                .color(c32a(p.text2)),
                        );
                    }
                });
            },
        );
    });
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
        Severity::Unknown => "Unknown severity",
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
    let alerts = alerts.as_slice();
    let full_w = ui.available_width() - 14.0;
    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.set_width(full_w);
        ui.set_max_width(full_w);
        ui.spacing_mut().item_spacing.y = 12.0;
        if alerts.is_empty() {
            card_frame(&p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("No active alerts").size(18.0).color(c32(p.text)));
                ui.label(
                    RichText::new(format!(
                        "There are no official warnings, watches or advisories for {} right now.",
                        loc.label
                    ))
                    .color(c32a(p.text2)),
                );
            });
        }
        for a in alerts {
            let c = class_color(&a.class);
            let frame = card_frame(&p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .fill(c32(c))
                        .corner_radius(CornerRadius::same(255))
                        .inner_margin(Margin::symmetric(10, 3))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(severity_text(a.severity))
                                    .size(12.0)
                                    .strong()
                                    .color(if ss_core::theme::brightness(c) > 150.0 {
                                        egui::Color32::BLACK
                                    } else {
                                        egui::Color32::WHITE
                                    }),
                            );
                        });
                    ui.label(RichText::new(&a.display_event).size(17.0).strong().color(c32(p.text)));
                });
                if let Some(h) = &a.headline {
                    ui.add(egui::Label::new(RichText::new(h).color(c32a(p.text2))).wrap());
                }
                ui.horizontal_wrapped(|ui| {
                    if let Some(area) = &a.area_desc {
                        ui.label(RichText::new(area).size(12.5).color(c32a(p.text3)));
                    }
                    if let Some(e) = &a.expires {
                        ui.label(RichText::new(format!("· until {}", fmt_until(e))).size(12.5).color(c32a(p.text3)));
                    }
                    if let Some(o) = &a.issuing_office {
                        ui.label(RichText::new(format!("· {o}")).size(12.5).color(c32a(p.text3)));
                    }
                });
                let has_more = a.description.is_some() || a.instruction.is_some();
                if has_more {
                    egui::CollapsingHeader::new(RichText::new("Details").color(c32(p.a0)))
                        .id_salt(("alert", &a.id))
                        .show(ui, |ui| {
                            if let Some(d) = &a.description {
                                ui.add(egui::Label::new(RichText::new(d.trim()).color(c32(p.text))).wrap());
                            }
                            if let Some(i) = &a.instruction {
                                ui.add_space(4.0);
                                ui.label(RichText::new("What to do").strong().color(c32(p.text)));
                                ui.add(egui::Label::new(RichText::new(i.trim()).color(c32(p.text))).wrap());
                            }
                            if let Some(u) = &a.url {
                                ui.hyperlink_to("Read the full alert", u);
                            }
                        });
                }
            });
            // Severity stripe down the left edge of the card.
            let r = frame.response.rect;
            ui.painter().rect_filled(
                egui::Rect::from_min_size(r.min, egui::vec2(5.0, r.height())),
                CornerRadius {
                    nw: RADIUS,
                    sw: RADIUS,
                    ne: 0,
                    se: 0,
                },
                c32(c),
            );
        }
        ui.label(
            RichText::new("Alerts come from the WMO alerting network (official CAP feeds from national weather services), via LibreWXR.")
                .size(11.5)
                .color(c32a(p.text3)),
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
}
