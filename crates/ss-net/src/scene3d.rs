//! Builds the 3D scene for a place: terrain everywhere (AWS Terrain Tiles,
//! global, free), plus radar — a measured NEXRAD Level II volume where a
//! WSR-88D is in range, otherwise the global radar mosaic extruded into
//! estimated columns.

use std::collections::HashMap;

use crate::{Net, NetError, Result};
use ss_core::radar3d::{
    local_km, mosaic_voxels, nearest_site, polar_volume, terrain_from_tiles, volume_voxels,
    RadarSource, Scene, Terrain, VoxelSpec,
};
use ss_core::{RadarFrame, RadarStation};

/// Terrarium-encoded elevation tiles (Mapzen / AWS Open Data, keyless).
pub const TERRAIN_URL: &str = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium";
/// Half the scene's width, km.
pub const HALF_KM: f32 = 200.0;
/// Terrain samples per side.
pub const TERRAIN_N: usize = 161;
/// A NEXRAD site is used when the place is within this distance.
pub const NEXRAD_RANGE_KM: f64 = 230.0;
/// Voxel size for measured volumes (gates are 0.25 km, radials 0.5°).
pub const NEXRAD_KM: f32 = 1.25;
/// Voxel size for the extruded mosaic (its pixels are ~1.5–2.5 km).
pub const MOSAIC_KM: f32 = 2.0;
/// Cap on voxels, to keep the view interactive.
pub const MAX_VOXELS: usize = 140_000;

/// The zoom whose tiles give about one terrain sample per pixel.
fn terrain_zoom(lat: f64) -> u8 {
    zoom_for(lat, 2.0 * f64::from(HALF_KM) / TERRAIN_N as f64)
}

/// The smallest zoom with pixels no bigger than `km_per_px_needed`.
fn zoom_for(lat: f64, km_per_px_needed: f64) -> u8 {
    let mut z = 4u8;
    while z < 14 {
        let km_per_px = 40_075.0 * lat.to_radians().cos() / (256.0 * 2f64.powi(i32::from(z)));
        if km_per_px <= km_per_px_needed {
            break;
        }
        z += 1;
    }
    z
}

impl Net {
    /// Terrain heights around (`lat`, `lon`).
    pub async fn terrain(&self, lat: f64, lon: f64) -> Result<Terrain> {
        let z = terrain_zoom(lat);
        let n = 1u32 << z;
        // Tiles covering the scene.
        let deg_lat = f64::from(HALF_KM) / 111.2;
        let deg_lon = deg_lat / lat.to_radians().cos().max(0.05);
        let (x0, y0) = ss_core::geo::lonlat_to_world(lon - deg_lon, lat + deg_lat, f64::from(z));
        let (x1, y1) = ss_core::geo::lonlat_to_world(lon + deg_lon, lat - deg_lat, f64::from(z));
        let (tx0, ty0) = ((x0 / 256.0) as i64, (y0 / 256.0).max(0.0) as u32);
        let (tx1, ty1) = ((x1 / 256.0) as i64, ((y1 / 256.0) as u32).min(n - 1));
        let mut wanted = Vec::new();
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                wanted.push((tx.rem_euclid(i64::from(n)) as u32, ty));
            }
        }
        let fetched = futures::future::join_all(wanted.iter().map(|&(x, y)| async move {
            let url = format!("{TERRAIN_URL}/{z}/{x}/{y}.png");
            ((x, y), self.get_bytes(&url, Some("image/png")).await)
        }))
        .await;
        let mut tiles: HashMap<(u32, u32), image::RgbImage> = HashMap::new();
        for (k, r) in fetched {
            if let Ok(png) = r {
                if let Ok(img) = image::load_from_memory(&png) {
                    tiles.insert(k, img.to_rgb8());
                }
            }
        }
        if tiles.is_empty() {
            return Err(NetError::Missing("terrain tiles"));
        }
        Ok(terrain_from_tiles(
            lat,
            lon,
            HALF_KM,
            TERRAIN_N,
            z,
            |wx, wy| {
                let tx = ((wx / 256.0).floor() as i64).rem_euclid(i64::from(n)) as u32;
                let ty = (wy / 256.0).floor() as u32;
                let img = tiles.get(&(tx, ty))?;
                let px = (wx.rem_euclid(256.0) as u32).min(img.width() - 1);
                let py = (wy.rem_euclid(256.0) as u32).min(img.height() - 1);
                Some(img.get_pixel(px, py).0)
            },
        ))
    }

    /// The whole scene. `stations` are the WSR-88D sites (may be empty);
    /// `mosaic` the radar host and observed frames for the fallback.
    pub async fn scene3d(
        &self,
        lat: f64,
        lon: f64,
        stations: &[RadarStation],
        mosaic_host: &str,
        frames: &[RadarFrame],
    ) -> Result<Scene> {
        let terrain = self.terrain(lat, lon).await.ok();
        let site = nearest_site(lat, lon, stations, |s| (s.lat, s.lon), NEXRAD_RANGE_KM)
            .map(|(s, _)| s.id.clone());
        let mut scene = Scene {
            lat,
            lon,
            half_km: HALF_KM,
            terrain,
            voxels: Vec::new(),
            voxel_km: NEXRAD_KM,
            voxel_dz_km: 0.5,
            source: None,
            radar_xy: None,
            polar: None,
        };
        // Measured volume where a NEXRAD covers the place…
        if let Some(id) = site {
            match self.latest_level2(&id).await {
                Ok(vol) => {
                    let (vlat, vlon) = (vol.lat, vol.lon);
                    let time = vol.time.clone();
                    let (voxels, polar) = tokio::task::spawn_blocking(move || {
                        let spec = VoxelSpec {
                            half_km: HALF_KM,
                            min_dbz: 10.0,
                            km: NEXRAD_KM,
                            dz: 0.5,
                            max: MAX_VOXELS,
                        };
                        (
                            volume_voxels(&vol, lat, lon, spec),
                            polar_volume(&vol, lat, lon),
                        )
                    })
                    .await
                    .map_err(|e| NetError::Parse(e.to_string()))?;
                    scene.voxels = voxels;
                    scene.polar = Some(polar);
                    scene.radar_xy = Some(local_km(lat, lon, vlat, vlon));
                    scene.source = Some(RadarSource::Nexrad { site: id, time });
                    return Ok(scene);
                }
                Err(e) => tracing::warn!("Level II for {id}: {e}; using the mosaic"),
            }
        }
        // …otherwise the global mosaic, extruded.
        if let Some(frame) = frames.last() {
            let grid = self.mosaic_grid(mosaic_host, frame, lat, lon).await?;
            let terrain = scene.terrain.clone();
            let voxels = tokio::task::spawn_blocking(move || {
                let spec = VoxelSpec {
                    half_km: HALF_KM,
                    min_dbz: 10.0,
                    km: MOSAIC_KM,
                    dz: 0.5,
                    max: MAX_VOXELS,
                };
                mosaic_voxels(&grid, lat, lon, spec, terrain.as_ref())
            })
            .await
            .map_err(|e| NetError::Parse(e.to_string()))?;
            scene.voxels = voxels;
            scene.voxel_km = MOSAIC_KM;
            scene.voxel_dz_km = 0.5;
            scene.source = Some(RadarSource::Mosaic {
                time: chrono::DateTime::from_timestamp(frame.time, 0)
                    .map(|d| d.to_rfc3339())
                    .unwrap_or_default(),
            });
        }
        Ok(scene)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_zoom_fits_the_scene() {
        // ~2.5 km per sample needed: zoom 6 near the equator is 2.4 km/px.
        assert_eq!(terrain_zoom(0.0), 6);
        assert!(terrain_zoom(60.0) <= 6);
        assert!(terrain_zoom(89.0) >= 4);
    }
}
