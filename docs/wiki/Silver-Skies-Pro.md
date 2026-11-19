# Using Silver Skies Pro

Silver Skies Pro is a weather workstation laid out like a financial terminal: black background, amber labels, white data, blue highlights, and green/red for rising/falling values. Everything can be done from the keyboard.

## The screen

| Area | What it is |
|---|---|
| **Header** | The logo, the active place in the amber "security" field, `WX <GO>`, coordinates, sync time, date and clock. An amber **UPDATE** chip appears here when a new version is out. |
| **Function keys** | `1) SAVE` … `12) SETTINGS`, each with an icon; also bound to **F1–F12**. On narrow windows they shrink to icon + number (hover for the name). |
| **Panes** | Docked in three columns around the central radar. Each has a numbered amber badge and title bar. |
| **Tool strip** (right edge) | One icon per pane: filled = shown, outlined = hidden. Click to show or hide. |
| **Command line + audit log** (bottom) | Type `:` (or click) to enter a command; the log above it shows what happened (data loaded, errors, alerts). |
| **Ticker** (very bottom) | Every saved place scrolling by: temperature, 3-hour trend (▲ rising / ▼ falling), condition. |

Press **Alt** for the menu bar (File, View, Radar, Alerts, Radio, AI, Help); pin it from View.

## Panes

| # | Pane | Shows |
|---|---|---|
| 1 | **Locations** | Saved places with temperature; search box; use my location; ✕ to remove. |
| 2 | **Radar** | The main map (see [Radar](Radar)). In the main window it fills the middle; controls float in a box at the bottom. |
| 3 | **Audit log** | Alerts in effect for the place, filterable, newest first. Click one to open it in its own window. |
| 4 | **Conditions** | Big readouts for temperature, feels-like, humidity and wind (with icons), then precipitation, cloud, UV, sunrise and sunset, and today's outlook. |
| 5 | **7-Day** | Daily low/high with range bars and chance of precipitation. |
| 6 | **Hourly** | The next 24 hours. |
| 7 | **Wx radio** | NOAA Weather Radio (live stream or simulated broadcast) and spoken alerts. See [Alerts and weather radio](Alerts-and-Weather-Radio). |
| 8 | **Storms** | Storm cells near the place (with tornado/mesocyclone/hail signatures), tropical cyclones worldwide, and recent storm reports. See [Severe weather and soundings](Severe-Weather-and-Soundings). |
| 9 | **Sounding** | Forecast Skew-T, hodograph and severe-weather indices for any hour. |
| 10 | **3D radar** | The radar volume over satellite-imaged terrain in 3D. See [3D radar and terrain](3D-Radar-and-Terrain). |

### Arranging panes

- **Move:** hover a pane's title bar for arrow buttons, or drag the title onto another position. `:move <pane> left|center|right [position]`, `:swap <a> <b>`.
- **Hide / show:** ✕ on the title bar, the tool strip, `:hide <pane>` / `:show <pane>`.
- **Undock:** the ↗ button or a double-click on the title opens the pane in its own window (`:popout <pane>`). **DOCK** in that window's title bar (or closing it) puts it back.
- **Reset:** **F11** or `:reset-layout`.

The layout, open windows and their positions are saved between runs.

## Keyboard

| Key | Action |
|---|---|
| `:` | Command line (Tab completes, ↑/↓ history, Esc cancels) |
| F1 | Save the current place |
| F2 | Refresh everything |
| F3 | Radar: jump to the live frame |
| F4 | Radar: centre on the place |
| F5 | New radar window |
| F6 | Toggle nowcast frames |
| F7 | Toggle IR satellite |
| F8 | Toggle storm-cell markers |
| F9 | List alerts |
| F10 | Weather radio on/off |
| F11 | Reset the layout |
| F12 | Settings |
| Ctrl+J | AI assistant |
| Alt | Menu bar |

The full command language is in the [command reference](Pro-Command-Reference).

## Settings

Settings (F12, `:settings`) is a window with a sidebar:

- **General:** weather source (Open-Meteo or AccuWeather + key), units, time format, auto refresh, light/dark, top glow bar, desktop notifications, emphasised storm polygons.
- **Data sources:** LibreWXR radar host; API keys for AccuWeather, WillyWeather (Australian warnings) and OpenWeatherMap (overlays). Keys have **Show/Hide**.
- **Radar:** colour scheme, opacity, smoothing, snow, nowcast, legend, terrain; severe-weather layers (storm cells, storm reports, tropical cyclones, motion arrows, mosaic cells); overlays (alert polygons, alerts in view, IR satellite, SPC outlook and discussions, wind barbs, radar sites); NEXRAD single site and product; OpenWeatherMap overlays.
- **Alerts:** colour, show/hide and reset for every alert type (filterable).
- **Radio & speech:** weather radio mode and feed; spoken alerts (which ones, speaking rate, attention tone, all places).
- **AI assistant:** provider, base URL, key, model ([AI assistant](AI-Assistant)).
- **Themes:** built-in themes with previews; paste or load your own theme JSON; copy the current theme.
- **Developer:** demo alerts.
- **About:** updates, version, data folder, source code, key commands, the raw config (secrets masked).

Changes save immediately. Most settings can also be changed with `:set` (see the [command reference](Pro-Command-Reference)).

## Themes

The default is the amber terminal. Built-ins include Pink Fuchsia, Wintergreen, Nord Blue, Purple Violet, Amber Terminal and Green Terminal, plus a light "paper terminal". A custom theme is JSON:

```json
{ "name": "Neon", "colors": { "a0": "255, 45, 155", "bg": "#180510", "surface": "#220818",
  "text": "#ffffff", "text2": "rgba(255,45,155,0.9)", "danger": "#ff4545", "safe": "#2ed573" } }
```

Slots: `a0` (accent), `bg`, `surface`, `surface3`, `border`, `text`, `text2`, `text3`, `danger`, `safe`. Any can be omitted.

## Typeface

Pro uses **Saira** (the face of its wordmark) with tabular digits so numbers line up, DejaVu for symbols, and Material Symbols for icons.
