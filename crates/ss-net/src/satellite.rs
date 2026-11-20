//! Geostationary satellite imagery worldwide, each satellite drawn over its
//! own longitude sector: GOES-West and GOES-East (NOAA) and Himawari (JMA)
//! from NASA GIBS, Meteosat (0° and Indian Ocean) from EUMETSAT's
//! EUMETView. Both serve 10-minute images; GIBS runs ~40–60 min behind
//! real time, EUMETView ~20–30 min.

use ss_core::types::SatProduct;

use crate::{Net, NetError, Result};

const GIBS: &str = "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best";
const EUMET: &str = "https://view.eumetsat.int/geoserver";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Server {
    /// NASA GIBS WMTS: layer, tile matrix set, deepest zoom.
    Gibs(&'static str, &'static str, u8),
    /// EUMETView WMS: workspace, layer, style ("" = the layer's default).
    Eumet(&'static str, &'static str, &'static str),
}

/// One satellite's imagery for a product, and the longitudes it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SatSource {
    /// Short name (`goes-east`).
    pub name: &'static str,
    /// The sector it's drawn over: west to east, degrees (east may exceed
    /// 180 for a sector across the date line).
    pub west: i16,
    pub east: i16,
    pub server: Server,
}

impl SatSource {
    /// Identifies the imagery (for tile cache keys).
    pub fn server_key(&self) -> String {
        match self.server {
            Server::Gibs(layer, ..) => layer.to_string(),
            Server::Eumet(ws, layer, style) => format!("{ws}:{layer}:{style}"),
        }
    }

    pub fn max_zoom(&self) -> u8 {
        match self.server {
            Server::Gibs(_, _, z) => z,
            Server::Eumet(..) => 9,
        }
    }

    /// A 256 px tile at `time` (unix s).
    pub fn tile_url(&self, time: i64, z: u8, x: u32, y: u32) -> String {
        let t = chrono::DateTime::from_timestamp(time, 0)
            .unwrap_or_default()
            .format("%Y-%m-%dT%H:%M:%SZ");
        match self.server {
            Server::Gibs(layer, tms, _) => {
                format!("{GIBS}/{layer}/default/{t}/{tms}/{z}/{y}/{x}.png")
            }
            Server::Eumet(ws, layer, style) => {
                let (w, s, e, n) = tile_bbox_3857(z, x, y);
                format!(
                    "{EUMET}/{ws}/wms?service=WMS&version=1.3.0&request=GetMap&layers={ws}:{layer}\
                     &styles={style}&crs=EPSG:3857&bbox={w},{s},{e},{n}&width=256&height=256\
                     &format=image/png&transparent=true&time={t}"
                )
            }
        }
    }

    /// Where the newest time is published.
    fn domain_url(&self) -> String {
        match self.server {
            Server::Gibs(layer, tms, _) => {
                let now = chrono::Utc::now();
                let from = (now - chrono::Duration::days(1)).format("%Y-%m-%dT%H:%M:%SZ");
                let to = now.format("%Y-%m-%dT%H:%M:%SZ");
                format!("{GIBS}/1.0.0/{layer}/default/{tms}/all/{from}--{to}.xml")
            }
            Server::Eumet(ws, layer, _) => {
                format!(
                    "{EUMET}/{ws}/{layer}/wms?service=WMS&request=GetCapabilities&version=1.3.0"
                )
            }
        }
    }
}

/// Web Mercator bounds of a tile, metres.
fn tile_bbox_3857(z: u8, x: u32, y: u32) -> (f64, f64, f64, f64) {
    const HALF: f64 = 20_037_508.342_789_244;
    let size = 2.0 * HALF / f64::from(1u32 << z);
    let w = -HALF + f64::from(x) * size;
    let n = HALF - f64::from(y) * size;
    (w, n - size, w + size, n)
}

/// The satellites for a product, west to east (none for the live
/// composite infrared, which LibreWXR serves).
pub fn sources(product: SatProduct) -> Vec<SatSource> {
    use Server::{Eumet, Gibs};
    const L6: &str = "GoogleMapsCompatible_Level6";
    const L7: &str = "GoogleMapsCompatible_Level7";
    let (gw, ge, hi, mtg, iodc) = match product {
        SatProduct::LiveInfrared => return Vec::new(),
        SatProduct::Infrared => (
            Gibs("GOES-West_ABI_Band13_Clean_Infrared", L6, 6),
            Gibs("GOES-East_ABI_Band13_Clean_Infrared", L6, 6),
            Gibs("Himawari_AHI_Band13_Clean_Infrared", L6, 6),
            // Colour-enhanced like GIBS's infrared.
            Eumet("mtg_fd", "ir105_hrfi", "mtg_fd_ir105_hrfi_style_02"),
            Eumet("msg_iodc", "ir108", ""),
        ),
        SatProduct::TrueColor => (
            Gibs("GOES-West_ABI_GeoColor", L7, 7),
            Gibs("GOES-East_ABI_GeoColor", L7, 7),
            // No GeoColor for Himawari on GIBS: infrared there.
            Gibs("Himawari_AHI_Band13_Clean_Infrared", L6, 6),
            Eumet("mtg_fd", "rgb_geocolour", ""),
            Eumet("msg_iodc", "rgb_naturalenhncd", ""),
        ),
        SatProduct::AirMass => (
            Gibs("GOES-West_ABI_Air_Mass", L6, 6),
            Gibs("GOES-East_ABI_Air_Mass", L6, 6),
            Gibs("Himawari_AHI_Air_Mass", L6, 6),
            Eumet("msg_fes", "rgb_airmass", ""),
            Eumet("msg_iodc", "rgb_airmass", ""),
        ),
    };
    vec![
        SatSource {
            name: "goes-west",
            west: 165,
            east: 255,
            server: gw,
        },
        SatSource {
            name: "goes-east",
            west: -105,
            east: -30,
            server: ge,
        },
        SatSource {
            name: "meteosat",
            west: -30,
            east: 40,
            server: mtg,
        },
        SatSource {
            name: "meteosat-iodc",
            west: 40,
            east: 90,
            server: iodc,
        },
        SatSource {
            name: "himawari",
            west: 90,
            east: 165,
            server: hi,
        },
    ]
}

/// The newest time in a GIBS domain (`…/2026-11-20T12:30:00Z/PT10M`) or a
/// WMS time dimension (`…/2026-11-20T12:40:00.000Z/PT10M`).
pub fn parse_latest(xml: &str) -> Option<i64> {
    let body = if let Some(i) = xml.find("<Domain>") {
        let rest = &xml[i + 8..];
        &rest[..rest.find('<')?]
    } else {
        let i = xml.find("<Dimension name=\"time\"")?;
        let rest = &xml[i..];
        let rest = &rest[rest.find('>')? + 1..];
        &rest[..rest.find('<')?]
    };
    // The last interval's end.
    let last = body.split(',').next_back()?;
    let end = last.split('/').nth(1).unwrap_or(last).trim();
    chrono::DateTime::parse_from_rfc3339(end)
        .ok()
        .map(|d| d.timestamp())
}

impl Net {
    /// When `src` has its newest image (unix s).
    pub async fn sat_latest(&self, src: &SatSource) -> Result<i64> {
        let xml = self.get_text(&src.domain_url(), None).await?;
        parse_latest(&xml).ok_or_else(|| NetError::Parse(format!("{}: no times", src.name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_latest_times() {
        let gibs = "<Domains><DimensionDomain><ows:Identifier>time</ows:Identifier>\
            <Domain>2026-11-20/2026-11-20T12:30:00Z/PT10M</Domain></DimensionDomain></Domains>";
        let t = parse_latest(gibs).unwrap();
        assert_eq!(
            chrono::DateTime::from_timestamp(t, 0)
                .unwrap()
                .format("%H:%M")
                .to_string(),
            "12:30"
        );
        let wms = r#"<Dimension name="time" default="2026-11-20T12:40:00Z" units="ISO8601" nearestValue="1">2024-09-23T00:00:00.000Z/2026-11-20T12:40:00.000Z/PT10M</Dimension>"#;
        let t = parse_latest(wms).unwrap();
        assert_eq!(
            chrono::DateTime::from_timestamp(t, 0)
                .unwrap()
                .format("%H:%M")
                .to_string(),
            "12:40"
        );
    }

    #[test]
    fn sectors_cover_the_world_once() {
        for p in [
            SatProduct::Infrared,
            SatProduct::TrueColor,
            SatProduct::AirMass,
        ] {
            let s = sources(p);
            let total: i32 = s.iter().map(|x| i32::from(x.east - x.west)).sum();
            assert_eq!(total, 360);
        }
        assert!(sources(SatProduct::LiveInfrared).is_empty());
    }

    #[test]
    fn builds_tile_urls() {
        let s = sources(SatProduct::Infrared);
        let t = 1_795_167_000; // 2026-11-20 09:30Z
        assert_eq!(
            s[1].tile_url(t, 3, 2, 3),
            "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/GOES-East_ABI_Band13_Clean_Infrared/default/2026-11-20T09:30:00Z/GoogleMapsCompatible_Level6/3/3/2.png"
        );
        let u = s[2].tile_url(t, 0, 0, 0);
        assert!(u.contains("layers=mtg_fd:ir105_hrfi") && u.contains("time=2026-11-20T09:30:00Z"));
        assert!(u.contains(
            "bbox=-20037508.342789244,-20037508.342789244,20037508.342789244,20037508.342789244"
        ));
    }
}
