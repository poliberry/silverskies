//! The search field and its suggestion dropdown: questions, keyword actions,
//! saved places and geocoder results, all fuzzy-ranked as you type.

use std::time::{Duration, Instant};

use egui::{CornerRadius, Key, Margin, RichText, Sense, Ui};
use ss_core::theme::Palette;
use ss_core::{Location, SavedLocation};

use crate::app::Action;
use crate::search;
use crate::theme::c32;

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
    /// Take keyboard focus on the next frame (Edit → Find, Ctrl+F).
    pub focus_next: bool,
    /// Where the dropdown was drawn last frame.
    dropdown_rect: Option<egui::Rect>,
    /// Give up focus and close the dropdown on the next frame.
    blur_next: bool,
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
    /// Leave the field (a dialog opened or the view changed).
    pub fn blur(&mut self) {
        self.blur_next = true;
    }

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
            "Search city or keyword… (e.g. Tucson, AZ · radar · weather in Paris tomorrow)"
        };
        // Material 3 search bar: a full-round container, leading magnifier,
        // trailing filled "Search" button; it lifts a tone while focused.
        let sch = crate::theme::scheme(p);
        let field_h = 52.0;
        let (field_rect, field_resp) =
            ui.allocate_exact_size(egui::vec2(width, field_h), Sense::hover());
        let edit_id = ui.make_persistent_id("search_text");
        let focused_before = ui.memory(|m| m.has_focus(edit_id));
        let fill = if focused_before {
            sch.surface_container_highest
        } else if field_resp.hovered() {
            sch.surface_container_high
                .lerp_to_gamma(sch.on_surface, 0.05)
        } else {
            sch.surface_container_high
        };
        ui.painter()
            .rect_filled(field_rect, CornerRadius::same(255), fill);
        let btn_w = 96.0;
        let text_w = (width - 18.0 - 22.0 - 8.0 - btn_w - 8.0 - 8.0).max(80.0);
        let edit = egui::TextEdit::singleline(&mut self.text)
            .id(edit_id)
            .hint_text(RichText::new(hint).color(sch.on_surface_variant))
            .font(egui::FontId::proportional(15.5))
            .text_color(sch.on_surface)
            .desired_width(text_w)
            .min_size(egui::vec2(text_w, field_h - 12.0))
            .align(egui::Align2::LEFT_CENTER)
            .margin(Margin::symmetric(4, 0))
            .frame(egui::Frame::NONE);
        let mut search_clicked = false;
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("search_field")
                .max_rect(field_rect.shrink2(egui::vec2(0.0, 6.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 4.0;
        child.add_space(18.0);
        // Magnifier, painted (no font has a reliable glyph for it).
        let (r, _) = child.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
        let stroke = egui::Stroke::new(2.2, sch.on_surface_variant);
        let c = r.center() - egui::vec2(1.5, 1.5);
        child.painter().circle_stroke(c, 6.5, stroke);
        crate::m3::rounded_line(
            child.painter(),
            c + egui::vec2(4.8, 4.8),
            c + egui::vec2(9.0, 9.0),
            2.4,
            sch.on_surface_variant,
        );
        child.add_space(6.0);
        let resp = child.add(edit);
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            let btn = egui::Button::new(crate::theme::label("Search", 14.0, sch.on_primary))
                .fill(sch.primary)
                .min_size(egui::vec2(btn_w, field_h - 12.0));
            search_clicked = ui.add(btn).clicked();
        });

        if resp.changed() {
            self.edited_at = Some(Instant::now());
            self.selected = 0;
            self.open = true;
            ui.ctx().request_repaint_after(DEBOUNCE);
        }
        if std::mem::take(&mut self.focus_next) {
            resp.request_focus();
        }
        if std::mem::take(&mut self.blur_next) {
            resp.surrender_focus();
            self.open = false;
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
        if search_clicked || (resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
            chosen = items
                .get(self.selected)
                .map(|s| s.action.clone())
                .or_else(|| {
                    let t = self.text.trim();
                    (!t.is_empty()).then(|| Action::Query(t.to_string()))
                });
        }

        // --- dropdown ---
        // Visible while typing, or while the pointer is on it (a click on a
        // row takes focus from the field before the click lands).
        let over_dropdown = self.dropdown_rect.is_some_and(|r| {
            ui.input(|i| i.pointer.hover_pos())
                .is_some_and(|p| r.contains(p))
        });
        let show_dropdown = self.open && !items.is_empty() && (focused || over_dropdown);
        if !show_dropdown {
            self.dropdown_rect = None;
        }
        if show_dropdown && chosen.is_none() {
            let area = egui::Area::new(egui::Id::unique("search_dropdown"))
                .order(egui::Order::Foreground)
                .fixed_pos(field_rect.left_bottom() + egui::vec2(0.0, 6.0));
            let inner = area.show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(sch.surface_container_high)
                    .corner_radius(CornerRadius::same(crate::m3::corner::EXTRA_LARGE))
                    .stroke(egui::Stroke::NONE)
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(field_rect.width() - 12.0);
                        for (i, s) in items.iter().enumerate() {
                            let row = ui.allocate_response(
                                egui::vec2(ui.available_width(), 44.0),
                                Sense::click(),
                            );
                            let hot = row.hovered() || i == self.selected;
                            if row.hovered() {
                                self.selected = i;
                            }
                            if hot {
                                ui.painter().rect_filled(
                                    row.rect,
                                    CornerRadius::same(255),
                                    sch.secondary_container,
                                );
                            }
                            let painter = ui.painter_at(row.rect);
                            let y = row.rect.center().y;
                            // Kind as a small tonal chip.
                            let kg = painter.layout_no_wrap(
                                s.kind.to_string(),
                                crate::theme::medium_font(11.5),
                                sch.on_tertiary_container,
                            );
                            let chip = egui::Rect::from_min_size(
                                egui::pos2(row.rect.left() + 10.0, y - 11.0),
                                egui::vec2(kg.size().x + 16.0, 22.0),
                            );
                            painter.rect_filled(
                                chip,
                                CornerRadius::same(255),
                                sch.tertiary_container,
                            );
                            painter.galley(
                                chip.center() - kg.size() / 2.0,
                                kg,
                                sch.on_tertiary_container,
                            );
                            let title_x = row.rect.left() + 96.0;
                            let g = painter.layout_no_wrap(
                                s.title.clone(),
                                egui::FontId::proportional(15.0),
                                if hot {
                                    sch.on_secondary_container
                                } else {
                                    sch.on_surface
                                },
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
                                    egui::FontId::proportional(12.5),
                                    sch.on_surface_variant,
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
            self.dropdown_rect = Some(inner.response.rect);
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
