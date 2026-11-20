//! Material Symbols icons (Apache-2.0), bundled as a small subset font.
//! Register [`FONT`] as a font family and draw these strings in it.

/// The subset font (only the icons below).
pub const FONT: &[u8] =
    include_bytes!("../assets/fonts/material-symbols/MaterialSymbolsOutlined-subset.ttf");

pub const ACCESSIBILITY_NEW: &str = "\u{e92c}";
pub const ADD: &str = "\u{e145}";
pub const AIR: &str = "\u{efd8}";
pub const ARROW_OUTWARD: &str = "\u{f8ce}";
pub const AUTO_AWESOME: &str = "\u{e65f}";
pub const BOOKMARK_ADD: &str = "\u{e598}";
pub const CALENDAR_MONTH: &str = "\u{ebcc}";
pub const CELL_TOWER: &str = "\u{ebba}";
pub const CHECK: &str = "\u{e5ca}";
pub const CHEVRON_LEFT: &str = "\u{e408}";
pub const CHEVRON_RIGHT: &str = "\u{e409}";
pub const CLOSE: &str = "\u{e14c}";
pub const CONTENT_COPY: &str = "\u{e14d}";
pub const CRISIS_ALERT: &str = "\u{ebe9}";
pub const CYCLONE: &str = "\u{ebd5}";
pub const DEVICE_THERMOSTAT: &str = "\u{e1ff}";
pub const EXPAND_LESS: &str = "\u{e5ce}";
pub const EXPAND_MORE: &str = "\u{e5cf}";
pub const FAST_FORWARD: &str = "\u{e01f}";
pub const HUMIDITY_PERCENTAGE: &str = "\u{f87e}";
pub const INFO: &str = "\u{e88e}";
pub const LANDSCAPE: &str = "\u{e3f7}";
pub const LAYERS: &str = "\u{e53b}";
pub const LOCATION_ON: &str = "\u{e0c8}";
pub const MAP: &str = "\u{e55b}";
pub const MY_LOCATION: &str = "\u{e1b3}";
pub const NAVIGATION: &str = "\u{e55d}";
pub const NOTIFICATIONS: &str = "\u{e7f4}";
pub const OPEN_IN_NEW: &str = "\u{e895}";
pub const PARTLY_CLOUDY_DAY: &str = "\u{f172}";
pub const PAUSE: &str = "\u{e034}";
pub const PLAY_ARROW: &str = "\u{e037}";
pub const POLYLINE: &str = "\u{ebbb}";
pub const PUBLIC: &str = "\u{e80b}";
pub const RADAR: &str = "\u{f04e}";
pub const RADIO: &str = "\u{e03e}";
pub const RADIO_BUTTON_CHECKED: &str = "\u{e837}";
pub const RECEIPT_LONG: &str = "\u{ef6e}";
pub const REFRESH: &str = "\u{e5d5}";
pub const RESTART_ALT: &str = "\u{f053}";
pub const ROTATION_3D: &str = "\u{e84d}";
pub const SATELLITE_ALT: &str = "\u{eb3a}";
pub const SCHEDULE: &str = "\u{e192}";
pub const SEND: &str = "\u{e163}";
pub const SENSORS: &str = "\u{e51e}";
pub const SETTINGS: &str = "\u{e8b8}";
pub const SKIP_NEXT: &str = "\u{e044}";
pub const SKIP_PREVIOUS: &str = "\u{e045}";
pub const STACKED_LINE_CHART: &str = "\u{f22b}";
pub const STORM: &str = "\u{f070}";
pub const SYNC: &str = "\u{e627}";
pub const THERMOSTAT: &str = "\u{f076}";
pub const THUNDERSTORM: &str = "\u{ebdb}";
pub const TORNADO: &str = "\u{e199}";
pub const TUNE: &str = "\u{e429}";
pub const VIEW_IN_AR: &str = "\u{e9fe}";
pub const VIEW_QUILT: &str = "\u{e8f1}";
pub const VISIBILITY: &str = "\u{e417}";
pub const WARNING: &str = "\u{e002}";
pub const WATER_DROP: &str = "\u{e798}";

/// Every icon above (for coverage tests).
pub const ALL: &[&str] = &[
    ACCESSIBILITY_NEW,
    ADD,
    AIR,
    ARROW_OUTWARD,
    AUTO_AWESOME,
    BOOKMARK_ADD,
    CALENDAR_MONTH,
    CELL_TOWER,
    CHECK,
    CHEVRON_LEFT,
    CHEVRON_RIGHT,
    CLOSE,
    CONTENT_COPY,
    CRISIS_ALERT,
    CYCLONE,
    DEVICE_THERMOSTAT,
    EXPAND_LESS,
    EXPAND_MORE,
    FAST_FORWARD,
    HUMIDITY_PERCENTAGE,
    INFO,
    LANDSCAPE,
    LAYERS,
    LOCATION_ON,
    MAP,
    MY_LOCATION,
    NAVIGATION,
    NOTIFICATIONS,
    OPEN_IN_NEW,
    PARTLY_CLOUDY_DAY,
    PAUSE,
    PLAY_ARROW,
    POLYLINE,
    PUBLIC,
    RADAR,
    RADIO,
    RADIO_BUTTON_CHECKED,
    RECEIPT_LONG,
    REFRESH,
    RESTART_ALT,
    ROTATION_3D,
    SATELLITE_ALT,
    SCHEDULE,
    SEND,
    SENSORS,
    SETTINGS,
    SKIP_NEXT,
    SKIP_PREVIOUS,
    STACKED_LINE_CHART,
    STORM,
    SYNC,
    THERMOSTAT,
    THUNDERSTORM,
    TORNADO,
    TUNE,
    VIEW_IN_AR,
    VIEW_QUILT,
    VISIBILITY,
    WARNING,
    WATER_DROP,
];
