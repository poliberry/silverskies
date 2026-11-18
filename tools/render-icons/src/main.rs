//! Regenerates the PNG app icons from the brand SVGs:
//!
//! ```text
//! cargo run -p render-icons
//! ```
//!
//! Writes `apps/<app>/assets/icon-dark.png` (white mark, for dark themes)
//! and `icon-light.png` (black mark, for light themes) at 512x512 for every
//! app, on a transparent background: the iconmark itself, not a tile. The
//! PNGs are committed so packaging never needs to run this.

use std::path::{Path, PathBuf};

use resvg::{tiny_skia, usvg};

const SIZE: u32 = 512;

/// Recolour the mark's flat fills/strokes; the embedded gradient image is kept.
fn recolor(svg: &str, mark: &str) -> String {
    svg.replace("fill=\"white\"", &format!("fill=\"{mark}\""))
        .replace("fill=\"black\"", &format!("fill=\"{mark}\""))
        .replace("stroke=\"white\"", &format!("stroke=\"{mark}\""))
        .replace("stroke=\"black\"", &format!("stroke=\"{mark}\""))
}

fn render(svg: &str, out: &Path) -> anyhow::Result<()> {
    let tree = usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default())?;
    let size = tree.size();
    // A little breathing room around the mark.
    let inner = SIZE as f32 * 0.86;
    let scale = (inner / size.width()).min(inner / size.height());
    let dx = (SIZE as f32 - size.width() * scale) / 2.0;
    let dy = (SIZE as f32 - size.height() * scale) / 2.0;
    // A new pixmap is fully transparent: no background plate.
    let mut pixmap = tiny_skia::Pixmap::new(SIZE, SIZE).ok_or_else(|| anyhow::anyhow!("pixmap"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy),
        &mut pixmap.as_mut(),
    );
    pixmap.save_png(out)?;
    println!("wrote {}", out.display());
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    // (app dir, dark-background mark, light-background mark)
    let apps = [
        ("apps/silverskies", "iconmark.svg", "iconmark.svg"),
        ("apps/pro", "iconmark-dark.svg", "iconmark-light.svg"),
    ];
    for (app, dark_src, light_src) in apps {
        let dir = root.join(app).join("assets");
        let dark = std::fs::read_to_string(dir.join("brand").join(dark_src))?;
        let light = std::fs::read_to_string(dir.join("brand").join(light_src))?;
        render(&recolor(&dark, "#ffffff"), &dir.join("icon-dark.png"))?;
        render(&recolor(&light, "#000000"), &dir.join("icon-light.png"))?;
    }
    Ok(())
}
