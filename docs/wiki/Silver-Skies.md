# Using Silver Skies

Silver Skies is the calm, everyday app: one place at a time, big readable numbers, and a search bar you can ask questions.

## The window

- **Sidebar (left):** your saved places with their current temperature, and **Use my location**. Click a place to show it.
- **Header (top):** the search bar, the date and time, refresh, °F/°C, and Settings (⚙).
- **Tabs:** **Forecast**, **Radar** and **Alerts** (Ctrl+1/2/3; ⌘ on macOS). The Alerts tab shows a count when alerts are active.

The colour scheme is generated from the sky: the accent follows the current conditions (or a theme you pick), as a full Material 3 tonal palette in light or dark mode.

## Forecast

From top to bottom:

- **Now:** the current temperature, condition, feels-like, today's high and low, and the weather icon on an animated shape.
- **Summary:** a plain-language summary written on your computer from the forecast (no AI needed), with highlights such as rain timing. With AI turned on, **Summarize** adds an AI-written summary underneath.
- **Stat tiles:** humidity, wind, wind direction, precipitation in the last hour.
- **Hourly forecast:** the next 24 hours as pills (temperature, icon, chance of rain); "Now" is filled.
- **7-day forecast:** one row per day with a cold-to-warm range bar and the chance of rain.
- **UV index, cloud cover, sun** (sunrise, sunset, daylight, the sun's position on its arc) and **feels like**.

When a severe alert is active, a red banner appears above the forecast and the glow at the top of the window turns red.

## Radar

A single looping radar map: the global composite (past frames plus a short nowcast), the place's pin and any alert areas. Play/pause and a time slider sit at the bottom; the legend shows rain intensity in dBZ. Drag to pan, scroll to zoom.

## Alerts

Official alerts for the place, newest and most severe first. Each card shows the event, severity and expiry; open it for the full text and safety advice. When there are none, the tab says so. Sources by country are listed on [Alerts and weather radio](Alerts-and-Weather-Radio).

## Search

The search bar does three things, ranked as you type:

| Type | Example | Result |
|---|---|---|
| A place | `paris`, `Tucson, AZ`, `48.85 2.35` | Matching places from the geocoder (and your saved ones); pick one to show it. |
| A keyword | `radar`, `alerts`, `week`, `settings`, `celsius`, `refresh` | Jumps to that view or runs that action. Typos are fine (`radr` finds Radar). |
| A question | `will it rain tomorrow?`, `do I need an umbrella?`, `what to wear in Seattle` | An answer card, written locally from the forecast. With AI on, the AI's answer follows. |

Use ↑/↓ to move through suggestions, Enter to choose, Esc to close.

## Menus and shortcuts

Press **Alt** to reveal the menu bar (File, Edit, View, Help); Alt or Esc hides it, or pin it with **View → Always Show Menu Bar**.

| Shortcut | Action |
|---|---|
| F5 or Ctrl+R | Refresh |
| Ctrl+F | Search |
| Ctrl+1 / 2 / 3 | Forecast / Radar / Alerts |
| Ctrl+, | Settings |
| Ctrl+J | AI assistant |
| Ctrl+Q | Quit |

(⌘ instead of Ctrl on macOS.)

## Settings

- **Units and time:** °F/°C (with mph or km/h), 12- or 24-hour clock.
- **Appearance:** light, dark or follow the system; colour theme ("Match the sky" or a fixed palette); **motion** (animations can be reduced).
- **Weather:** the forecast source — Open-Meteo (default, free, no key) or AccuWeather (needs your API key; see [Data sources](Data-Sources)) — and the refresh interval.
- **AI:** optional; see [AI assistant](AI-Assistant). You can also have an AI summary written automatically when a forecast loads.
- **Updates:** automatic checks on or off, check now, install.
- **Developer:** add demo alerts to see how alerts look (never saved).

Settings are saved as you change them, in the folder shown at the bottom of the dialog (see [Settings and files](Settings-and-Files)).

## AI assistant

**Ctrl+J** opens the assistant in its own window: ask about the forecast for your places in plain language. See [AI assistant](AI-Assistant).
