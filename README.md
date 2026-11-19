<img width="300" height="120" alt="Frame(1)" src="https://github.com/user-attachments/assets/6a0f0428-fbe4-428c-a3ab-b4a46d2b2a48" />

## Know before it hits.

Silver Skies is a suite of weather forecasting and radar applications, written in Rust.

| | App | What it is |
|---|---|---|
| <img src="apps/silverskies/assets/icon-dark.png" width="48" alt=""> | [**Silver Skies**](apps/silverskies/README.md) | A simple, calm weather forecast: the forecast at a glance, plain-language summaries, fuzzy search for places and keywords, a simple radar loop, WMO alerts and optional AI summaries and answers. |
| <img src="apps/pro/assets/icon-dark.png" width="48" alt=""> | [**Silver Skies Pro**](apps/pro/README.md) | The over-engineered one: a composite forecast, meteorology and radar workstation with docked panes, NEXRAD products, every alert feed, storm polygons, weather radio, TTS, multiple radar windows and a command line. |

## Documentation

How both apps work — every pane, command, data source and setting — is in the
[wiki](https://github.com/poliberry/silverskies/wiki) (source: [`docs/wiki/`](docs/wiki)).

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
  ss-assets/    shared UI assets: bundled fonts (Google Sans family, Saira, DejaVu, a
                Material Symbols subset), Google's V1 weather icons, SVG rasterizing, the
                Alt-revealed menu bar
  ss-update/    in-app updates from GitHub Releases
tools/
  render-icons/ regenerates each app's PNG icons from its brand SVGs (`cargo run -p render-icons`)
```

Each app keeps its brand marks in `apps/<app>/assets/brand/` and its installer/bundle metadata
in `apps/<app>/dist/`.

## Releases

Every push to `main` builds installers of both apps for Windows (installer + portable zip),
macOS (Apple silicon and Intel `.dmg`/`.zip`) and Linux (AppImage + `.deb`) and publishes them
together as a new GitHub Release, tagged `v<version>.<build>` (for example `v2.0.2.1`) with the
commits since the previous release as its notes. Builds count from 1 for each version, and the
build number is shown in each app's About screen ("2.0.2 (build 1)"). Bump `version` in the
workspace `Cargo.toml` to start a new version at build 1. Running the Release workflow by hand
builds the same installers as downloadable workflow artifacts without publishing.

### Updates

Both apps check GitHub's latest release at startup and once a day (Settings → Updates in Silver
Skies, Settings → About in Pro; it can be turned off), compare its tag with their own version and
build, and offer the update. The download is checked against the SHA-256 GitHub publishes for each
file, then installed the way the app was installed:

| Installed from | Update |
|---|---|
| Windows installer | runs the new installer silently; it restarts the app |
| macOS `.zip` / `.dmg` (in a `.app`) | replaces the `.app` bundle in place and restarts it |
| Linux AppImage | replaces the AppImage file and restarts it |
| Linux `.deb` | downloads the new `.deb` and opens it in the software installer |
| anything else (portable zip, a build directory) | opens the release page |

Local development builds are only offered a newer *version*, not newer builds of their own.
`SS_UPDATE_URL` points the check at a local mock of the GitHub API, for testing.

## Credits

Weather icons: Google's V1 weather icon set, from
[bignutty/google-weather-icons](https://github.com/bignutty/google-weather-icons). Fonts:
Google Sans, Google Sans Flex and Google Sans Code (SIL Open Font License), DejaVu (Bitstream
Vera license).
