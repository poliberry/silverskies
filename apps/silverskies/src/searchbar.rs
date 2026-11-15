//! The search field and its suggestion dropdown: questions, keyword actions,
//! saved places and geocoder results, all fuzzy-ranked as you type.

use std::time::{Duration, Instant};

use egui::{CornerRadius, Key, Margin, RichText, Sense, Ui};
use ss_core::theme::Palette;
use ss_core::{Location, SavedLocation};

use crate::app::Action;
use crate::search;
use crate::theme::{alpha, c32, c32a, RADIUS};

const DEBOUNCE: Duration = Duration::from_millis(350);
const MAX_PLACES: usize = 4;
const MAX_KEYWORDS: usize = 3;
const MAX_RESULTS: usize = 5;

#[derive(Default)]
pub struct SearchBar {
    text: String,
    edited_at: Option<Instant>,
    /// The query we last sent to the geocoder, and its results.
    asked: Option<String>,
    results: Vec<Location>,
    results_for: Option<String>,
    selected: usize,
    open: bool,
}

#[derive(Debug, Clone)]
struct Suggestion {
    /// Small label on the left ("Ask", "Go to", "Place", …).
    kind: &'static str,
    title: String,
    detail: Option<String>,
    action: Action,
}

impl SearchBar {
    /// The text the geocoder should look up for the current input.
    fn geocode_text(&self) -> Option<String> {
        let parsed = search::parse(&self.text);
        let place = parsed.place?;
        (place.chars().count() >= 3).then_some(place)
    }

    /// A debounced geocoder request, at most once per distinct query.
    pub fn wants_geocode(&mut self) -> Option<String> {
        let at = self.edited_at?;
        if at.elapsed() < DEBOUNCE {
            return None;
        }
        let q = self.geocode_text()?;
        if self.asked.as_deref() == Some(q.as_str()) {
            return None;
        }
        self.asked = Some(q.clone());
        Some(q)
    }

    pub fn set_results(&mut self, query: String, results: Vec<Location>) {
        if self.asked.as_deref() == Some(query.as_str()) {
            self.results = results;
            self.results_for = Some(query);
        }
    }

    fn suggestions(&self, places: &[SavedLocation], ai_on: bool) -> Vec<Suggestion> {
        let q = self.text.trim();
        let mut out = Vec::new();
        if q.is_empty() {
            return out;
        }
        let parsed = search::parse(q);
        let natural = parsed.is_question || parsed.topic.is_some() || parsed.day.is_some();
        if natural {
            out.push(Suggestion {
                kind: if parsed.is_question { "Ask" } else { "Show" },
                title: q.to_string(),
                detail: Some(match (&parsed.place, ai_on) {
                    (Some(p), true) => format!("{p} · answered with AI"),
                    (Some(p), false) => p.clone(),
                    (None, true) => "Current place · answered with AI".into(),
                    (None, false) => "Current place".into(),
                }),
                action: Action::Query(q.to_string()),
            });
        }

        let mut kws: Vec<(i32, search::Keyword)> = search::keywords()
            .into_iter()
            .filter_map(|k| search::keyword_score(q, &k).map(|s| (s, k)))
            .filter(|(s, _)| *s >= 20)
            .collect();
        kws.sort_by_key(|k| std::cmp::Reverse(k.0));
        // Keywords lead when they clearly match; otherwise places do.
        let kw_items: Vec<Suggestion> = kws
            .into_iter()
            .take(MAX_KEYWORDS)
            .map(|(_, k)| Suggestion {
                kind: "Go to",
                title: k.title.to_string(),
                detail: None,
                action: Action::Intent(k.intent),
            })
            .collect();

        let needle = parsed.place.clone().unwrap_or_else(|| q.to_string());
        let saved: Vec<Suggestion> = search::rank(&needle, places.iter().map(|l| l.label.as_str()))
            .into_iter()
            .take(MAX_PLACES)
            .map(|(i, _)| Suggestion {
                kind: "Saved",
                title: places[i].label.clone(),
                detail: None,
                action: Action::Select(places[i].id.clone()),
            })
            .collect();

        let looks_like_keyword = kw_items
            .first()
            .is_some_and(|_| q.split_whitespace().count() == 1 && saved.is_empty());
        if looks_like_keyword {
            out.extend(kw_items);
            out.extend(saved);
        } else {
            out.extend(saved);
            out.extend(kw_items);
        }

        if !natural && self.results_for.as_deref() == self.geocode_text().as_deref() {
            for r in self.results.iter().take(MAX_RESULTS) {
                // Skip results that duplicate a saved place.
                if places
                    .iter()
                    .any(|l| ss_core::geo::haversine_km(l.lat, l.lon, r.lat, r.lon) < 3.0)
                {
                    continue;
                }
                out.push(Suggestion {
                    kind: "Place",
                    title: r.label.clone(),
                    detail: Some(r.coord_label()),
                    action: Action::AddPlace(r.clone()),
                });
            }
        }
        out
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        width: f32,
        places: &[SavedLocation],
        ai_on: bool,
    ) -> Option<Action> {
        let mut chosen = None;
        let hint = if ai_on {
            "Search places, or ask “will it rain tomorrow?”"
        } else {
            "Search places or keywords — “Paris”, “radar”, “weather in Tokyo tomorrow”"
        };
        let edit = egui::TextEdit::singleline(&mut self.text)
            .hint_text(RichText::new(hint).color(c32a(p.text3)))
            .desired_width(width - 44.0)
            .margin(Margin::symmetric(14, 9))
            .frame(egui::Frame::NONE);
        let frame = egui::Frame::new()
            .fill(c32(p.surface3))
            .corner_radius(CornerRadius::same(255))
            .inner_margin(Margin {
                left: 14,
                right: 4,
                top: 0,
                bottom: 0,
            });
        let mut resp = None;
        let outer = frame.show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                // Magnifier, painted (no font has a reliable glyph for it).
                let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::hover());
                let stroke = egui::Stroke::new(1.8, c32a(p.text2));
                let c = r.center() - egui::vec2(2.0, 2.0);
                ui.painter().circle_stroke(c, 5.5, stroke);
                ui.painter()
                    .line_segment([c + egui::vec2(4.0, 4.0), c + egui::vec2(8.5, 8.5)], stroke);
                resp = Some(ui.add(edit));
            });
        });
        let resp = resp.expect("text edit drawn");
        let field_rect = outer.response.rect;

        if resp.changed() {
            self.edited_at = Some(Instant::now());
            self.selected = 0;
            self.open = true;
            ui.ctx().request_repaint_after(DEBOUNCE);
        }
        if resp.gained_focus() {
            self.open = true;
        }

        let items = self.suggestions(places, ai_on);
        if !items.is_empty() {
            self.selected = self.selected.min(items.len() - 1);
        }
        let focused = resp.has_focus();
        if focused {
            if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                self.selected = (self.selected + 1).min(items.len().saturating_sub(1));
            }
            if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                self.selected = self.selected.saturating_sub(1);
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                self.open = false;
            }
        }
        // Enter: the highlighted suggestion, or the raw text as a query.
        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            chosen = items
                .get(self.selected)
                .map(|s| s.action.clone())
                .or_else(|| {
                    let t = self.text.trim();
                    (!t.is_empty()).then(|| Action::Query(t.to_string()))
                });
        }

        // --- dropdown ---
        let show_dropdown = self.open && !items.is_empty() && (focused || chosen.is_none());
        if show_dropdown && chosen.is_none() {
            let area = egui::Area::new(egui::Id::unique("search_dropdown"))
                .order(egui::Order::Foreground)
                .fixed_pos(field_rect.left_bottom() + egui::vec2(0.0, 6.0));
            let inner = area.show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(c32(p.surface))
                    .corner_radius(CornerRadius::same(RADIUS))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_width(field_rect.width() - 12.0);
                        for (i, s) in items.iter().enumerate() {
                            let row = ui.allocate_response(
                                egui::vec2(ui.available_width(), 38.0),
                                Sense::click(),
                            );
                            let hot = row.hovered() || i == self.selected;
                            if row.hovered() {
                                self.selected = i;
                            }
                            if hot {
                                ui.painter().rect_filled(
                                    row.rect,
                                    CornerRadius::same(10),
                                    alpha(p.a0, 50),
                                );
                            }
                            let painter = ui.painter_at(row.rect);
                            let y = row.rect.center().y;
                            painter.text(
                                egui::pos2(row.rect.left() + 12.0, y),
                                egui::Align2::LEFT_CENTER,
                                s.kind,
                                egui::FontId::proportional(11.5),
                                c32(p.a0),
                            );
                            let title_x = row.rect.left() + 70.0;
                            let g = painter.layout_no_wrap(
                                s.title.clone(),
                                egui::FontId::proportional(14.5),
                                c32(p.text),
                            );
                            let tw = g.size().x;
                            painter.galley(
                                egui::pos2(title_x, y - g.size().y / 2.0),
                                g,
                                c32(p.text),
                            );
                            if let Some(d) = &s.detail {
                                painter.text(
                                    egui::pos2(title_x + tw + 10.0, y),
                                    egui::Align2::LEFT_CENTER,
                                    d,
                                    egui::FontId::proportional(12.0),
                                    c32a(p.text3),
                                );
                            }
                            if row.clicked() {
                                chosen = Some(s.action.clone());
                            }
                        }
                        if ai_on {
                            ui.add_space(2.0);
                        }
                    });
            });
            // Close when clicking anywhere outside the field and dropdown.
            if ui.input(|i| i.pointer.any_pressed())
                && !inner.response.hovered()
                && !resp.hovered()
                && chosen.is_none()
            {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    if !inner.response.rect.contains(pos) && !field_rect.contains(pos) {
                        self.open = false;
                    }
                }
            }
        }

        if chosen.is_some() {
            self.text.clear();
            self.results.clear();
            self.results_for = None;
            self.asked = None;
            self.edited_at = None;
            self.open = false;
            resp.surrender_focus();
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Intent;

    fn saved(label: &str) -> SavedLocation {
        SavedLocation {
            id: label.to_lowercase(),
            label: label.into(),
            lat: 0.0,
            lon: 0.0,
            accu_weather_location_key: None,
        }
    }

    #[test]
    fn suggestions_mix_questions_keywords_and_places() {
        let places = vec![saved("Seattle"), saved("San Diego"), saved("Norman")];
        let mut sb = SearchBar {
            text: "radar".into(),
            ..Default::default()
        };
        let s = sb.suggestions(&places, false);
        assert!(matches!(
            s[0].action,
            Action::Intent(Intent::View(search::View::Radar))
        ));

        sb.text = "sea".into();
        let s = sb.suggestions(&places, false);
        assert!(matches!(&s[0].action, Action::Select(id) if id == "seattle"));

        sb.text = "will it rain tomorrow?".into();
        let s = sb.suggestions(&places, true);
        assert_eq!(s[0].kind, "Ask");
        assert!(s[0].detail.as_deref().unwrap().contains("AI"));
    }

    #[test]
    fn geocode_is_debounced_and_deduplicated() {
        let mut sb = SearchBar {
            text: "weather in Paris".into(),
            edited_at: Some(Instant::now() - Duration::from_secs(1)),
            ..Default::default()
        };
        assert_eq!(sb.wants_geocode().as_deref(), Some("Paris"));
        assert_eq!(sb.wants_geocode(), None);
        sb.text = "Pa".into();
        sb.asked = None;
        assert_eq!(sb.wants_geocode(), None, "too short");
    }

    #[test]
    fn stale_results_are_ignored() {
        let mut sb = SearchBar {
            asked: Some("Paris".into()),
            ..Default::default()
        };
        sb.set_results("Pari".into(), vec![Location::new(1.0, 2.0, "Old")]);
        assert!(sb.results.is_empty());
        sb.set_results("Paris".into(), vec![Location::new(48.8, 2.3, "Paris")]);
        assert_eq!(sb.results.len(), 1);
    }
}
