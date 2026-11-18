//! Severe-storm overlay: storm cells with threat symbols and projected
//! tracks, and lettered storm-report markers, with hover hit-testing.

use std::any::Any;

use egui::{vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke};

use crate::layers::{DrawCtx, Layer};
use crate::viewport::Viewport;

/// What a mark represents (drives its symbol).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StormShape {
    /// Tornado vortex signature: inverted triangle, pulsing.
    Tornado,
    /// Mesocyclone: double ring.
    Meso,
    /// Severe hail: diamond.
    Hail,
    /// Strong (≥ 50 dBZ) cell: filled circle.
    Strong,
    /// Ordinary cell: small dot, only when zoomed in.
    Cell,
    /// A storm report: lettered disc.
    Report(&'static str),
    /// A tropical cyclone centre: the classic two-armed symbol.
    Cyclone,
}

/// A line from one lon/lat to another; `true` draws it dashed.
pub type Segment = ((f64, f64), (f64, f64), bool);

#[derive(Debug, Clone)]
pub struct StormMark {
    pub id: String,
    pub lon: f64,
    pub lat: f64,
    pub color: Color32,
    pub shape: StormShape,
    /// Projected positions (e.g. +15/30/45/60 min), drawn as a dashed track.
    pub track: Vec<(f64, f64)>,
    /// Extra line segments (from, to, dashed): e.g. a cyclone's past track
    /// (solid) and forecast track (dashed).
    pub segments: Vec<Segment>,
    /// Hover text.
    pub tooltip: String,
}

pub struct StormLayer {
    pub name: String,
    pub marks: Vec<StormMark>,
    pub visible: bool,
}

/// Ordinary cells and their tracks only show from this zoom.
const DETAIL_ZOOM: f64 = 6.0;

impl StormLayer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            marks: Vec::new(),
            visible: true,
        }
    }

    fn shown(&self, m: &StormMark, zoom: f64) -> bool {
        !matches!(m.shape, StormShape::Cell) || zoom >= DETAIL_ZOOM
    }

    /// The mark nearest `pos` within `radius_px`, if any.
    pub fn hit_test(
        &self,
        pos: Pos2,
        radius_px: f32,
        viewport: &Viewport,
        rect: Rect,
    ) -> Option<&StormMark> {
        if !self.visible {
            return None;
        }
        self.marks
            .iter()
            .filter(|m| self.shown(m, viewport.zoom))
            .map(|m| {
                (
                    (viewport.screen_from_lonlat(m.lon, m.lat, rect) - pos).length(),
                    m,
                )
            })
            .filter(|(d, _)| *d <= radius_px)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, m)| m)
    }
}

impl Layer for StormLayer {
    fn name(&self) -> &str {
        &self.name
    }
    fn visible(&self) -> bool {
        self.visible
    }
    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn draw(&mut self, ctx: &mut DrawCtx, painter: &Painter) {
        let zoom = ctx.viewport.zoom;
        let outline = Color32::from_black_alpha(if ctx.dark { 220 } else { 160 });
        let clip = ctx.rect.expand(40.0);
        let mut pulsing = false;
        for m in &self.marks {
            if !self.shown(m, zoom) {
                continue;
            }
            let c = ctx.viewport.screen_from_lonlat(m.lon, m.lat, ctx.rect);
            for (a, b, dashed) in &m.segments {
                let pa = ctx.viewport.screen_from_lonlat(a.0, a.1, ctx.rect);
                let pb = ctx.viewport.screen_from_lonlat(b.0, b.1, ctx.rect);
                let col = m.color.gamma_multiply(if *dashed { 0.85 } else { 1.0 });
                if *dashed {
                    painter.add(Shape::dashed_line(
                        &[pa, pb],
                        Stroke::new(2.0, col),
                        6.0,
                        4.0,
                    ));
                } else {
                    painter.line_segment([pa, pb], Stroke::new(2.4, col));
                }
                painter.circle_filled(pb, 2.5, col);
            }
            if !clip.contains(c) {
                continue;
            }
            // Track: dashed line through the projected points, ticks at each,
            // arrowhead at the end. Minor cells only when zoomed in.
            let major = !matches!(m.shape, StormShape::Cell | StormShape::Strong);
            if !m.track.is_empty() && (major || zoom >= DETAIL_ZOOM) {
                let mut pts = vec![c];
                pts.extend(
                    m.track
                        .iter()
                        .map(|(lo, la)| ctx.viewport.screen_from_lonlat(*lo, *la, ctx.rect)),
                );
                let col = m.color.gamma_multiply(0.9);
                painter.add(Shape::dashed_line(&pts, Stroke::new(1.6, col), 5.0, 3.0));
                for (i, p) in pts.iter().enumerate().skip(1) {
                    let prev = pts[i - 1];
                    let d = (*p - prev).normalized();
                    let n = vec2(-d.y, d.x);
                    if i + 1 == pts.len() {
                        painter.add(Shape::convex_polygon(
                            vec![*p + d * 6.0, *p + n * 4.0, *p - n * 4.0],
                            col,
                            Stroke::NONE,
                        ));
                    } else {
                        painter.line_segment([*p - n * 3.5, *p + n * 3.5], Stroke::new(1.4, col));
                    }
                }
            }
            match m.shape {
                StormShape::Tornado => {
                    pulsing = true;
                    let t = (ctx.time * 3.0).sin() as f32 * 0.5 + 0.5;
                    painter.circle_stroke(
                        c,
                        12.0 + 6.0 * t,
                        Stroke::new(2.0, m.color.gamma_multiply(1.0 - t * 0.8)),
                    );
                    let tri = vec![
                        c + vec2(-8.0, -6.0),
                        c + vec2(8.0, -6.0),
                        c + vec2(0.0, 8.0),
                    ];
                    painter.add(Shape::convex_polygon(
                        tri,
                        m.color,
                        Stroke::new(1.5, outline),
                    ));
                }
                StormShape::Meso => {
                    painter.circle_stroke(c, 8.0, Stroke::new(3.0, outline));
                    painter.circle_stroke(c, 8.0, Stroke::new(2.0, m.color));
                    painter.circle_stroke(c, 4.0, Stroke::new(1.5, m.color));
                }
                StormShape::Hail => {
                    let d = vec![
                        c + vec2(0.0, -7.0),
                        c + vec2(7.0, 0.0),
                        c + vec2(0.0, 7.0),
                        c + vec2(-7.0, 0.0),
                    ];
                    painter.add(Shape::convex_polygon(d, m.color, Stroke::new(1.5, outline)));
                }
                StormShape::Strong => {
                    painter.circle_filled(c, 4.5, m.color);
                    painter.circle_stroke(c, 4.5, Stroke::new(1.0, outline));
                }
                StormShape::Cell => {
                    painter.circle_filled(c, 2.5, m.color);
                }
                StormShape::Cyclone => {
                    // Centre disc with two curved arms (the TC map symbol).
                    let r = 9.0;
                    let arm = |sign: f32| -> Vec<Pos2> {
                        (0..=10)
                            .map(|i| {
                                let t = i as f32 / 10.0;
                                let a = sign * std::f32::consts::PI * (0.5 + t * 0.9);
                                let rr = r * (0.6 + 1.1 * t);
                                c + vec2(a.cos() * rr * sign, a.sin() * rr)
                            })
                            .collect()
                    };
                    for sign in [1.0, -1.0] {
                        painter.add(Shape::line(arm(sign), Stroke::new(4.0, outline)));
                        painter.add(Shape::line(arm(sign), Stroke::new(2.5, m.color)));
                    }
                    painter.circle_filled(c, r * 0.6, m.color);
                    painter.circle_stroke(c, r * 0.6, Stroke::new(1.5, outline));
                    painter.circle_filled(c, r * 0.22, outline);
                }
                StormShape::Report(letter) => {
                    painter.circle_filled(c, 7.0, m.color);
                    painter.circle_stroke(c, 7.0, Stroke::new(1.0, outline));
                    painter.text(
                        c,
                        Align2::CENTER_CENTER,
                        letter,
                        FontId::monospace(9.5),
                        Color32::from_gray(15),
                    );
                }
            }
        }
        if pulsing {
            painter.ctx().request_repaint();
        }
    }
}
