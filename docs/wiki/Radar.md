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

## Pro: single-site radars in Europe (OPERA)

About 200 European weather radars in some 20 countries (France, Germany, Spain, the Nordics, Poland, the Netherlands, Belgium, Switzerland, Denmark, the Baltics, Czechia, Slovakia, Hungary, Romania, Croatia, Ireland, Iceland, Malta and more) are listed with the US radars: their triangles appear on the map from zoom 5, and `:site deess` (the radar's ODIM code: country + site) or a click selects one. Pro then reads the radar's newest scans from EUMETNET's [Open Radar Data](https://eumetnet.github.io/openradardata-documentation/) (CC BY 4.0, a new scan every 5 minutes, about 5 minutes behind) and shows them exactly like NEXRAD Level II: every tilt, reflectivity and, where the country sends them, **velocity**, **storm-relative velocity**, **ZDR** and **CC** (the Netherlands and Denmark send full dual-pol; most others reflectivity and velocity). Cross sections, the 3D view and distance measuring all work. Storm-relative velocity uses the motion of the storm cells found in the composite around the radar.

What each radar sends differs: some countries send whole volumes, others one tilt per file (merged here into a volume), and a few radars are retired or offline (the bar then says no recent scan).

## Pro: single-site NEXRAD and Level II

`:site KTLX` (or clicking a radar site) switches the radar from the composite to one US radar. The product menu in the control bar then offers:

- **Level II · every tilt** — the radar's full-resolution volume scan, decoded in the app: reflectivity, velocity, storm-relative velocity, correlation coefficient (CC) and differential reflectivity (ZDR). Step through the tilts with − and + (or `:tilt 3`, `:tilt up`); the elevation angle and scan time are shown. Refreshes every few minutes. `:level2 vel` (`:l2 srv`, `:l2 off`, …) does the same from the command line.
- **NWS images · lowest tilt** — the National Weather Service's ready-made images (base reflectivity and velocity, hydrometeor classification, rainfall totals).

**Distance and bearing:** click the ruler icon in the radar bar and drag a line on the map. The line shows its length (km and miles) and its bearing from A to B; click the icon again to clear it. This works in every radar mode, worldwide.

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
| Storm swaths (max composite echo) | Computed from the LibreWXR composite | Worldwide |
| Lightning (flashes) | NOAA GOES GLM | The Americas, Atlantic and Pacific |
| Lightning (density) | EUMETSAT MTG Lightning Imager | Europe, Africa |
| Wind barbs | METAR observations (api.weather.gov) | Where stations exist |
| Radar sites | api.weather.gov | US WSR-88D |
| Wind, temperature, precipitation overlays; air quality | OpenWeatherMap (needs a key) | Worldwide |

Hover any symbol for details: storm cells (signatures, hail size, echo tops, motion), reports, cyclones (category, winds, track), alert polygons, discussions.

## Pro: satellite imagery (worldwide)

The **SAT** toggle (Settings → Radar → *Satellite*) shows one of:

| Imagery (`:sat …`) | What | Delay |
|---|---|---|
| Infrared, live (`:sat live`, default) | The composite's worldwide infrared, newest image only | Live |
| Infrared (`:sat ir`) | Clean infrared (10.3–10.8 µm), colour-enhanced: cold, high cloud tops (storms) stand out | ~30–60 min |
| True colour / GeoColor (`:sat color`) | What the eye would see by day; infrared with city lights by night | ~30–60 min |
| Air mass (`:sat airmass`) | Red/brown = dry, descending air (jet streams, dry slots), green = warm moist air, blue/purple = cold air | ~30–60 min |

The looped kinds come from every weather satellite in geostationary orbit, each drawn over its own longitudes so the whole globe is covered by the best-placed one: **GOES-West** (central Pacific to the Rockies), **GOES-East** (the Americas and Atlantic), **Meteosat** (Europe and Africa), **Meteosat IODC** (the Middle East and India) and **Himawari** (East Asia and Australia). Images are 10 minutes apart and follow the radar loop: each frame shows the satellite image from the same time, or the newest one where the satellite hasn't caught up yet. The credit line gives the image time. GOES and Himawari come from NASA GIBS, Meteosat from EUMETSAT's EUMETView. Himawari has no true colour there: its sector shows infrared in that mode.

## Pro: lightning

The **LTG** toggle (bolt icon, `:layer lightning`, Settings → Radar) shows lightning from the last 15 minutes, refreshed every minute:

- **The Americas, the Atlantic and the Pacific:** every flash seen by the GOES-East and GOES-West Geostationary Lightning Mappers, as crosses coloured by age: white (under a minute, circled), yellow (3 min), orange (6), red (10), dark red (older).
- **Europe and Africa:** EUMETSAT's Meteosat Third Generation Lightning Imager, as a flash-density image (5-minute updates).

When there has been a flash within 100 km of your place in the last 15 minutes, a badge at the top of the map reads, e.g., **LTG 12 km NW · 3 min**: the nearest flash's distance, direction and age (red within 16 km, about 10 miles: thunder range). The 3D radar draws recent flashes as bolts above the ground.

**Anywhere else (with your own key):** Settings → Data sources → *Non-commercial sources* takes a free [Xweather](https://signup.xweather.com/developer) developer key (client ID and secret). Its free plan is for non-commercial use, so it stays off until you allow such sources. Pro then checks Vaisala's worldwide lightning network (GLD360) for strikes within 100 km of your place, where the GOES satellites don't give individual flashes (Europe, Africa, Asia, Australia), every 30 minutes by default. Each check returns the last 5 minutes and costs 10 of the free plan's 15,000 monthly accesses: every 30 minutes uses about 14,400. The strikes join the map and the nearest-strike badge. *Check now* tests the key.

Satellite lightning mappers see total lightning (in-cloud and cloud-to-ground) at about 8–10 km resolution, so a flash's position is approximate. Without a key there is no free real-time feed for East and South Asia or Australia. Blitzortung's community network isn't used: its terms forbid storm-warning use and require the project's permission.

## Pro: hail swaths and rotation tracks (US)

Two layers from NOAA's Multi-Radar Multi-Sensor system (MRMS), which merges every US radar every two minutes:

- **Hail swaths** (hail icon, `:layer hail`): the largest hail the radars estimated (MESH) at each point over the period, from 0.5 in: green 0.5″, lime 0.75″, yellow 1″, orange 1.5″, red 2″, maroon 2.75″, white 4″+.
- **Rotation tracks** (cyclone icon, `:layer rotation`): the strongest low-level rotation (0–2 km azimuthal shear) over the period, from 0.008 s⁻¹: lavender, violet, purple, pink, rose, white for 0.03 s⁻¹ and up. A long, narrow, bright streak is the path of a rotating storm, often a tornado's.

**Outside the US** (anywhere MRMS doesn't reach) the hail toggle shows **storm swaths** instead, worked out by the app from the worldwide composite around your place: the strongest echo at each point over the period (the composite keeps about two hours), from 50 dBZ: green 50, yellow 55, orange 60, red 65, purple 70+. Cores of 55 dBZ and more usually carry hail, so the swaths show where hail most likely fell; hover for the value (*strongest echo 58 dBZ (hail likely) · last 2 h*). It is an estimate from a composite, coarser than MRMS's hail sizes. Tracks and swaths have a dark outline so they stand out over the radar.

Choose how far back they reach in Settings → Radar → *Track period* or with `:period` (30 min, 1, 2, 4, 6 or 24 h; default 2 h). They refresh every few minutes. Hover the map for the value under the pointer, e.g. *hail up to 1.75 in (44 mm) · last 2 h*. Single isolated pixels (usually clutter) are left out. The values are radar estimates: compare with storm reports.

## Pro: 4-panel

The **4-PANEL** pane (grid icon in the tool strip, `:show 4panel`) shows one radar's scan four ways at once, the way storm chasers and forecasters read a storm: by default reflectivity, velocity, correlation coefficient and differential reflectivity, each with its own legend; pick another product for any map from its menu (storm-relative velocity, for example). All four use the same tilt (◀ ▶ in the pane, or `:tilt`), pan and zoom together, and mirror the pointer as a cross, so a hook in reflectivity, a couplet in velocity and a debris signature in CC line up.

It uses the radar selected with `:site` (US NEXRAD or European OPERA), or else the nearest one within 230 km of your place that's working. Velocity, CC and ZDR are drawn only where the same tilt has at least 5 dBZ of reflectivity, hiding clear-air noise (the same applies to the radar pane's Level II view).

## Pro: placefiles

Pro reads **placefiles**, the plain-text overlay format GRLevelX made popular and many feeds publish: spotter positions, local reports, warning tracks, station plots, storm attributes, your own annotations. Add one in Settings → Radar → *Placefiles* (paste its URL) or with `:placefile add URL`; `:placefile list`, `:placefile toggle N` and `:placefile remove N` manage them.

What's drawn: lines, filled polygons and triangles, text, places, and icons from icon sheets (rotated, e.g. wind barbs; white-on-black sheets are drawn in the file's colour, as GRLevelX does), including `Object` groups placed by pixel offsets around a point. Each item appears only when you're zoomed in past its `Threshold`. Each file refreshes on its own `Refresh` interval (5 minutes if it doesn't say; at least 30 seconds). Hover an icon, a text or a line for its details. `Image` and `Raster` lines aren't supported.

Some public placefiles:

| Feed | URL |
|---|---|
| Spotter Network positions | `https://www.spotternetwork.org/feeds/gr.txt` |
| IEM NEXRAD storm attributes | `https://mesonet.agron.iastate.edu/request/grx/l3attr.txt` |
| IEM warning time-motion-location | `https://mesonet.agron.iastate.edu/request/grx/time_mot_loc.txt` |
| IEM Iowa station plots | `https://mesonet.agron.iastate.edu/request/grx/asos.php` |

## Pro: NEXRAD single site

`:site KTLX` (or Settings → Radar) switches from the composite to one US radar via NOAA's NCEP GeoServer, with a choice of product (`:product`): **Reflectivity**, **Velocity**, **Hydrometeor classification**, **Storm total precipitation**, **1-hour accumulation**. `:site` with no argument returns to the composite.

## Pro: more radar windows

**F5** (`:radar new`) opens another radar window; each has its own view and layers. `:radar popout` undocks the main one.

## Silver Skies

Silver Skies shows the same composite with play/pause, a time slider, the legend, the place's pin and alert areas — one colour scheme, nothing to configure.
