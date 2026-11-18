//! The docked workstation layout: pane stacks flush to the left and right,
//! the radar as the central viewport, centre panes in a bottom dock, the
//! command console beneath it and a tool strip on the far right.

use std::collections::HashMap;

use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Id, LayerId, Order, Rect, Sense, Stroke,
    StrokeKind, Ui, UiBuilder,
};
use ss_core::command::Command;
use ss_core::{Column, Layout, PanelKind, SavedLocation};

use crate::panels::{Panel, PanelCx};
use crate::state::{PanelRequests, Shared, WindowTarget};
use crate::tasks::Tasks;
use crate::theme::{c32, c32a, on, PRO_BAR, PRO_BLUE};

pub const TITLE_H: f32 = 24.0;
const MARGIN: f32 = 12.0;
/// Gap between docked panes (the black background shows through).
const DOCK_GAP: f32 = 4.0;
/// Width of the right-hand tool strip.
pub const STRIP_W: f32 = 46.0;
const MIN_SIDE_W: f32 = 240.0;
const CARD_ALPHA: u8 = 255;
/// Height of the radar backdrop's floating controls slot (bottom-left).
/// Sized for the two-row transport + layers box plus the dBZ legend.
pub const CONTROLS_H: f32 = 120.0;
const COLUMNS: [Column; 3] = [Column::Left, Column::Center, Column::Right];

#[derive(Default)]
pub struct LayoutState {
    dragging: Option<PanelKind>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutAction {
    Place {
        panel: PanelKind,
        col: Column,
        index: Option<usize>,
    },
    Hide(PanelKind),
    /// Undock into its own window (handled by the app, not the layout).
    Popout(PanelKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

pub fn apply_action(layout: &mut Layout, action: LayoutAction) {
    match action {
        LayoutAction::Place { panel, col, index } => layout.place(panel, col, index),
        LayoutAction::Hide(p) => layout.hide(p),
        LayoutAction::Popout(_) => {}
    }
}

/// The action a title-bar glyph button performs, or `None` when the move is
/// impossible (already at the edge).
pub fn neighbor_move(layout: &Layout, panel: PanelKind, dir: Dir) -> Option<LayoutAction> {
    let (col, idx) = layout.find(panel)?;
    let col_i = COLUMNS.iter().position(|c| *c == col)?;
    match dir {
        Dir::Left | Dir::Right => {
            let next = match dir {
                Dir::Left => col_i.checked_sub(1)?,
                _ => (col_i + 1 < COLUMNS.len()).then_some(col_i + 1)?,
            };
            Some(LayoutAction::Place {
                panel,
                col: COLUMNS[next],
                index: None,
            })
        }
        Dir::Up => (idx > 0).then(|| LayoutAction::Place {
            panel,
            col,
            index: Some(idx - 1),
        }),
        Dir::Down => (idx + 1 < layout.column(col).len()).then(|| LayoutAction::Place {
            panel,
            col,
            index: Some(idx + 1),
        }),
    }
}

/// Insertion index for a drop at height `y` among the other panels' rects.
pub fn drop_index(others: &[Rect], y: f32) -> usize {
    others.iter().filter(|r| r.center().y < y).count()
}

/// Terminal-style title bar: dark grey with a numbered amber badge and an
/// amber title; blue while hovered, amber (black text) while dragged.
/// Returns the colour glyphs on the bar should use.
fn paint_title_block(
    ui: &Ui,
    title: Rect,
    p: &ss_core::theme::Palette,
    text: &str,
    number: Option<usize>,
    hot: bool,
    dragging: bool,
) -> Color32 {
    let (fill, fg) = if dragging {
        (c32(p.a0), on(p.a0))
    } else if hot {
        (c32(PRO_BLUE), Color32::WHITE)
    } else {
        (c32(PRO_BAR), c32a(p.text2))
    };
    ui.painter().rect_filled(title, 0.0, fill);
    let mut x = title.left() + 6.0;
    if let Some(n) = number {
        let badge = Rect::from_min_size(
            pos2(title.left() + 3.0, title.top() + 3.0),
            vec2(18.0, title.height() - 6.0),
        );
        ui.painter().rect_filled(badge, 0.0, c32(p.a0));
        ui.painter().text(
            badge.center(),
            Align2::CENTER_CENTER,
            n.to_string(),
            FontId::monospace(11.0),
            on(p.a0),
        );
        x = badge.right() + 7.0;
    }
    ui.painter().text(
        pos2(x, title.center().y),
        Align2::LEFT_CENTER,
        text.to_uppercase(),
        FontId::monospace(11.5),
        fg,
    );
    fg
}

/// Pane body: opaque fill with a thin grey outline.
fn paint_card(ui: &Ui, rect: Rect, p: &ss_core::theme::Palette, fill: Color32) {
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter().rect_stroke(
        rect,
        0.0,
        Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(p.a0[0], p.a0[1], p.a0[2], 150),
        ),
        StrokeKind::Inside,
    );
}

/// A single full-size framed panel (as used by child windows): surface fill,
/// border, a [`TITLE_H`] title bar showing `title`, and a padded body.
pub fn panel_frame(
    ui: &mut Ui,
    p: &ss_core::theme::Palette,
    title: &str,
    add: impl FnOnce(&mut Ui),
) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    ui.painter().rect_filled(area, 0.0, c32(p.surface));
    ui.painter()
        .rect_stroke(area, 0.0, Stroke::new(1.5, c32(p.text)), StrokeKind::Inside);
    let bar = Rect::from_min_size(area.min, vec2(area.width(), TITLE_H));
    paint_title_block(ui, bar, p, title, None, false, false);
    let body = Rect::from_min_max(
        pos2(area.left() + 6.0, bar.bottom() + 4.0),
        pos2(area.right() - 6.0, area.bottom() - 4.0),
    );
    if body.width() > 8.0 && body.height() > 8.0 {
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt("window_body")
                .max_rect(body)
                .layout(egui::Layout::top_down(Align::Min)),
        );
        child.set_clip_rect(body.intersect(ui.clip_rect()));
        add(&mut child);
    }
}

/// Opaque fill of a docked pane.
pub fn card_fill(p: &ss_core::theme::Palette) -> Color32 {
    Color32::from_rgba_unmultiplied(p.surface[0], p.surface[1], p.surface[2], CARD_ALPHA)
}

/// Relative height weight of a pane in a side dock. The radar is the
/// central viewport, never a stacked pane.
pub fn card_height(kind: PanelKind) -> f32 {
    match kind {
        PanelKind::Locations => 340.0,
        PanelKind::Conditions => 330.0,
        PanelKind::Hourly => 128.0,
        PanelKind::Forecast => 250.0,
        PanelKind::AuditLog => 220.0,
        PanelKind::Radio => 200.0,
        PanelKind::Storms => 300.0,
        PanelKind::Sounding => 460.0,
        PanelKind::Radar => 0.0,
    }
}

/// Pane heights for one dock column: proportional to [`card_height`] and
/// filling `avail` exactly (gaps included), never below a title bar.
pub fn stack_heights(kinds: &[PanelKind], avail: f32) -> Vec<f32> {
    if kinds.is_empty() {
        return Vec::new();
    }
    let gaps = DOCK_GAP * (kinds.len() as f32 - 1.0);
    let weights: Vec<f32> = kinds.iter().map(|k| card_height(*k)).collect();
    let sum = weights.iter().sum::<f32>().max(1.0);
    let room = (avail - gaps).max(0.0);
    weights
        .iter()
        .map(|w| (w / sum * room).max(TITLE_H + 4.0))
        .collect()
}

/// Where everything in the main window's work area goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dock {
    /// Left pane stack (zero width when the column is empty).
    pub left: Rect,
    /// Right pane stack.
    pub right: Rect,
    /// The radar viewport.
    pub viewport: Rect,
    /// The bottom dock: centre-column panes side by side.
    pub bottom: Rect,
    /// The command console, full centre width, at the very bottom.
    pub console: Rect,
    /// The vertical tool strip on the far right.
    pub strip: Rect,
}

fn without_radar(layout: &Layout) -> Layout {
    let mut l = layout.clone();
    l.left.retain(|k| *k != PanelKind::Radar);
    l.center.retain(|k| *k != PanelKind::Radar);
    l.right.retain(|k| *k != PanelKind::Radar);
    l
}

/// Docked geometry: tool strip on the far right, side stacks flush to the
/// edges, and the centre split into viewport / bottom dock / console.
pub fn dock(area: Rect, layout: &Layout, console_h: f32) -> Dock {
    let nr = without_radar(layout);
    let strip = Rect::from_min_max(pos2(area.right() - STRIP_W, area.top()), area.max);
    let work = Rect::from_min_max(area.min, pos2(strip.left() - DOCK_GAP, area.bottom()));
    let side = |frac: f32, empty: bool| {
        if empty {
            0.0
        } else {
            let w = frac.clamp(0.05, 0.6) * work.width();
            if work.width() >= 700.0 {
                w.max(MIN_SIDE_W)
            } else {
                w
            }
        }
    };
    let l = side(layout.left_width_frac, nr.left.is_empty());
    let r = side(layout.right_width_frac, nr.right.is_empty());
    let left = Rect::from_min_max(work.min, pos2(work.left() + l, work.bottom()));
    let right = Rect::from_min_max(pos2(work.right() - r, work.top()), work.max);
    let cx0 = if l > 0.0 {
        left.right() + DOCK_GAP
    } else {
        work.left()
    };
    let cx1 = if r > 0.0 {
        right.left() - DOCK_GAP
    } else {
        work.right()
    };
    let cx1 = cx1.max(cx0);
    let console = Rect::from_min_max(
        pos2(cx0, work.bottom() - console_h),
        pos2(cx1, work.bottom()),
    );
    let bottom_h = if nr.center.is_empty() {
        0.0
    } else {
        (work.height() * 0.3).clamp(170.0, 320.0)
    };
    let bottom_bot = console.top() - DOCK_GAP;
    let bottom = Rect::from_min_max(pos2(cx0, bottom_bot - bottom_h), pos2(cx1, bottom_bot));
    let viewport_bot = if bottom_h > 0.0 {
        bottom.top() - DOCK_GAP
    } else {
        bottom_bot
    };
    let viewport = Rect::from_min_max(
        pos2(cx0, work.top()),
        pos2(cx1, viewport_bot.max(work.top())),
    );
    Dock {
        left,
        right,
        viewport,
        bottom,
        console,
        strip,
    }
}

/// Where the radar draws its transport/legend box: bottom-left of the viewport.
pub fn controls_slot(area: Rect, layout: &Layout, console_h: f32) -> Rect {
    let v = dock(area, layout, console_h).viewport;
    let w = v.width().min(500.0) - 2.0 * MARGIN;
    Rect::from_min_size(
        pos2(v.left() + MARGIN, v.bottom() - MARGIN - CONTROLS_H),
        vec2(w.max(0.0), CONTROLS_H),
    )
}

/// The command console strip under the bottom dock.
pub fn cmdbar_rect(area: Rect, layout: &Layout, height: f32) -> Rect {
    dock(area, layout, height).console
}

#[allow(clippy::too_many_arguments)]
fn run_panel(
    ui: &mut Ui,
    panels: &mut HashMap<PanelKind, Box<dyn Panel>>,
    kind: PanelKind,
    shared: &mut Shared,
    tasks: &Tasks,
    emit: &mut Vec<Command>,
    req: &mut PanelRequests,
    location: Option<&SavedLocation>,
    overlay: Option<Rect>,
) {
    if let Some(panel) = panels.get_mut(&kind) {
        let mut cx = PanelCx {
            palette: shared.palette,
            shared,
            tasks,
            emit,
            req,
            location: location.cloned(),
            window: WindowTarget::Main,
            overlay,
        };
        panel.ui(ui, &mut cx);
    }
}

/// Tool strip buttons: (label, panel it toggles).
const STRIP: [(&str, PanelKind); 9] = [
    ("LOC", PanelKind::Locations),
    ("RDR", PanelKind::Radar),
    ("NOW", PanelKind::Conditions),
    ("HRS", PanelKind::Hourly),
    ("7DY", PanelKind::Forecast),
    ("LOG", PanelKind::AuditLog),
    ("RAD", PanelKind::Radio),
    ("STM", PanelKind::Storms),
    ("SKW", PanelKind::Sounding),
];

/// The vertical tool strip: one block per pane; filled = docked.
fn tool_strip(
    ui: &mut Ui,
    strip: Rect,
    p: &ss_core::theme::Palette,
    layout: &Layout,
    emit: &mut Vec<Command>,
) {
    ui.painter().rect_filled(strip, 0.0, c32(p.surface));
    ui.painter()
        .vline(strip.left(), strip.y_range(), Stroke::new(1.5, c32(p.a0)));
    let mut y = strip.top() + 6.0;
    for (label, kind) in STRIP {
        let b = Rect::from_min_size(
            pos2(strip.left() + 5.0, y),
            vec2(strip.width() - 10.0, 34.0),
        );
        y += 40.0;
        if b.bottom() > strip.bottom() {
            break;
        }
        let docked = layout.is_docked(kind);
        let resp = ui
            .interact(b, Id::unique(("strip", kind.id())), Sense::click())
            .on_hover_text(format!(
                "{} {}",
                if docked { "Hide" } else { "Show" },
                kind.title()
            ));
        let (fill, fg) = match (resp.hovered(), docked) {
            (true, _) => (c32(PRO_BLUE), Color32::WHITE),
            (false, true) => (c32(p.a0), on(p.a0)),
            (false, false) => (Color32::TRANSPARENT, c32a(p.text3)),
        };
        ui.painter().rect_filled(b, 0.0, fill);
        if !docked && !resp.hovered() {
            ui.painter()
                .rect_stroke(b, 0.0, Stroke::new(1.0, c32a(p.border)), StrokeKind::Inside);
        }
        ui.painter().text(
            b.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(10.5),
            fg,
        );
        if resp.clicked() {
            let line = format!("{} {}", if docked { "hide" } else { "show" }, kind.id());
            if let Ok(c) = ss_core::command::parse(&line) {
                emit.push(c);
            }
        }
    }
}

/// One docked pane: frame, title block with glyph buttons, and its body.
/// Returns (title response, whether a glyph button was hit).
#[allow(clippy::too_many_arguments)]
fn dock_pane(
    ui: &mut Ui,
    prect: Rect,
    kind: PanelKind,
    state: &mut LayoutState,
    layout: &Layout,
    p: &ss_core::theme::Palette,
    actions: &mut Vec<LayoutAction>,
    drop: &mut Option<(PanelKind, egui::Pos2)>,
) -> Rect {
    paint_card(ui, prect, p, card_fill(p));
    // Empty pane space must block the map underneath.
    ui.interact(
        prect,
        Id::unique(("card", kind.id())),
        Sense::click_and_drag(),
    );

    let title = Rect::from_min_size(prect.min, vec2(prect.width(), TITLE_H));
    let tid = Id::unique(("panel_title", kind.id()));
    let tresp = ui.interact(title, tid, Sense::click_and_drag());
    let dragging_me = state.dragging == Some(kind);
    let hot = ui.rect_contains_pointer(title) || dragging_me;
    let number = PanelKind::ALL
        .iter()
        .position(|k| *k == kind)
        .map(|i| i + 1);
    let glyph_fg = paint_title_block(ui, title, p, kind.title(), number, hot, dragging_me);

    // Double-click a title to undock the pane into its own window.
    if tresp.double_clicked() {
        actions.push(LayoutAction::Popout(kind));
    }
    if tresp.drag_started() {
        state.dragging = Some(kind);
    }
    if tresp.drag_stopped() {
        state.dragging = None;
        if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
            *drop = Some((kind, pos));
        }
    }

    if hot && state.dragging.is_none() {
        let buttons: [(&str, Option<LayoutAction>); 6] = [
            ("×", Some(LayoutAction::Hide(kind))),
            ("↗", Some(LayoutAction::Popout(kind))),
            ("▾", neighbor_move(layout, kind, Dir::Down)),
            ("▴", neighbor_move(layout, kind, Dir::Up)),
            ("▸", neighbor_move(layout, kind, Dir::Right)),
            ("◂", neighbor_move(layout, kind, Dir::Left)),
        ];
        let mut x = title.right() - 2.0;
        for (glyph, action) in buttons {
            let Some(action) = action else { continue };
            let b = Rect::from_min_size(pos2(x - 20.0, title.top()), vec2(20.0, TITLE_H));
            x -= 20.0;
            let tip = match glyph {
                "×" => "Hide pane",
                "↗" => "Undock into its own window (or double-click the title)",
                _ => "Move pane",
            };
            let bresp = ui
                .interact(b, tid.with(glyph), Sense::click())
                .on_hover_text(tip);
            if bresp.hovered() {
                let fill = if glyph == "×" { p.danger } else { PRO_BLUE };
                ui.painter().rect_filled(b, 0.0, c32(fill));
            }
            let col = if bresp.hovered() {
                Color32::WHITE
            } else {
                glyph_fg
            };
            ui.painter().text(
                b.center(),
                Align2::CENTER_CENTER,
                glyph,
                FontId::monospace(13.0),
                col,
            );
            if bresp.clicked() {
                actions.push(action);
            }
        }
    }

    Rect::from_min_max(
        pos2(prect.left() + 7.0, title.bottom() + 5.0),
        pos2(prect.right() - 7.0, prect.bottom() - 5.0),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    state: &mut LayoutState,
    shared: &mut Shared,
    panels: &mut HashMap<PanelKind, Box<dyn Panel>>,
    tasks: &Tasks,
    emit: &mut Vec<Command>,
    req: &mut PanelRequests,
    location: Option<&SavedLocation>,
    console_h: f32,
) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let layout = shared.layout.clone();
    let p = shared.palette;
    let d = dock(area, &layout, console_h);

    // --- radar viewport ---
    if layout.find(PanelKind::Radar).is_some() && d.viewport.height() > 40.0 {
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt("radar_viewport")
                .max_rect(d.viewport)
                .layout(egui::Layout::top_down(Align::Min)),
        );
        child.set_clip_rect(d.viewport.intersect(ui.clip_rect()));
        run_panel(
            &mut child,
            panels,
            PanelKind::Radar,
            shared,
            tasks,
            emit,
            req,
            location,
            Some(controls_slot(area, &layout, console_h)),
        );
        // Viewport frame: grey outline with an amber function tag.
        ui.painter().rect_stroke(
            d.viewport,
            0.0,
            Stroke::new(1.0, c32a(p.border)),
            StrokeKind::Inside,
        );
        let tab_text = "RADR <GO>  COMPOSITE RADAR";
        let galley =
            ui.painter()
                .layout_no_wrap(tab_text.into(), FontId::monospace(10.5), on(p.a0));
        let tab = Rect::from_min_size(d.viewport.min, galley.size() + vec2(14.0, 6.0));
        ui.painter().rect_filled(tab, 0.0, c32(p.a0));
        ui.painter()
            .galley(tab.min + vec2(7.0, 3.0), galley, on(p.a0));
    } else if d.viewport.height() > 40.0 {
        ui.painter().rect_filled(d.viewport, 0.0, c32(p.bg));
        ui.painter().rect_stroke(
            d.viewport,
            0.0,
            Stroke::new(1.0, c32a(p.border)),
            StrokeKind::Inside,
        );
        ui.painter().text(
            d.viewport.center(),
            Align2::CENTER_CENTER,
            "NO VIEWPORT — :show radar",
            FontId::monospace(12.0),
            c32a(p.text3),
        );
    }

    tool_strip(ui, d.strip, &p, &layout, emit);

    let mut actions: Vec<LayoutAction> = Vec::new();
    // (panel, column, rect) as drawn this frame, for drop targeting.
    let mut drawn: Vec<(PanelKind, Column, Rect)> = Vec::new();
    let mut drop: Option<(PanelKind, egui::Pos2)> = None;

    for col in COLUMNS {
        let kinds: Vec<PanelKind> = layout
            .column(col)
            .iter()
            .copied()
            .filter(|k| *k != PanelKind::Radar)
            .collect();
        if kinds.is_empty() {
            continue;
        }
        let rects: Vec<Rect> = match col {
            Column::Left | Column::Right => {
                let r = if col == Column::Left { d.left } else { d.right };
                if r.width() < 1.0 {
                    continue;
                }
                let mut y = r.top();
                stack_heights(&kinds, r.height())
                    .into_iter()
                    .map(|h| {
                        let pr = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), h));
                        y += h + DOCK_GAP;
                        pr
                    })
                    .collect()
            }
            Column::Center => {
                let r = d.bottom;
                if r.height() < 1.0 {
                    continue;
                }
                let n = kinds.len() as f32;
                let w = (r.width() - DOCK_GAP * (n - 1.0)) / n;
                (0..kinds.len())
                    .map(|i| {
                        Rect::from_min_size(
                            pos2(r.left() + i as f32 * (w + DOCK_GAP), r.top()),
                            vec2(w, r.height()),
                        )
                    })
                    .collect()
            }
        };
        for (kind, prect) in kinds.iter().zip(rects) {
            drawn.push((*kind, col, prect));
            let body = dock_pane(
                ui,
                prect,
                *kind,
                state,
                &layout,
                &p,
                &mut actions,
                &mut drop,
            );
            if body.width() > 8.0 && body.height() > 8.0 {
                let mut child = ui.new_child(
                    UiBuilder::new()
                        .id_salt(("panel_body", kind.id()))
                        .max_rect(body)
                        .layout(egui::Layout::top_down(Align::Min)),
                );
                child.set_clip_rect(body.intersect(ui.clip_rect()));
                run_panel(
                    &mut child, panels, *kind, shared, tasks, emit, req, location, None,
                );
            }
        }
    }

    // Drop targets: the left dock, the centre (viewport + bottom dock), the right dock.
    let targets = [
        if d.left.width() >= 1.0 {
            d.left
        } else {
            Rect::from_min_size(area.min, vec2(MIN_SIDE_W, area.height()))
        },
        Rect::from_min_max(d.viewport.min, d.bottom.max.max(d.viewport.max)),
        if d.right.width() >= 1.0 {
            d.right
        } else {
            Rect::from_min_max(
                pos2(d.strip.left() - MIN_SIDE_W, area.top()),
                pos2(d.strip.left(), area.bottom()),
            )
        },
    ];

    // --- drag feedback ---
    if let Some(kind) = state.dragging {
        if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
            let painter = ui
                .ctx()
                .layer_painter(LayerId::new(Order::Tooltip, Id::unique("panel_drag")));
            for r in targets {
                let over = r.contains(pos);
                let a = if over { 48 } else { 14 };
                painter.rect_filled(
                    r,
                    0.0,
                    Color32::from_rgba_unmultiplied(p.a0[0], p.a0[1], p.a0[2], a),
                );
                let stroke = if over {
                    Stroke::new(2.0, c32(PRO_BLUE))
                } else {
                    Stroke::new(1.0, c32a([p.a0[0], p.a0[1], p.a0[2], 90]))
                };
                painter.rect_stroke(r, 0.0, stroke, StrokeKind::Inside);
            }
            let label = kind.title().to_uppercase();
            let galley = painter.layout_no_wrap(label, FontId::monospace(12.0), Color32::WHITE);
            let tag = Rect::from_min_size(pos + vec2(12.0, 12.0), galley.size() + vec2(12.0, 6.0));
            painter.rect_filled(tag, 0.0, c32(PRO_BLUE));
            painter.galley(tag.min + vec2(6.0, 3.0), galley, Color32::WHITE);
            ui.ctx().request_repaint();
        }
    }

    // --- resolve a drop ---
    if let Some((kind, pos)) = drop {
        let ci = (0..3)
            .find(|i| targets[*i].contains(pos))
            .unwrap_or(if pos.x < area.center().x { 0 } else { 2 });
        let col = COLUMNS[ci];
        let others: Vec<Rect> = drawn
            .iter()
            .filter(|(k, c, _)| *c == col && *k != kind)
            .map(|(_, _, r)| *r)
            .collect();
        let index = if col == Column::Center {
            // The bottom dock is a row: order by x.
            let i = others.iter().filter(|r| r.center().x < pos.x).count();
            // Index counts the radar, which stays first in the centre list.
            i + usize::from(
                layout
                    .find(PanelKind::Radar)
                    .is_some_and(|(c, _)| c == Column::Center),
            )
        } else {
            drop_index(&others, pos.y)
        };
        if layout.find(kind) != Some((col, index)) {
            actions.push(LayoutAction::Place {
                panel: kind,
                col,
                index: Some(index),
            });
        }
    }

    if !actions.is_empty() {
        for a in actions {
            if let LayoutAction::Popout(kind) = a {
                emit.push(Command::Popout(kind));
                continue;
            }
            apply_action(&mut shared.layout, a);
        }
        shared.dirty_session = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::default()
    }

    #[test]
    fn neighbor_moves_respect_edges() {
        let l = layout();
        // Locations is alone in the left column.
        assert_eq!(neighbor_move(&l, PanelKind::Locations, Dir::Left), None);
        assert_eq!(neighbor_move(&l, PanelKind::Locations, Dir::Up), None);
        assert_eq!(
            neighbor_move(&l, PanelKind::Locations, Dir::Right),
            Some(LayoutAction::Place {
                panel: PanelKind::Locations,
                col: Column::Center,
                index: None
            })
        );
        // Radar is first in centre; AuditLog is last.
        assert_eq!(neighbor_move(&l, PanelKind::Radar, Dir::Up), None);
        assert!(neighbor_move(&l, PanelKind::Radar, Dir::Down).is_some());
        assert_eq!(neighbor_move(&l, PanelKind::AuditLog, Dir::Down), None);
    }

    #[test]
    fn applying_actions_updates_layout() {
        let mut l = layout();
        let a = neighbor_move(&l, PanelKind::Radar, Dir::Down).unwrap();
        apply_action(&mut l, a);
        assert_eq!(l.center, vec![PanelKind::AuditLog, PanelKind::Radar]);
        apply_action(&mut l, LayoutAction::Hide(PanelKind::Radar));
        assert!(!l.is_docked(PanelKind::Radar));
        assert!(l.hidden.contains(&PanelKind::Radar));
    }

    #[test]
    fn drop_index_counts_panels_above_pointer() {
        let rects = [
            Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 100.0)),
            Rect::from_min_max(pos2(0.0, 100.0), pos2(10.0, 200.0)),
        ];
        assert_eq!(drop_index(&rects, 10.0), 0);
        assert_eq!(drop_index(&rects, 120.0), 1);
        assert_eq!(drop_index(&rects, 500.0), 2);
        assert_eq!(drop_index(&[], 5.0), 0);
    }

    #[test]
    fn stack_heights_fill_the_column_proportionally() {
        let kinds = [
            PanelKind::Locations,
            PanelKind::Conditions,
            PanelKind::Forecast,
        ];
        let avail = 900.0;
        let hs = stack_heights(&kinds, avail);
        let total = hs.iter().sum::<f32>() + DOCK_GAP * 2.0;
        assert!((total - avail).abs() < 0.01);
        assert!((hs[0] / 340.0 - hs[1] / 330.0).abs() < 0.001);
        let tiny = stack_heights(&kinds, 10.0);
        assert!(tiny.iter().all(|h| *h >= TITLE_H + 4.0));
        assert!(stack_heights(&[], 100.0).is_empty());
    }

    #[test]
    fn dock_regions_tile_the_area() {
        let area = Rect::from_min_max(pos2(0.0, 40.0), pos2(1400.0, 900.0));
        let l = layout();
        let d = dock(area, &l, 60.0);
        for r in [d.left, d.right, d.viewport, d.bottom, d.console, d.strip] {
            assert!(area.expand(0.01).contains_rect(r), "{r:?}");
        }
        // Side docks are flush to the edges, the strip is far right.
        assert_eq!(d.left.left(), area.left());
        assert_eq!(d.strip.right(), area.right());
        assert!((d.strip.width() - STRIP_W).abs() < 0.01);
        assert!(d.right.right() <= d.strip.left());
        // Centre stack: viewport above the bottom dock above the console.
        assert!(d.viewport.bottom() <= d.bottom.top());
        assert!(d.bottom.bottom() <= d.console.top());
        assert!((d.console.bottom() - area.bottom()).abs() < 0.01);
        assert!((d.console.height() - 60.0).abs() < 0.01);
        assert!(d.left.right() < d.viewport.left() && d.viewport.right() < d.right.left());
        assert_eq!(cmdbar_rect(area, &l, 60.0), d.console);
    }

    #[test]
    fn empty_columns_collapse() {
        let area = Rect::from_min_max(pos2(0.0, 0.0), pos2(1400.0, 900.0));
        let mut l = layout();
        l.left.clear();
        l.center.retain(|k| *k == PanelKind::Radar);
        let d = dock(area, &l, 50.0);
        assert_eq!(d.left.width(), 0.0);
        assert_eq!(d.viewport.left(), area.left());
        assert_eq!(d.bottom.height(), 0.0);
        assert!((d.viewport.bottom() - (d.console.top() - DOCK_GAP)).abs() < 0.01);
    }

    #[test]
    fn controls_slot_sits_in_the_viewport() {
        let area = Rect::from_min_max(pos2(0.0, 40.0), pos2(1400.0, 900.0));
        let l = layout();
        let s = controls_slot(area, &l, 80.0);
        let v = dock(area, &l, 80.0).viewport;
        assert!(v.contains_rect(s));
        assert!((s.height() - CONTROLS_H).abs() < 0.01);
    }

    #[test]
    fn card_height_zero_only_for_radar() {
        for k in PanelKind::ALL {
            assert_eq!(card_height(k) == 0.0, k == PanelKind::Radar);
        }
    }
}
