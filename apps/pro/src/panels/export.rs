//! Radar loop export: step through the loop's frames, wait for each to
//! load, screenshot the map, then write an animated GIF.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender};
use egui::{ColorImage, Rect};
use image::RgbaImage;

/// Widest frame written, pixels (keeps files a sensible size).
const MAX_W: u32 = 960;
/// Each frame is shown this long, the last one longer.
const FRAME_MS: u32 = 500;
const LAST_MS: u32 = 1800;
/// Frames drawn after a frame's tiles are in, before it's captured (lets
/// fades and labels settle).
const SETTLE_FRAMES: u8 = 4;
/// Give up waiting for a frame's tiles after this long, seconds.
const MAX_WAIT_S: f32 = 10.0;

/// What the panel should do next frame.
pub enum Step {
    /// Show this loop frame (and keep repainting).
    Show(usize),
    /// All frames captured: the file is being written.
    Writing,
}

pub struct LoopExport {
    pub frames: usize,
    pub i: usize,
    settle: u8,
    since: Instant,
    rx: Option<Receiver<Arc<ColorImage>>>,
    images: Vec<RgbaImage>,
    /// Where the finished file's path (or an error) arrives.
    pub done: Option<Receiver<Result<PathBuf, String>>>,
}

impl LoopExport {
    pub fn new(frames: usize) -> Self {
        Self {
            frames,
            i: 0,
            settle: 0,
            since: Instant::now(),
            rx: None,
            images: Vec::new(),
            done: None,
        }
    }

    /// Advance: `ready` is how loaded frame `i` is (0..=1), `crop` the
    /// map's rectangle (points) and `ppp` pixels per point.
    pub fn step(&mut self, ctx: &egui::Context, ready: f32, crop: Rect, ppp: f32) -> Step {
        if self.done.is_some() {
            return Step::Writing;
        }
        if let Some(rx) = &self.rx {
            if let Ok(img) = rx.try_recv() {
                self.rx = None;
                if let Some(c) = crop_image(&img, crop, ppp) {
                    self.images.push(c);
                }
                self.i += 1;
                self.settle = 0;
                self.since = Instant::now();
                if self.i >= self.frames {
                    self.finish();
                    return Step::Writing;
                }
            }
            return Step::Show(self.i);
        }
        let waited = self.since.elapsed().as_secs_f32();
        if ready >= 0.99 || waited > MAX_WAIT_S {
            self.settle += 1;
            if self.settle > SETTLE_FRAMES {
                let (tx, rx): (Sender<Arc<ColorImage>>, _) = crossbeam_channel::bounded(1);
                let c2 = ctx.clone();
                ctx.request_screenshot(move |img| {
                    let _ = tx.send(img);
                    c2.request_repaint();
                });
                self.rx = Some(rx);
            }
        }
        Step::Show(self.i)
    }

    fn finish(&mut self) {
        let images = std::mem::take(&mut self.images);
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.done = Some(rx);
        std::thread::spawn(move || {
            let path = output_path();
            let r = write_gif(&images, &path).map(|()| path);
            let _ = tx.send(r);
        });
    }
}

/// Cut the map out of a window screenshot, scaled down to [`MAX_W`].
fn crop_image(img: &ColorImage, crop: Rect, ppp: f32) -> Option<RgbaImage> {
    let [w, h] = img.size;
    let x0 = ((crop.left() * ppp).round().max(0.0) as usize).min(w);
    let y0 = ((crop.top() * ppp).round().max(0.0) as usize).min(h);
    let x1 = ((crop.right() * ppp).round() as usize).clamp(x0, w);
    let y1 = ((crop.bottom() * ppp).round() as usize).clamp(y0, h);
    let (cw, ch) = (x1 - x0, y1 - y0);
    if cw < 16 || ch < 16 {
        return None;
    }
    let mut out = RgbaImage::new(cw as u32, ch as u32);
    for y in 0..ch {
        for x in 0..cw {
            let c = img.pixels[(y0 + y) * w + x0 + x];
            out.put_pixel(x as u32, y as u32, image::Rgba([c.r(), c.g(), c.b(), 255]));
        }
    }
    if out.width() > MAX_W {
        let nh = (out.height() as f32 * MAX_W as f32 / out.width() as f32).round() as u32;
        out = image::imageops::resize(
            &out,
            MAX_W,
            nh.max(1),
            image::imageops::FilterType::Triangle,
        );
    }
    Some(out)
}

/// `~/Pictures/silverskies-radar-YYYYMMDD-HHMM.gif` (or the home folder,
/// or the temp folder).
pub fn output_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let dir = home
        .as_ref()
        .map(|h| h.join("Pictures"))
        .filter(|p| p.is_dir())
        .or(home)
        .unwrap_or_else(std::env::temp_dir);
    let name = format!(
        "silverskies-radar-{}.gif",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    dir.join(name)
}

pub fn write_gif(images: &[RgbaImage], path: &Path) -> Result<(), String> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame};
    if images.is_empty() {
        return Err("no frames were captured".into());
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = GifEncoder::new_with_speed(std::io::BufWriter::new(file), 10);
    enc.set_repeat(Repeat::Infinite)
        .map_err(|e| e.to_string())?;
    let n = images.len();
    for (k, img) in images.iter().enumerate() {
        let ms = if k + 1 == n { LAST_MS } else { FRAME_MS };
        let frame = Frame::from_parts(img.clone(), 0, 0, Delay::from_numer_denom_ms(ms, 1));
        enc.encode_frame(frame).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_an_animated_gif() {
        let a = RgbaImage::from_pixel(20, 10, image::Rgba([255, 0, 0, 255]));
        let b = RgbaImage::from_pixel(20, 10, image::Rgba([0, 0, 255, 255]));
        let dir = std::env::temp_dir().join(format!("ss-gif-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.gif");
        write_gif(&[a, b], &p).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(&bytes[..6], b"GIF89a");
        assert!(write_gif(&[], &p).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn crops_and_scales() {
        let img = ColorImage::filled([2000, 1000], egui::Color32::RED);
        let c = crop_image(
            &img,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 500.0)),
            2.0,
        )
        .unwrap();
        assert_eq!(c.width(), MAX_W);
        assert_eq!(c.height(), MAX_W / 2);
    }
}
