//! Baseline-aware text placement for egui.
//!
//! egui lines widgets up by the centre of their boxes, and a horizontal row
//! centres each item as it is added, so text of different sizes (or text
//! next to a taller widget) ends up visibly off-line. These helpers place
//! text by its baseline instead.

use egui::{pos2, Align, Color32, FontId, Galley, Painter, Rect};

/// Distance from a galley's top to its first line's baseline.
pub fn galley_baseline(g: &Galley) -> f32 {
    g.rows
        .first()
        .map(|r| {
            r.pos.y
                + r.row
                    .glyphs
                    .first()
                    .map_or(r.row.size.y * 0.8, |gl| gl.pos.y)
        })
        .unwrap_or(g.size().y * 0.8)
}

/// Paint one line of text with its baseline at `y`, anchored at `x` by
/// `align` (left, centre or right). Returns the text's rect.
pub fn text_on_baseline(
    painter: &Painter,
    x: f32,
    y: f32,
    align: Align,
    text: impl ToString,
    font: FontId,
    color: Color32,
) -> Rect {
    let g = painter.layout_no_wrap(text.to_string(), font, color);
    let w = g.size().x;
    let left = match align {
        Align::Min => x,
        Align::Center => x - w / 2.0,
        Align::Max => x - w,
    };
    let r = Rect::from_min_size(pos2(left, y - galley_baseline(&g)), g.size());
    painter.galley(r.min, g, color);
    r
}

/// The baseline that visually centres text of `size` points on `center_y`
/// (half the cap height below the centre).
pub fn baseline_for_center(center_y: f32, size: f32) -> f32 {
    center_y + size * 0.36
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_sits_inside_the_first_line() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let g = ui.painter().layout_no_wrap(
                "Ag".into(),
                FontId::proportional(20.0),
                Color32::WHITE,
            );
            let b = galley_baseline(&g);
            assert!(
                b > g.size().y * 0.5 && b < g.size().y,
                "{b} / {}",
                g.size().y
            );
        });
        out.textures_delta.clear();
    }

    #[test]
    fn centred_baseline_is_below_centre() {
        assert!(baseline_for_center(100.0, 20.0) > 100.0);
    }
}
