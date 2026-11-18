//! STORMS: the storm cells near the tracked location (from every NEXRAD's
//! storm-cell algorithms: tornado vortex and mesocyclone signatures, hail,
//! echo tops, motion), with distance, bearing and time-to-arrival when a
//! cell is heading this way, then the nearest local storm reports.

use egui::{RichText, Ui};
use ss_core::geo::{bearing_deg, haversine_km};
use ss_core::storms::{StormCell, StormReport, Tvs};
use ss_core::weather::units::compass_8;
use ss_core::PanelKind;

use super::{placeholder, Panel, PanelCx};
use crate::theme::{c32, c32a, on, PRO_DOWN, PRO_YELLOW};

/// Only cells within this range of the location are listed.
const NEAR_KM: f64 = 400.0;

pub struct StormsPanel;

/// Threat colour: tornadic red, mesocyclone magenta, hail/severe yellow,
/// strong cells amber, others grey.
pub fn threat_color(t: u8) -> [u8; 3] {
    match t {
        4 => [0xff, 0x2a, 0x2a],
        3 => [0xff, 0x3d, 0xd8],
        2 => PRO_YELLOW,
        1 => [0xff, 0x9f, 0x1c],
        _ => [0x9a, 0x9a, 0xa6],
    }
}

/// Minutes until a cell moving at its current motion passes closest to
/// (`lat`, `lon`), with that closest distance — `None` when it's moving away.
pub fn closest_approach(c: &StormCell, lat: f64, lon: f64) -> Option<(f64, f64)> {
    if c.speed_kt < 3.0 {
        return None;
    }
    // Local flat-earth frame around the location, km.
    let kx = 111.32 * lat.to_radians().cos();
    let (x, y) = ((c.lon - lon) * kx, (c.lat - lat) * 110.57);
    let h = c.heading_deg().to_radians();
    let v = c.speed_kt * 1.852 / 60.0; // km per minute
    let (vx, vy) = (h.sin() * v, h.cos() * v);
    let t = -(x * vx + y * vy) / (vx * vx + vy * vy);
    if t <= 0.0 {
        return None;
    }
    let (cx, cy) = (x + vx * t, y + vy * t);
    Some((t, (cx * cx + cy * cy).sqrt()))
}

fn cell_row(ui: &mut Ui, p: &ss_core::theme::Palette, c: &StormCell, lat: f64, lon: f64) {
    let km = haversine_km(lat, lon, c.lat, c.lon);
    let brg = bearing_deg(lat, lon, c.lat, c.lon);
    let t = c.threat();
    let col = threat_color(t);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let tag = match (c.tvs, t) {
            (Tvs::Tvs, _) => "TOR",
            (Tvs::Elevated, _) => "ETVS",
            (_, 3) | (_, 2) if c.meso > 0 => "MESO",
            (_, 2) => "HAIL",
            (_, 1) => "STRG",
            _ => "CELL",
        };
        egui::Frame::new()
            .fill(c32(col))
            .inner_margin(egui::Margin::symmetric(4, 0))
            .show(ui, |ui| {
                ui.label(RichText::new(tag).monospace().color(on(col)));
            });
        ui.label(
            RichText::new(format!("{}/{}", c.radar, c.id))
                .monospace()
                .color(c32(p.text)),
        );
        ui.label(
            RichText::new(format!("{:>4.0} km {:<3}", km, compass_8(brg)))
                .monospace()
                .color(c32a(p.text2)),
        );
        ui.label(
            RichText::new(format!(
                "→{} {:.0} kt",
                compass_8(c.heading_deg()),
                c.speed_kt
            ))
            .monospace()
            .color(c32a(p.text2)),
        );
    });
    ui.label(
        RichText::new(format!("  {}", c.summary()))
            .monospace()
            .size(11.5)
            .color(c32a(p.text3)),
    );
    if let Some((mins, miss_km)) = closest_approach(c, lat, lon) {
        if miss_km < 15.0 && mins < 120.0 {
            ui.label(
                RichText::new(format!(
                    "  ▲ heading here: closest {:.0} km in {:.0} min",
                    miss_km, mins
                ))
                .monospace()
                .size(11.5)
                .color(c32(PRO_DOWN)),
            );
        }
    }
}

fn report_row(ui: &mut Ui, p: &ss_core::theme::Palette, r: &StormReport, lat: f64, lon: f64) {
    let km = haversine_km(lat, lon, r.lat, r.lon);
    let when = chrono::DateTime::parse_from_rfc3339(&r.valid)
        .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let col = r.kind.color();
        egui::Frame::new()
            .fill(c32(col))
            .inner_margin(egui::Margin::symmetric(4, 0))
            .show(ui, |ui| {
                ui.label(RichText::new(r.kind.letter()).monospace().color(on(col)));
            });
        ui.label(RichText::new(when).monospace().color(c32a(p.text3)));
        ui.label(RichText::new(r.headline()).monospace().color(c32(p.text)));
        ui.label(
            RichText::new(format!("{:.0} km · {}, {}", km, r.place, r.state))
                .monospace()
                .color(c32a(p.text2)),
        );
    });
}

impl Panel for StormsPanel {
    fn kind(&self) -> PanelKind {
        PanelKind::Storms
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut PanelCx) {
        let p = cx.palette;
        let sh = &*cx.shared;
        let Some(loc) = cx.location.as_ref() else {
            placeholder(ui, &p, "no location");
            return;
        };
        let (lat, lon) = (loc.lat, loc.lon);
        // NEXRAD cells where the US network covers; radar-mosaic cells
        // (computed for the active place) everywhere else.
        let mosaic = sh.mosaic_for.as_deref() == Some(loc.id.as_str());
        let mut cells: Vec<&StormCell> = sh
            .storm_cells
            .iter()
            .chain(sh.mosaic_cells.iter().filter(|_| mosaic))
            .filter(|c| haversine_km(lat, lon, c.lat, c.lon) <= NEAR_KM)
            .collect();
        let mut tropical: Vec<(f64, &ss_core::storms::TropicalCyclone)> = sh
            .tropical
            .iter()
            .map(|t| (haversine_km(lat, lon, t.lat, t.lon), t))
            .collect();
        tropical.sort_by(|a, b| a.0.total_cmp(&b.0));
        cells.sort_by(|a, b| {
            b.threat().cmp(&a.threat()).then(
                haversine_km(lat, lon, a.lat, a.lon)
                    .total_cmp(&haversine_km(lat, lon, b.lat, b.lon)),
            )
        });
        let mut reports: Vec<&StormReport> = sh
            .storm_reports
            .iter()
            .filter(|r| haversine_km(lat, lon, r.lat, r.lon) <= NEAR_KM)
            .collect();
        reports.sort_by(|a, b| b.valid.cmp(&a.valid));

        egui::ScrollArea::vertical()
            .id_salt("storms_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                // Summary line: counts by threat across the whole network.
                let all = &sh.storm_cells;
                let tor = all.iter().filter(|c| c.tvs == Tvs::Tvs).count();
                let meso = all.iter().filter(|c| c.meso > 0).count();
                let hail = all.iter().filter(|c| c.max_hail_in >= 1.0).count();
                ui.label(
                    RichText::new(format!(
                        "US NEXRAD: {} cells · {} TVS · {} meso · {} ≥1\" hail",
                        all.len(),
                        tor,
                        meso,
                        hail
                    ))
                    .monospace()
                    .color(c32(p.a0)),
                );
                if let Some(at) = sh.storms_at {
                    ui.label(
                        RichText::new(format!("updated {}", at.format("%H:%M")))
                            .monospace()
                            .size(11.0)
                            .color(c32a(p.text3)),
                    );
                }
                if let Some(e) = &sh.storms_error {
                    ui.label(
                        RichText::new(format!("storm data: {e}"))
                            .monospace()
                            .color(c32(p.danger)),
                    );
                }
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("-- TROPICAL CYCLONES, WORLDWIDE ({}) --", tropical.len()))
                        .monospace()
                        .color(c32(p.a0)),
                );
                if tropical.is_empty() {
                    placeholder(ui, &p, "no active tropical cyclones");
                }
                for (km, t) in &tropical {
                    let col = t.color();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        egui::Frame::new()
                            .fill(c32(col))
                            .inner_margin(egui::Margin::symmetric(4, 0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(t.alert.to_uppercase())
                                        .monospace()
                                        .color(on(col)),
                                );
                            });
                        ui.label(RichText::new(&t.name).monospace().color(c32(p.text)));
                        ui.label(
                            RichText::new(format!(
                                "{:.0} km {} · {:.0} km/h",
                                km,
                                compass_8(bearing_deg(lat, lon, t.lat, t.lon)),
                                t.max_wind_kmh
                            ))
                            .monospace()
                            .color(c32a(p.text2)),
                        );
                    });
                    ui.label(
                        RichText::new(format!(
                            "  {}{}",
                            t.category,
                            if t.country.is_empty() {
                                String::new()
                            } else {
                                format!(" · {}", t.country)
                            }
                        ))
                        .monospace()
                        .size(11.5)
                        .color(c32a(p.text3)),
                    );
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("-- CELLS WITHIN {NEAR_KM:.0} KM ({}) --", cells.len()))
                        .monospace()
                        .color(c32(p.a0)),
                );
                if mosaic {
                    ui.label(
                        RichText::new(
                            "no NEXRAD here: cells found in the radar mosaic (intensity and motion only)",
                        )
                        .monospace()
                        .size(11.0)
                        .color(c32a(p.text3)),
                    );
                }
                if cells.is_empty() {
                    placeholder(ui, &p, "no storm cells nearby");
                }
                for c in cells.iter().take(40) {
                    cell_row(ui, &p, c, lat, lon);
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("-- STORM REPORTS, 24 H, US ({}) --", reports.len()))
                        .monospace()
                        .color(c32(p.a0)),
                );
                if reports.is_empty() {
                    placeholder(ui, &p, "no storm reports nearby");
                }
                for r in reports.iter().take(60) {
                    report_row(ui, &p, r, lat, lon);
                }
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Cells: NEXRAD via Iowa Environmental Mesonet (US), or found in the global radar mosaic elsewhere. \
                         Reports: NWS (US only). Tropical cyclones: GDACS (JTWC/NOAA), worldwide.",
                    )
                        .monospace()
                        .size(10.5)
                        .color(c32a(p.text3)),
                );
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(lon: f64, lat: f64, from: f64, kt: f64) -> StormCell {
        StormCell {
            radar: "TLX".into(),
            id: "A1".into(),
            lon,
            lat,
            tvs: Tvs::None,
            meso: 0,
            posh: 0.0,
            poh: 0.0,
            max_hail_in: 0.0,
            vil: 0.0,
            max_dbz: 50.0,
            max_dbz_height_kft: 0.0,
            top_kft: 0.0,
            motion_from_deg: from,
            speed_kt: kt,
            valid: String::new(),
            source: ss_core::storms::CellSource::Nexrad,
            area_km2: 0.0,
        }
    }

    #[test]
    fn approaching_cell_has_eta_receding_cell_none() {
        // 30 km due west, moving from the west (heading east) at 30 kt.
        let c = cell(-97.8, 35.2, 270.0, 30.0);
        let (mins, miss) = closest_approach(&c, 35.2, -97.47).unwrap();
        assert!(miss < 2.0, "{miss}");
        // ~30 km at 55.6 km/h ≈ 32 min.
        assert!((mins - 32.4).abs() < 3.0, "{mins}");
        // Same cell moving west: going away.
        assert!(closest_approach(&cell(-97.8, 35.2, 90.0, 30.0), 35.2, -97.47).is_none());
        // Stationary cells have no ETA.
        assert!(closest_approach(&cell(-97.8, 35.2, 270.0, 0.0), 35.2, -97.47).is_none());
    }
}
