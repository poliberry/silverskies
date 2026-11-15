# Silver Skies

## Know before it hits.

Silver Skies is a suite of weather forecasting and radar applications, written in Rust.

| | App | What it is |
|---|---|---|
| <img src="apps/silverskies/assets/icon-dark.png" width="48" alt=""> | [**Silver Skies**](apps/silverskies/README.md) | A simple, calm weather forecast: the forecast at a glance, plain-language summaries, fuzzy search for places and keywords, a simple radar loop, WMO alerts and optional AI summaries and answers. |
| <img src="apps/pro/assets/icon-dark.png" width="48" alt=""> | [**Silver Skies Pro**](apps/pro/README.md) | The over-engineered one: a composite forecast, meteorology and radar workstation with docked panes, NEXRAD products, every alert feed, storm polygons, weather radio, TTS, multiple radar windows and a command line. |

## Building

Requires a stable Rust toolchain (1.80+).

```
cargo run --release -p silverskies        # Silver Skies
cargo run --release -p silverskies-pro    # Silver Skies Pro
```

On Linux you need the usual egui/cpal/speech-dispatcher system libraries:
`libxkbcommon-dev libwayland-dev libgtk-3-dev libasound2-dev libspeechd-dev`.

## Repository layout

```
apps/
  silverskies/  Silver Skies — the simple app (eframe)
  pro/          Silver Skies Pro — the workstation app (eframe)
crates/
  ss-core/      pure domain logic: alert taxonomy & colors, units, WMO codes, themes,
                SAME encoder/decoder, geometry, command grammar  (fully unit tested)
  ss-net/       API clients: NWS, ECCC, LibreWXR, SPC, Open-Meteo, Nominatim, OWM, NCEP, NWR
  ss-store/     JSON config/locations/session files per app, atomic writes, dedupe store
  ss-audio/     tone synthesis, native TTS, live stream player
  ss-map/       slippy-map engine for egui: tile cache, Mercator viewport, raster/WMS/polygon/
                marker/wind-barb layers
  ss-assets/    shared UI assets: bundled fonts, Pixel Weather icons, SVG rasterizing
tools/
  render-icons/ regenerates each app's PNG icons from its brand SVGs (`cargo run -p render-icons`)
```

Each app keeps its brand marks in `apps/<app>/assets/brand/` and its installer/bundle metadata
in `apps/<app>/dist/`.

## Releases

Pushing a `v*` tag that matches the workspace version builds installers of both apps for
Windows (installer + portable zip), macOS (Apple silicon and Intel `.dmg`/`.zip`) and Linux
(AppImage + `.deb`) and publishes them together in one GitHub Release.
