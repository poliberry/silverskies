use ss_core::same::SameHeader;
use ss_core::{
    AirQuality, Alert, BBox, Location, NwrFeed, RadarStation, SpcOutlookFeature, Weather,
    WeatherMaps,
};

use crate::state::{LogLevel, RadioStatus, WindObs};

/// Results posted by background tasks and applied on the UI thread.
pub enum AppEvent {
    /// NEXRAD storm cells (IEM).
    StormCells(Result<Vec<ss_core::storms::StormCell>, String>),
    /// Tropical cyclones worldwide (GDACS).
    Tropical(Result<Vec<ss_core::storms::TropicalCyclone>, String>),
    /// Storm cells found in the radar mosaic around a location.
    MosaicCells {
        location_id: String,
        result: Result<Vec<ss_core::storms::StormCell>, String>,
    },
    /// Local storm reports (IEM).
    StormReports(Result<Vec<ss_core::storms::StormReport>, String>),
    /// Forecast soundings for a location (Open-Meteo pressure levels).
    /// A radar site's newest Level II scan (for the 2D radar).
    Level2 {
        site: String,
        result: Result<std::sync::Arc<ss_core::radar3d::SiteScan>, String>,
    },
    /// The 3D view's scene for a location.
    Scene3d {
        location_id: String,
        result: Result<Box<ss_core::radar3d::Scene>, String>,
    },
    Soundings {
        location_id: String,
        result: Result<Vec<ss_core::sounding::Sounding>, String>,
    },
    WeatherLoaded {
        location_id: String,
        result: Result<Weather, String>,
    },
    AlertsLoaded {
        location_id: String,
        alerts: Vec<Alert>,
        errors: Vec<String>,
    },
    RadarStations(Result<Vec<RadarStation>, String>),
    WeatherMaps(Result<WeatherMaps, String>),
    Geocoded {
        query: String,
        results: Vec<Location>,
    },
    ReverseGeocoded {
        location_id: String,
        label: String,
    },
    IpLocated(Option<Location>),
    AreaAlertsLoaded(Vec<Alert>),
    /// Alerts for the current map viewport (auto-refreshed).
    ViewportAlertsLoaded {
        bbox: BBox,
        alerts: Vec<Alert>,
    },
    Log(LogLevel, String),
    /// Nearest NWR streams for a location (empty when none could be resolved).
    RadioCandidates {
        location_id: String,
        feeds: Vec<NwrFeed>,
    },
    RadioStatus(RadioStatus),
    SameHeader(SameHeader),
    SameEom,
    SpcOutlook(Result<Vec<SpcOutlookFeature>, String>),
    McdAlerts(Result<Vec<Alert>, String>),
    WindObs {
        #[allow(dead_code)] // identifies the requested viewport
        bbox: BBox,
        obs: Vec<WindObs>,
    },
    AirQuality {
        location_id: String,
        result: Result<AirQuality, String>,
    },
}
