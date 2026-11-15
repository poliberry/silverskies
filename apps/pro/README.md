# Silver Skies Pro

The over-engineered one: a composite forecast, meteorology and radar workstation, written
entirely in Rust. Part of the [Silver Skies](../../README.md) suite — for a simple, calm
forecast app, see [Silver Skies](../silverskies/README.md).

Pro is laid out like an engineering workstation: a menu bar and a toolbar of block buttons
across the top, pane stacks docked flush to the left and right, the radar as the central
viewport, centre panes in a bottom dock, a command console beneath it and a tool strip down the
right edge that shows and hides panes. It is black, with white, blue and violet text, an
electric-blue primary accent and a violet secondary accent; every control is a hard-edged
block. Conditions read out like a digital readout (DRO).

Everything is also driven by an embedded `:` command line — locations, layout, radar layers,
alerts, theming and the weather radio. Panes can be dragged between docks, hidden, or popped out
into their own OS windows, and you can open as many independent radar windows as you like.

## Features

- **Global radar** — LibreWXR (RainViewer-compatible) composite radar with past frames + nowcast,
  playback timeline, nine color schemes, storm cells/arrows, satellite infrared, and a dBZ legend.
- **NEXRAD single-site products** (US) — super-res reflectivity, velocity, hydrometeor
  classification, storm-total and 1-hour accumulation via NCEP WMS, with range rings, VCP status
  and live/stale indicators.
- **Alerts everywhere** — NWS (US), Environment Canada, BOM via WillyWeather (AU), SPC mesoscale
  discussions and convective outlooks, and WMO/CAP feeds worldwide via LibreWXR, merged and
  de-duplicated into one audit log.
- **Alert polygons** — zone-based advisories drawn as outlined fills; **storm-based warnings**
  (tornado, severe thunderstorm, flash flood; PDS, emergency and confirmed tiers) drawn as an
  emphasized, pulsing "storm" layer above them.
- **Weather alerts over TTS** — newly arrived alerts for your active (and optionally all saved)
  locations are read aloud with native text-to-speech, preceded by the SAME attention tone, with
  a severity filter and adjustable speech rate. Any alert can be spoken on demand (`:speak`).
- **Weather radio** — simulated NOAA Weather Radio (TTS + tones) or a live NWR stream chosen by
  distance, with real-time SAME header decoding from the audio.
- **Current conditions & 7-day / hourly forecast** (Open-Meteo, no key needed; AccuWeather
  optional), today's outlook, UV, wind barbs from METARs, OpenWeatherMap overlays and AQI.
- **Workstation UI** — docked, movable/hideable/pop-out panes, layout persisted between runs,
  built-in and custom JSON themes (plus condition-tinted default accent), 12/24h and °F/°C,
  desktop notifications, and command-line completion/history.

Alert polygons on the radar are labelled only while you hover over them.

## Commands

Press `:` (or `Ctrl+K`) to open the command bar. A few examples:

```
:goto Tucson, AZ          :add                     :units c
:move radar right 0       :hide forecast           :popout audit-log
:radar new                :product velocity        :site kfsx
:layer satellite on       :radar scheme 4          :play
:alerts                   :speak 2                 :radio live
:theme nord-blue          :set tts.enabled true    :help
```

`:help` lists everything; `Tab` completes commands and arguments.

## Building

From the repository root:

```
cargo run --release -p silverskies-pro
```

See the [root README](../../README.md) for system libraries.

## Local data

Pro keeps its JSON files in its own OS config directory (`Silver Skies Pro`): `config.json`,
`locations.json`, `session.json`, `dedupe.json`. On first run it imports `config.json`,
`locations.json` and `session.json` from the pre-split `Silver Skies` directory (never
overwriting), and settings from the old Electron app (`%APPDATA%\silver-skies-desktop\`).

## Data sources

Open-Meteo, api.weather.gov (NWS/SPC/NEXRAD metadata), Environment and Climate Change Canada,
LibreWXR, NOAA NCEP GeoServer, OpenStreetMap/Nominatim, CARTO basemaps, OpenWeatherMap (optional
key), WillyWeather (optional key), noaaweatherradio.org.
