//! Storm cells anywhere the radar mosaic has coverage: fetch the raw-dBZ
//! ("colour scheme 0") tiles around a point for the two newest observed
//! frames and run the cell tracker in `ss_core::cells`.

use crate::{Net, NetError, Result};
use ss_core::cells::{dbz_from_pixel, mosaic_cells, DbzGrid};
use ss_core::geo::lonlat_to_world;
use ss_core::storms::StormCell;
use ss_core::RadarFrame;

/// Zoom of the tiles analysed (~2 km pixels at mid latitudes).
pub const ZOOM: u8 = 6;
/// Tiles per side around the point (an odd number, centred on it).
pub const SPAN: u32 = 3;

/// Raw-dBZ tile URL (scheme 0, unsmoothed, no snow colours).
pub fn dbz_tile_url(host: &str, path: &str, z: u8, x: u32, y: u32) -> String {
    format!(
        "{}{path}/256/{z}/{x}/{y}/0/0_0.png",
        host.trim_end_matches('/')
    )
}

/// The tile block (top-left tile x/y) centred on `(lon, lat)`.
pub fn block_origin(lon: f64, lat: f64) -> (u32, u32) {
    let (wx, wy) = lonlat_to_world(lon, lat, f64::from(ZOOM));
    let n = 1u32 << ZOOM;
    let half = SPAN / 2;
    let tx = ((wx / 256.0) as u32).min(n - 1);
    let ty = ((wy / 256.0) as u32).min(n - 1);
    (
        tx.saturating_sub(half).min(n - SPAN),
        ty.saturating_sub(half).min(n - SPAN),
    )
}

/// Decode one tile into the grid at block position (`bx`, `by`).
fn paint_tile(grid: &mut DbzGrid, png: &[u8], bx: u32, by: u32) -> Result<()> {
    let img = image::load_from_memory(png)
        .map_err(|e| NetError::Parse(format!("radar tile: {e}")))?
        .to_luma_alpha8();
    for (x, y, p) in img.enumerate_pixels() {
        let gx = (bx * 256 + x) as usize;
        let gy = (by * 256 + y) as usize;
        if gx < grid.w && gy < grid.h {
            grid.dbz[gy * grid.w + gx] = dbz_from_pixel(p.0[0], p.0[1]);
        }
    }
    Ok(())
}

impl Net {
    async fn dbz_grid(&self, host: &str, frame: &RadarFrame, ox: u32, oy: u32) -> Result<DbzGrid> {
        let size = (SPAN * 256) as usize;
        let mut grid = DbzGrid::new(size, size, ZOOM, f64::from(ox * 256), f64::from(oy * 256));
        let fetches = (0..SPAN).flat_map(|by| (0..SPAN).map(move |bx| (bx, by)));
        let tiles = futures::future::join_all(fetches.map(|(bx, by)| async move {
            let url = dbz_tile_url(host, &frame.path, ZOOM, ox + bx, oy + by);
            (bx, by, self.get_bytes(&url, Some("image/png")).await)
        }))
        .await;
        for (bx, by, r) in tiles {
            // A missing tile just means no data there.
            if let Ok(png) = r {
                paint_tile(&mut grid, &png, bx, by)?;
            }
        }
        Ok(grid)
    }

    /// The raw-dBZ grid of one mosaic frame around (`lat`, `lon`).
    pub async fn mosaic_grid(
        &self,
        host: &str,
        frame: &RadarFrame,
        lat: f64,
        lon: f64,
    ) -> Result<DbzGrid> {
        let (ox, oy) = block_origin(lon, lat);
        self.dbz_grid(host, frame, ox, oy).await
    }

    /// The strongest echo at each point around (`lat`, `lon`) over the
    /// `frames` (a storm swath), on a regular lat/lon grid of `step_deg`
    /// (for [`crate::mrms::Grid`]-based drawing); NaN where nothing fell.
    pub async fn mosaic_swath(
        &self,
        host: &str,
        frames: &[RadarFrame],
        lat: f64,
        lon: f64,
        step_deg: f64,
    ) -> Result<crate::mrms::Grid> {
        let (ox, oy) = block_origin(lon, lat);
        let grids: Vec<_> =
            futures::future::join_all(frames.iter().map(|f| self.dbz_grid(host, f, ox, oy)))
                .await
                .into_iter()
                .flatten()
                .collect();
        let Some(first) = grids.first() else {
            return Err(NetError::Missing("radar frames"));
        };
        let mut max = vec![f32::NAN; first.w * first.h];
        for g in &grids {
            for (m, d) in max.iter_mut().zip(&g.dbz) {
                if let Some(d) = d {
                    if m.is_nan() || *m < *d {
                        *m = *d;
                    }
                }
            }
        }
        let (w, h, zoom) = (first.w, first.h, first.zoom);
        let (x0, y0) = (first.x0, first.y0);
        let (west, north) = ss_core::geo::world_to_lonlat(x0, y0, f64::from(zoom));
        let (east, south) =
            ss_core::geo::world_to_lonlat(x0 + w as f64, y0 + h as f64, f64::from(zoom));
        let nx = ((east - west) / step_deg).floor() as usize;
        let ny = ((north - south) / step_deg).floor() as usize;
        let mut values = vec![f32::NAN; nx * ny];
        for r in 0..ny {
            let la = north - (r as f64 + 0.5) * step_deg;
            let (_, wy) = ss_core::geo::lonlat_to_world(west, la, f64::from(zoom));
            let py = (wy - y0) as usize;
            if py >= h {
                continue;
            }
            for c in 0..nx {
                let lo = west + (c as f64 + 0.5) * step_deg;
                let (wx, _) = ss_core::geo::lonlat_to_world(lo, la, f64::from(zoom));
                let px = (wx - x0) as usize;
                if px < w {
                    values[r * nx + c] = max[py * w + px];
                }
            }
        }
        Ok(crate::mrms::Grid {
            lat0: north - step_deg / 2.0,
            lon0: west + step_deg / 2.0,
            dlat: step_deg,
            dlon: step_deg,
            nx,
            ny,
            values,
            time: frames.last().map_or(0, |f| f.time),
        })
    }

    /// Storm cells within roughly 700–1,000 km of (`lat`, `lon`) from the two
    /// newest observed mosaic frames.
    pub async fn mosaic_storm_cells(
        &self,
        host: &str,
        past: &[RadarFrame],
        lat: f64,
        lon: f64,
    ) -> Result<Vec<StormCell>> {
        let n = past.len();
        if n < 2 {
            return Ok(Vec::new());
        }
        let (a, b) = (&past[n - 2], &past[n - 1]);
        let (ox, oy) = block_origin(lon, lat);
        let (ga, gb) = futures::future::join(
            self.dbz_grid(host, a, ox, oy),
            self.dbz_grid(host, b, ox, oy),
        )
        .await;
        let minutes = ((b.time - a.time) as f64 / 60.0).max(1.0);
        let valid = chrono::DateTime::from_timestamp(b.time, 0)
            .map(|d| d.to_rfc3339())
            .unwrap_or_default();
        Ok(mosaic_cells(&ga?, &gb?, minutes, &valid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_urls_ask_for_raw_dbz() {
        assert_eq!(
            dbz_tile_url("https://h/", "/v2/radar/1", 6, 10, 24),
            "https://h/v2/radar/1/256/6/10/24/0/0_0.png"
        );
    }

    #[test]
    fn block_is_centred_and_clamped() {
        // Northern California at z6 is tile (10, 24): block starts one left/up.
        assert_eq!(block_origin(-121.8, 38.0), (9, 23));
        // At the antimeridian / poles the block stays on the map.
        let (x, y) = block_origin(179.9, 84.0);
        assert!(x + SPAN <= 64 && y + SPAN <= 64);
        assert_eq!(block_origin(-180.0, -84.0).0, 0);
    }

    #[test]
    fn decodes_a_greyscale_tile() {
        let mut img = image::GrayAlphaImage::new(256, 256);
        img.put_pixel(10, 20, image::LumaA([90, 255])); // 58 dBZ
        img.put_pixel(11, 20, image::LumaA([0, 0]));
        let mut png = Vec::new();
        image::DynamicImage::ImageLumaA8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut g = DbzGrid::new(512, 512, ZOOM, 0.0, 0.0);
        paint_tile(&mut g, &png, 1, 0).unwrap();
        assert_eq!(g.dbz[20 * 512 + 256 + 10], Some(58.0));
        assert_eq!(g.dbz[20 * 512 + 256 + 11], None);
    }
}
