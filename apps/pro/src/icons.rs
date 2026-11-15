//! Pro brand marks and window icon; the weather icon machinery lives in the
//! shared `ss-assets` crate.

use egui::{Context, TextureHandle};

pub use ss_assets::weather_image;

const LOGOMARK_DARK: &str = include_str!("../assets/brand/logomark-dark.svg");
const LOGOMARK_LIGHT: &str = include_str!("../assets/brand/logomark-light.svg");

/// The Silver Skies Pro logomark. `dark` = for a dark background (white
/// wordmark), otherwise the black-on-light variant.
pub fn logo_texture(ctx: &Context, dark: bool, height_pt: u32) -> TextureHandle {
    let (key, svg) = if dark {
        ("pro-logomark-dark", LOGOMARK_DARK)
    } else {
        ("pro-logomark-light", LOGOMARK_LIGHT)
    };
    ss_assets::svg_texture(ctx, key, svg, height_pt)
}

const ICON_DARK: &[u8] = include_bytes!("../assets/icon-dark.png");
const ICON_LIGHT: &[u8] = include_bytes!("../assets/icon-light.png");

/// Window icon for the given system theme (dark theme uses the dark icon).
pub fn app_icon(dark: bool) -> std::sync::Arc<egui::IconData> {
    ss_assets::icon_data(if dark { ICON_DARK } else { ICON_LIGHT })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logomarks_rasterize() {
        for svg in [LOGOMARK_DARK, LOGOMARK_LIGHT] {
            let img = ss_assets::rasterize(svg, 212, 60).unwrap();
            assert!(img.pixels.iter().any(|c| c.a() > 0));
        }
    }

    #[test]
    fn app_icons_decode() {
        for dark in [true, false] {
            let i = app_icon(dark);
            assert!(i.width > 1 && i.height > 1);
            assert_eq!(i.rgba.len(), (i.width * i.height * 4) as usize);
        }
    }
}
