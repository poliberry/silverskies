# Alerts and weather radio

## Where alerts come from

| Region | Source | Notes |
|---|---|---|
| United States | [api.weather.gov](https://www.weather.gov/documentation/services-web-api) (NWS) | Active alerts for the point, with zone and storm polygons |
| Canada | [Environment and Climate Change Canada](https://api.weather.gc.ca) | Alerts intersecting the point |
| Worldwide | [LibreWXR](https://librewxr.net) | National weather services' CAP alerts via the WMO alerting network |
| Australia | [WillyWeather](https://www.willyweather.com.au/info/api.html) | Bureau of Meteorology warnings; needs a WillyWeather API key |
| United States | NOAA SPC | Mesoscale discussions (Pro) |

Alerts for each saved place are refreshed with its forecast. Duplicates from different sources are merged, and alerts you've already been notified about aren't repeated (the dedupe list is kept in `dedupe.json`).

## Classes and colours

Every alert is classified (for example *Tornado Warning*, *Tornado Warning (PDS)*, *Severe Thunderstorm Warning (Destructive)*, *Flash Flood Emergency*, *Winter Storm Warning*, ECCC colour levels …) and drawn in its standard colour. In Pro, Settings → Alerts lets you recolour or hide any class; hidden classes disappear everywhere (lists, map and notifications).

## How alerts are shown

- **Silver Skies:** the Alerts tab (with a count), a banner over the forecast for severe alerts, and a red glow along the top of the window.
- **Pro:** the audit log pane, alert windows with the full text, polygons on the radar, the top glow, and desktop notifications. "Emphasised storm polygons" draws warning polygons (tornado, severe thunderstorm, flash flood and their PDS/emergency tiers) more boldly.

## Spoken alerts and weather radio (Pro)

- **Spoken alerts (TTS):** new alerts are read aloud with your system's voice, filtered by severity (extreme only … all), at a chosen rate, optionally after the attention tone, and optionally for every saved place.
- **Weather radio:**
  - **Live:** streams the nearest NOAA Weather Radio transmitter (from the [noaaweatherradio.org](https://noaaweatherradio.org) directory) or a stream URL you give.
  - **Simulated:** generates a NOAA-style broadcast for alerts: SAME header bursts (AFSK, 520.83 baud; mark 2083.3 Hz, space 1562.5 Hz), the 1050 Hz attention tone, the spoken message, and the end-of-message bursts.

`:radio …`, `:speak` and `:mute` control these from the command line.

## Demo alerts

Settings → Developer (both apps) adds local demo alerts of any type so you can see how they look; they are never saved or sent anywhere.
