//! Form widgets for Pro's dialogs (settings, prompts): section cards,
//! label/control rows, toggle switches, framed text fields and buttons, all
//! drawn from the palette so they match the terminal look while reading as
//! real controls.

use egui::{
    vec2, Align, Align2, Color32, CornerRadius, FontId, Galley, Layout, Margin, Rect, Response,
    RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, WidgetText,
};
use ss_core::theme::Palette;
use std::sync::Arc;

use crate::theme::{c32, c32a, on, saira, PRO_BLUE};

/// Corner radius of fields, buttons and cards.
pub const R: u8 = 3;
/// Height of single-line fields, buttons and combo boxes.
pub const CONTROL_H: f32 = 28.0;
/// Minimum height of a settings row.
const ROW_H: f32 = 44.0;

fn border(p: &Palette) -> Color32 {
    c32a(p.border)
}

/// The card fill: a step up from the background.
pub fn raised(p: &Palette) -> Color32 {
    lerp_color(c32(p.bg), c32(p.surface3), 0.7)
}

/// Neutral secondary text (help lines, descriptions): the text colour
/// faded toward the background, not the amber label colour.
pub fn muted(p: &Palette) -> Color32 {
    lerp_color(c32(p.text), c32(p.bg), 0.42)
}

/// Page title and one-line description at the top of a settings page.
pub fn page_header(ui: &mut Ui, p: &Palette, title: &str, subtitle: &str) {
    ui.label(
        RichText::new(title)
            .font(saira(22.0, "medium"))
            .color(c32(p.text)),
    );
    ui.add_space(2.0);
    ui.add(
        egui::Label::new(
            RichText::new(subtitle)
                .font(saira(13.0, "light"))
                .color(muted(p)),
        )
        .wrap(),
    );
    ui.add_space(14.0);
}

/// A titled group of rows on a raised card.
pub fn card<R>(ui: &mut Ui, p: &Palette, title: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    if !title.is_empty() {
        ui.label(
            RichText::new(title.to_uppercase())
                .font(saira(11.5, "semibold"))
                .extra_letter_spacing(1.2)
                .color(c32(p.a0)),
        );
        ui.add_space(4.0);
    }
    let r = egui::Frame::new()
        .fill(raised(p))
        .stroke(Stroke::new(1.0, border(p)))
        .corner_radius(CornerRadius::same(R))
        .inner_margin(Margin::symmetric(14, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            body(ui)
        })
        .inner;
    ui.add_space(18.0);
    r
}

fn galley(ui: &Ui, text: &str, font: FontId, color: Color32, wrap: f32) -> Arc<Galley> {
    WidgetText::from(RichText::new(text).font(font).color(color)).into_galley(
        ui,
        Some(egui::TextWrapMode::Wrap),
        wrap,
        egui::TextStyle::Body,
    )
}

/// One setting: a label (and optional help line) on the left, the control
/// right-aligned and vertically centred on the right, a hairline below.
/// `control_w` is the width reserved for the control.
pub fn row<T>(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    help: Option<&str>,
    control_w: f32,
    control: impl FnOnce(&mut Ui) -> T,
) -> T {
    let full = ui.available_width();
    let gap = 16.0;
    let text_w = (full - control_w - gap).max(80.0);
    let lg = galley(ui, label, saira(14.0, "regular"), c32(p.text), text_w);
    let hg = help.map(|h| galley(ui, h, saira(12.0, "light"), muted(p), text_w));
    let text_h = lg.size().y + hg.as_ref().map_or(0.0, |g| g.size().y + 1.0);
    let h = (text_h + 16.0).max(ROW_H);
    let top = ui.cursor().min;
    let rect = Rect::from_min_size(top, vec2(full, h));
    // Text block, vertically centred.
    let mut y = rect.center().y - text_h / 2.0;
    let painter = ui.painter().clone();
    let lh = lg.size().y;
    painter.galley(egui::pos2(rect.left(), y), lg, c32(p.text));
    y += lh + 1.0;
    if let Some(g) = hg {
        painter.galley(egui::pos2(rect.left(), y), g, muted(p));
    }
    // Control, right-aligned and centred in the row.
    let crect = Rect::from_min_max(
        egui::pos2(rect.right() - control_w, rect.top()),
        rect.right_bottom(),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(crect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let out = control(&mut child);
    ui.allocate_rect(rect, Sense::hover());
    out
}

/// A hairline between rows on a card.
pub fn divider(ui: &mut Ui, p: &Palette) {
    let r = ui.available_rect_before_wrap();
    let y = ui.cursor().min.y;
    ui.painter().hline(
        r.left()..=r.right(),
        y,
        Stroke::new(1.0, border(p).gamma_multiply(0.7)),
    );
    ui.add_space(1.0);
}

/// Rows on a card with dividers between them: call `next` before each row
/// but the first.
pub struct Rows {
    first: bool,
}

impl Rows {
    pub fn new() -> Self {
        Self { first: true }
    }
    pub fn next(&mut self, ui: &mut Ui, p: &Palette) {
        if !std::mem::replace(&mut self.first, false) {
            divider(ui, p);
        }
    }
}

/// An on/off switch. Returns the response; `changed()` when flipped.
pub fn toggle(ui: &mut Ui, p: &Palette, on_: &mut bool) -> Response {
    let size = vec2(38.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on_ = !*on_;
        resp.mark_changed();
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), *on_, ""));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(resp.id, *on_, 0.12);
        let enabled = ui.is_enabled();
        let off_fill = c32(p.bg);
        let on_fill = c32(p.a0);
        let mut fill = lerp_color(off_fill, on_fill, t);
        let mut knob = lerp_color(muted(p), on(p.a0), t);
        if !enabled {
            fill = fill.gamma_multiply(0.45);
            knob = knob.gamma_multiply(0.5);
        }
        let painter = ui.painter();
        let r = rect.height() / 2.0;
        painter.rect(
            rect,
            CornerRadius::same(r as u8),
            fill,
            Stroke::new(
                1.0,
                if resp.hovered() {
                    c32(PRO_BLUE)
                } else {
                    border(p)
                },
            ),
            StrokeKind::Inside,
        );
        let x = egui::lerp((rect.left() + r)..=(rect.right() - r), t);
        painter.circle_filled(egui::pos2(x, rect.center().y), r - 4.0, knob);
    }
    resp
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

/// Restyle `ui` so text fields read as fields: a recessed fill, a visible
/// outline, rounded corners and a blue focus ring.
pub fn field_style(ui: &mut Ui, p: &Palette) {
    let v = ui.visuals_mut();
    v.extreme_bg_color = c32(p.bg);
    let w = &mut v.widgets;
    w.inactive.bg_stroke = Stroke::new(1.0, border(p));
    w.hovered.bg_stroke = Stroke::new(1.0, c32a(p.text3));
    w.active.bg_stroke = Stroke::new(1.0, c32(PRO_BLUE));
    for s in [&mut w.inactive, &mut w.hovered, &mut w.active] {
        s.corner_radius = CornerRadius::same(R);
    }
    v.selection.stroke = Stroke::new(1.5, c32(PRO_BLUE));
}

/// A single-line text field `width` wide.
pub fn text_field(ui: &mut Ui, p: &Palette, text: &mut String, hint: &str, width: f32) -> Response {
    ui.scope(|ui| {
        field_style(ui, p);
        ui.add(
            TextEdit::singleline(text)
                .hint_text(RichText::new(hint).color(c32a(p.text3)))
                .margin(Margin::symmetric(8, 5))
                .min_size(vec2(width, CONTROL_H))
                .desired_width(width - 16.0)
                .align(Align2::LEFT_CENTER),
        )
    })
    .inner
}

/// A secret (API key) field with a Show/Hide button. Edits a copy so that
/// merely showing it never turns "no key" into an empty key. Returns true
/// when the value changed.
pub fn secret_field(
    ui: &mut Ui,
    p: &Palette,
    val: &mut Option<String>,
    reveal: &mut bool,
    hint: &str,
    width: f32,
) -> bool {
    let mut s = val.clone().unwrap_or_default();
    // Right-to-left: the button first, then the field fills what's left.
    let btn_w = 58.0;
    if button(ui, p, if *reveal { "Hide" } else { "Show" })
        .on_hover_text(if *reveal {
            "Hide the key"
        } else {
            "Show the key (to check or copy it)"
        })
        .clicked()
    {
        *reveal = !*reveal;
    }
    let field_w = (width - btn_w - ui.spacing().item_spacing.x).max(80.0);
    let r = ui
        .scope(|ui| {
            field_style(ui, p);
            ui.add(
                TextEdit::singleline(&mut s)
                    .password(!*reveal)
                    .hint_text(RichText::new(hint).color(c32a(p.text3)))
                    .margin(Margin::symmetric(8, 5))
                    .min_size(vec2(field_w, CONTROL_H))
                    .desired_width(field_w - 16.0)
                    .align(Align2::LEFT_CENTER),
            )
        })
        .inner;
    let new = crate::settings::opt_string(&s);
    if r.changed() && new != *val {
        *val = new;
        return true;
    }
    false
}

fn styled_button(ui: &mut Ui, text: RichText, fill: Color32, stroke: Stroke) -> Response {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for s in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            s.corner_radius = CornerRadius::same(R);
        }
        w.inactive.weak_bg_fill = fill;
        w.inactive.bg_stroke = stroke;
        ui.add(
            egui::Button::new(text)
                .min_size(vec2(0.0, CONTROL_H))
                .corner_radius(CornerRadius::same(R)),
        )
    })
    .inner
}

/// A secondary (outlined) button.
pub fn button(ui: &mut Ui, p: &Palette, text: &str) -> Response {
    styled_button(
        ui,
        RichText::new(text)
            .font(saira(13.5, "medium"))
            .color(c32(p.text)),
        c32(p.surface3),
        Stroke::new(1.0, border(p)),
    )
}

/// The primary (filled accent) button.
pub fn primary_button(ui: &mut Ui, p: &Palette, text: &str) -> Response {
    styled_button(
        ui,
        RichText::new(text)
            .font(saira(13.5, "semibold"))
            .color(on(p.a0)),
        c32(p.a0),
        Stroke::new(1.0, c32(p.a0)),
    )
}

/// A combo box `width` wide over `opts`.
pub fn combo<T: Copy + PartialEq>(
    ui: &mut Ui,
    p: &Palette,
    id: &str,
    cur: &mut T,
    opts: &[(T, &str)],
    width: f32,
) -> bool {
    let text = opts
        .iter()
        .find(|(v, _)| v == cur)
        .map(|(_, l)| *l)
        .unwrap_or("?");
    let mut changed = false;
    ui.scope(|ui| {
        field_style(ui, p);
        let w = &mut ui.visuals_mut().widgets;
        w.inactive.weak_bg_fill = c32(p.bg);
        w.open.corner_radius = CornerRadius::same(R);
        ui.spacing_mut().interact_size.y = CONTROL_H;
        ui.spacing_mut().button_padding = vec2(9.0, 4.0);
        egui::ComboBox::from_id_salt(id)
            .selected_text(text)
            .width(width)
            .height(320.0)
            .show_ui(ui, |ui| {
                for (v, l) in opts {
                    changed |= ui.selectable_value(cur, *v, *l).changed();
                }
            });
    });
    changed
}

/// A status line: a coloured dot and text.
pub fn status(ui: &mut Ui, color: Color32, text_color: Color32, text: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(8.0, 16.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 3.5, color);
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .font(saira(12.5, "regular"))
                    .color(text_color),
            )
            .wrap(),
        );
    });
}

/// A full-width note block (info or warning) with a coloured edge.
pub fn callout(ui: &mut Ui, p: &Palette, accent: Color32, text: &str) {
    let r = egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            accent.r(),
            accent.g(),
            accent.b(),
            20,
        ))
        .corner_radius(CornerRadius::same(R))
        .inner_margin(Margin {
            left: 14,
            right: 10,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    RichText::new(text)
                        .font(saira(12.5, "regular"))
                        .color(muted(p)),
                )
                .wrap(),
            );
        })
        .response
        .rect;
    ui.painter().rect_filled(
        Rect::from_min_max(r.left_top(), egui::pos2(r.left() + 3.0, r.bottom())),
        CornerRadius {
            nw: R,
            sw: R,
            ne: 0,
            se: 0,
        },
        accent,
    );
}

/// A horizontal slider laid out left to right (value box after the rail)
/// even inside a right-to-left row.
pub fn slider(ui: &mut Ui, p: &Palette, slider: egui::Slider<'_>) -> Response {
    ui.scope_builder(
        UiBuilder::new().layout(Layout::left_to_right(Align::Center)),
        |ui| {
            field_style(ui, p);
            let v = ui.visuals_mut();
            v.selection.bg_fill = c32(p.a0);
            v.widgets.inactive.bg_fill = c32(p.surface3);
            ui.add(slider.trailing_fill(true))
        },
    )
    .inner
}

/// Vertical navigation: one item per entry, the selected one highlighted
/// with an accent bar. Returns the clicked index.
pub fn nav(ui: &mut Ui, p: &Palette, items: &[&str], selected: usize) -> Option<usize> {
    let mut clicked = None;
    for (i, label) in items.iter().enumerate() {
        let sel = i == selected;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
        let painter = ui.painter();
        if sel {
            painter.rect_filled(
                rect,
                CornerRadius::same(R),
                Color32::from_rgba_unmultiplied(p.a0[0], p.a0[1], p.a0[2], 30),
            );
            painter.rect_filled(
                Rect::from_min_size(rect.left_top() + vec2(0.0, 6.0), vec2(3.0, 20.0)),
                CornerRadius::same(1),
                c32(p.a0),
            );
        } else if resp.hovered() {
            painter.rect_filled(rect, CornerRadius::same(R), c32(p.surface3));
        }
        let color = if sel { c32(p.a0) } else { c32(p.text) };
        painter.text(
            rect.left_center() + vec2(14.0, 0.0),
            Align2::LEFT_CENTER,
            *label,
            saira(14.0, if sel { "medium" } else { "regular" }),
            color,
        );
        if resp.clicked() {
            clicked = Some(i);
        }
        ui.add_space(2.0);
    }
    clicked
}
