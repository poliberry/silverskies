//! Fetching placefiles and their icon sheets.

use std::collections::HashSet;

use ss_core::placefile::{parse, resolve_url, Placefile};

use crate::{Net, NetError, Result};

/// A decoded icon sheet: RGBA pixels.
pub struct SheetImage {
    pub url: String,
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
    /// The sheet had no alpha channel: black was made transparent and the
    /// symbols are drawn in the placefile's colour (GRLevelX's convention).
    pub tint: bool,
}

/// Decode an icon sheet. Without an alpha channel, brightness becomes
/// alpha (black = transparent) over white.
pub fn decode_sheet(bytes: &[u8]) -> Option<([usize; 2], Vec<u8>, bool)> {
    let img = image::load_from_memory(bytes).ok()?;
    let size = [img.width() as usize, img.height() as usize];
    if img.color().has_alpha() {
        return Some((size, img.to_rgba8().into_raw(), false));
    }
    let rgb = img.to_rgb8();
    let rgba = rgb
        .pixels()
        .flat_map(|p| {
            let a = p.0[0].max(p.0[1]).max(p.0[2]);
            [255, 255, 255, a]
        })
        .collect();
    Some((size, rgba, true))
}

impl Net {
    /// Read a placefile, and the icon sheets it uses that aren't in
    /// `have` (by URL). Text that isn't UTF-8 (common: Latin-1 names) is
    /// read leniently.
    pub async fn placefile(
        &self,
        url: &str,
        have: &HashSet<String>,
    ) -> Result<(Placefile, Vec<SheetImage>)> {
        let bytes = self.get_bytes(url, None).await?;
        let text = String::from_utf8_lossy(&bytes);
        if text.trim_start().starts_with('<') {
            return Err(NetError::Service(
                "not a placefile (the server sent a web page)".into(),
            ));
        }
        let mut pf = parse(&text);
        for f in &mut pf.icon_files {
            f.url = resolve_url(url, &f.url);
        }
        let mut sheets = Vec::new();
        let wanted: HashSet<String> = pf
            .icon_files
            .iter()
            .map(|f| f.url.clone())
            .filter(|u| !have.contains(u))
            .collect();
        for u in wanted {
            match self.get_bytes(&u, None).await {
                Ok(b) => match decode_sheet(&b) {
                    Some((size, rgba, tint)) => sheets.push(SheetImage {
                        url: u,
                        size,
                        rgba,
                        tint,
                    }),
                    None => tracing::warn!("icon sheet {u}: not an image"),
                },
                Err(e) => tracing::warn!("icon sheet {u}: {e}"),
            }
        }
        Ok((pf, sheets))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_without_alpha_key_out_black() {
        let mut img = image::RgbImage::new(2, 1);
        img.put_pixel(1, 0, image::Rgb([255, 255, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let (size, rgba, tint) = decode_sheet(&png).unwrap();
        assert_eq!(size, [2, 1]);
        assert!(tint);
        assert_eq!(rgba, vec![255, 255, 255, 0, 255, 255, 255, 255]);
    }
}
