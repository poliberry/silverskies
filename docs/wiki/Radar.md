# Radar

## The composite loop (both apps)

The main radar is a worldwide **composite**: national radar networks merged into one picture, served as map tiles by [LibreWXR](https://librewxr.net) (RainViewer-compatible). The app loads the frame index (`/public/weather-maps.json`), then plays the past frames (about two hours, every ten minutes) followed by short **nowcast** frames (an extrapolation of the next ~30 minutes; Pro labels them NOWCAST).

How the loop stays smooth:

- The frame under the playhead is downloaded first, then the frames after it, so what you're looking at appears first.
- Tiles on screen (basemap, terrain, the shown and next radar frame) have their own download queue, separate from prefetching, so they never wait.
- When a frame isn't loaded yet, the last loaded image of that tile is held (no flashing to blank), and the player briefly waits rather than showing a half-drawn frame.
- Moving between frames crossfades.
- Requests for tiles you have panned or zoomed away from are dropped.
- Tiles are cached on disk, so revisiting an area is instant.

The **legend** shows reflectivity in dBZ: roughly 20 light rain, 40 heavy rain, 50+ intense storms and hail cores.

## Satellite map and deep zoom (both apps)

Under the radar you can choose a plain **map** or a **satellite** photo map (Esri World Imagery, worldwide), with place names and, on satellite, roads drawn over it. In Silver Skies use the **Map | Satellite** switch at the top-left of the radar; in Pro use the **IMG** layer button, Settings → Radar → *Satellite imagery map*, or `:layer imagery` (`:layer labels` toggles the names).

You can zoom in like a real map, from the whole globe down to street level (zoom 21). Imagery is sharp to about zoom 19; past that, and wherever a closer tile doesn't exist or is still loading, the nearest coarser tile is enlarged so the map never goes blank.

## Pro: single-site NEXRAD and Level II

`:site KTLX` (or clicking a radar site) switches the radar from the composite to one US radar. The product menu in the control bar then offers:

- **Level II · every tilt** — the radar's full-resolution volume scan, decoded in the app: reflectivity, velocity, storm-relative velocity, correlation coefficient (CC) and differential reflectivity (ZDR). Step through the tilts with − and + (or `:tilt 3`, `:tilt up`); the elevation angle and scan time are shown. Refreshes every few minutes. `:level2 vel` (`:l2 srv`, `:l2 off`, …) does the same from the command line.
- **NWS images · lowest tilt** — the National Weather Service's ready-made images (base reflectivity and velocity, hydrometeor classification, rainfall totals).

**Cross sections:** with a Level II product showing, click the ruler icon in the radar bar (next to the layer toggles) and drag a line across a storm. A window shows the vertical slice under the line, from the ground to 16 km, in the same product: each point is taken from the tilt(s) the radar measured it with (close tilts blended, gaps between far-apart high tilts left empty), so you see the real structure: the height of the core, overhangs, the echo top. A and B mark the ends on the map; closing the window clears the line.

Velocity is green toward the radar and red away; storm-relative velocity subtracts the average motion of the storm cells the radar is tracking, so rotation (green beside red) stands out. Velocity isn't dealiased: very fast winds can fold over to the opposite colour. Low CC inside a strong echo can mean hail or tornado debris.

## Pro: layers

The radar bar, under the function keys in Pro's main window, has playback (back, play/pause, forward, **LIVE**) and the frame slider (or, on a single NEXRAD site, the product, tilt and cross-section tool), the colour scheme, icon toggles for the most-used layers (amber = on) and the legend. Nothing sits over the map. A radar in its own window keeps these controls under its map. Everything else is in Settings → Radar or `:layer`.

| Layer | Source | Coverage |
|---|---|---|
| Composite radar | LibreWXR | Worldwide where radars exist |
| Nowcast frames | LibreWXR | Worldwide |
| IR satellite | LibreWXR | Worldwide |
| Satellite imagery map | Esri World Imagery | Worldwide |
| Place names, roads | Esri reference layers | Worldwide |
| Terrain (shaded relief) | Esri World Hillshade (map mode) | Worldwide |
| Alert polygons | The alert sources ([Alerts](Alerts-and-Weather-Radio)) | Where alerts have areas |
| Alerts in view | LibreWXR bbox alerts | Worldwide (WMO CAP) |
| SPC Day 1 outlook, mesoscale discussions | NOAA Storm Prediction Center | US |
| Storm cells (NEXRAD) | Iowa Environmental Mesonet | US |
| Storm cells (mosaic) | Computed from the composite | Worldwide |
| Storm reports | Iowa Environmental Mesonet (NWS LSRs) | US |
| Tropical cyclones | GDACS | Worldwide |
| Hail swaths, rotation tracks | NOAA MRMS | US |
| Lightning (flashes) | NOAA GOES GLM | The Americas, Atlantic and Pacific |
| Lightning (density) | EUMETSAT MTG Lightning Imager | Europe, Africa |
| Wind barbs | METAR observations (api.weather.gov) | Where stations exist |
| Radar sites | api.weather.gov | US WSR-88D |
| Wind, temperature, precipitation overlays; air quality | OpenWeatherMap (needs a key) | Worldwide |

Hover any symbol for details: storm cells (signatures, hail size, echo tops, motion), reports, cyclones (category, winds, track), alert polygons, discussions.

## Pro: lightning

The **LTG** toggle (bolt icon, `:layer lightning`, Settings → Radar) shows lightning from the last 15 minutes, refreshed every minute:

- **The Americas, the Atlantic and the Pacific:** every flash seen by the GOES-East and GOES-West Geostationary Lightning Mappers, as crosses coloured by age: white (under a minute, circled), yellow (3 min), orange (6), red (10), dark red (older).
- **Europe and Africa:** EUMETSAT's Meteosat Third Generation Lightning Imager, as a flash-density image (5-minute updates).

When there has been a flash within 100 km of your place in the last 15 minutes, a badge at the top of the map reads, e.g., **LTG 12 km NW · 3 min**: the nearest flash's distance, direction and age (red within 16 km, about 10 miles: thunder range). The 3D radar draws recent flashes as bolts above the ground.

Satellite lightning mappers see total lightning (in-cloud and cloud-to-ground) at about 8–10 km resolution, so a flash's position is approximate. There is no free real-time feed for East and South Asia or Australia yet.

## Pro: hail swaths and rotation tracks (US)

Two layers from NOAA's Multi-Radar Multi-Sensor system (MRMS), which merges every US radar every two minutes:

- **Hail swaths** (hail icon, `:layer hail`): the largest hail the radars estimated (MESH) at each point over the period, from 0.5 in: green 0.5″, lime 0.75″, yellow 1″, orange 1.5″, red 2″, maroon 2.75″, white 4″+.
- **Rotation tracks** (cyclone icon, `:layer rotation`): the strongest low-level rotation (0–2 km azimuthal shear) over the period, from 0.008 s⁻¹: lavender, violet, purple, pink, rose, white for 0.03 s⁻¹ and up. A long, narrow, bright streak is the path of a rotating storm, often a tornado's.

Choose how far back they reach in Settings → Radar → *Track period* or with `:period` (30 min, 1, 2, 4, 6 or 24 h; default 2 h). They refresh every few minutes. Hover the map for the value under the pointer, e.g. *hail up to 1.75 in (44 mm) · last 2 h*. Single isolated pixels (usually clutter) are left out. The values are radar estimates: compare with storm reports.

## Pro: NEXRAD single site

`:site KTLX` (or Settings → Radar) switches from the composite to one US radar via NOAA's NCEP GeoServer, with a choice of product (`:product`): **Reflectivity**, **Velocity**, **Hydrometeor classification**, **Storm total precipitation**, **1-hour accumulation**. `:site` with no argument returns to the composite.

## Pro: more radar windows

**F5** (`:radar new`) opens another radar window; each has its own view and layers. `:radar popout` undocks the main one.

## Silver Skies

Silver Skies shows the same composite with play/pause, a time slider, the legend, the place's pin and alert areas — one colour scheme, nothing to configure.
