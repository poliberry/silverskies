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

## Pro: layers

The floating control box in Pro has playback (back, play/pause, forward, **LIVE**), a frame slider, the colour scheme and icon toggles for the most-used layers. Everything else is in Settings → Radar or `:layer`.

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
| Wind barbs | METAR observations (api.weather.gov) | Where stations exist |
| Radar sites | api.weather.gov | US WSR-88D |
| Wind, temperature, precipitation overlays; air quality | OpenWeatherMap (needs a key) | Worldwide |

Hover any symbol for details: storm cells (signatures, hail size, echo tops, motion), reports, cyclones (category, winds, track), alert polygons, discussions.

## Pro: NEXRAD single site

`:site KTLX` (or Settings → Radar) switches from the composite to one US radar via NOAA's NCEP GeoServer, with a choice of product (`:product`): **Reflectivity**, **Velocity**, **Hydrometeor classification**, **Storm total precipitation**, **1-hour accumulation**. `:site` with no argument returns to the composite.

## Pro: more radar windows

**F5** (`:radar new`) opens another radar window; each has its own view and layers. `:radar popout` undocks the main one.

## Silver Skies

Silver Skies shows the same composite with play/pause, a time slider, the legend, the place's pin and alert areas — one colour scheme, nothing to configure.
