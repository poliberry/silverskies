# Silver Skies

A simple, calm weather forecast app for the desktop, written in Rust. Part of the
[Silver Skies](../../README.md) suite — for the full composite forecast, meteorology and radar
workstation, see [Silver Skies Pro](../pro/README.md).

## Look

A native window and title bar (which names the current place), a top header with search, the
places sidebar and the forecast as a bento grid of tiles. Styling follows the Silver Skies web
design: near-black (or soft white) surfaces, square tiles with hairline borders, spaced
uppercase captions in Google Sans Code, a huge thin temperature in Google Sans Flex, Google's V1
weather icons, and an accent that follows the sky. Press **Alt** for the menu bar (File, Edit,
View, Help); View → Always Show Menu Bar pins it.

Shortcuts: F5 or Ctrl+R refresh, Ctrl+F search, Ctrl+1/2/3 Forecast/Radar/Alerts, Ctrl+,
settings, Ctrl+Q quit.

## Features

- **Forecast at a glance** — current conditions with a large temperature and the sky's icon,
  feels-like, high/low and local time; the next 24 hours; a 7-day forecast with temperature
  range bars (click a day for sunrise, sunset and UV).
- **Conditions detail** — humidity, wind, apparent temperature, precipitation, cloud cover in
  words, a UV gauge with sunscreen advice, and the sun's position on its arc with daylight length.
- **Summaries** — a plain-language summary of every forecast, written locally from the data:
  what it's like now, when rain starts or stops, feels-like, wind, tomorrow's change, UV, the
  wettest day ahead and any alerts in effect.
- **Fuzzy search for places and keywords** — one search field finds saved places (typo
  tolerant: `chicgo`, `pairs`), new places from the geocoder, and keywords (`radar`, `alerts`,
  `week`, `°c`, `settings`, `summary`…). It also reads plain questions: “weather in Paris
  tomorrow”, “is it raining at the Eiffel Tower?”, “do I need an umbrella today?” — it jumps to
  the place, highlights the day and answers.
- **Simple radar** — the composite radar loop (past frames plus a short nowcast) on a basemap,
  with play/pause, a time scrubber, alert areas and your place pinned.
- **Weather alerts from the WMO alerting network** — official warnings, watches and advisories
  (CAP feeds from national weather services worldwide, via LibreWXR) for each place, with
  details and safety instructions; severe alerts pulse.
- **Optional AI** — turn it on in Settings with your own Anthropic API key and Silver Skies adds
  a written AI summary and AI answers to questions you type. Only the selected place's forecast
  is sent. It is off by default and nothing changes without a key.
- **Works offline** — the last good forecast for each place is kept and shown if a refresh fails.
- **Tinted by the sky** — the accent follows current conditions; light, dark or system theme;
  °F/°C, 12/24-hour time and auto-refresh from 5 minutes to 2 hours (or off).

## Building

From the repository root:

```
cargo run --release -p silverskies
```

See the [root README](../../README.md) for system libraries.

## Local data

JSON files live in the OS config directory (`Silver Skies`): the shared `config.json` and
`locations.json`, plus `simple.json` for settings only this app has (AI on/off, API key, model)
and `forecast-cache.json` (the last good forecast per place).
The API key is stored only on this computer.

## Data sources

Open-Meteo (forecast and geocoding), LibreWXR (radar and WMO/CAP alerts), OpenStreetMap /
Nominatim, Esri basemaps, and — only when you turn AI on — the Anthropic API.
