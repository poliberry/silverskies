# 3D radar and terrain (Pro)

The **3D RADAR** pane shows the atmosphere around a place as a 3D volume above the real landscape, with storms and tornado signatures marked. Open it from the tool strip (cube icon), with `:show 3d`, or in its own window with `:popout 3d` (recommended: it likes space).

## What you see

- **Terrain:** the real landscape and nothing else (no streets or buildings): shaded by elevation (sea, lowland, hills, mountains, peaks), lit from the north-west, with faint contour lines every 100 m, fading into the sky with distance.
- **Satellite** (toggle): Esri World Imagery draped on the terrain instead. Free imagery is flat photos, which fall apart up close or seen from the side, so while it's on the view stays at least 30 km away and at least 40° above the horizon.
- **Zoom like a map:** scroll from the 400 km overview down to a few hundred metres. Close to the camera the app streams finer elevation (AWS Terrain Tiles, to level 15) tile by tile, spending detail where you're looking. Height exaggeration eases to true scale as you get close. Close up, the elevation data's lumps where cities and forests stand are smoothed away.
- **Radar volume:** reflectivity rendered as a true volume on the GPU, coloured like the 2D radar (green → yellow → red → magenta/white for the most intense cores). Light rain is a thin veil; cores are nearly solid. Echoes behind hills are hidden by them.
- **Storm cells:** a line from the ground to the echo top, coloured by threat (yellow strong, orange severe, magenta rotating, red tornadic).
- **Tornado vortex signatures:** a red funnel from cloud base to the ground.
- **Mesocyclones:** a magenta ring at mid-levels.
- **Storm reports (US):** tornado (T), funnel cloud (C) and hail (H) pins on the ground.
- **The radar** (green dot) with 50/100/150/200 km range rings, and the **place** (white).
- Hover any marker for details.

## Measured vs. estimated

How the echoes are placed depends on where you are, and the pane always says which:

| Where | Radar data | Heights |
|---|---|---|
| Within 230 km of a US WSR-88D (NEXRAD) | The radar's newest **Level II volume scan** from NOAA's public archive on AWS | **Measured.** Every tilt the radar swept (0.5° up to 19.5°), each echo placed by beam geometry. |
| Within 230 km of a European OPERA radar (~200 radars in ~20 countries) | The radar's newest **volume scan** from EUMETNET's Open Radar Data (ODIM HDF5) | **Measured**, the same way. If the nearest radar is offline, the next nearest (up to three) is tried. |
| Anywhere else | The global radar **composite** (2D) | **Estimated** from intensity: stronger echoes are drawn taller (about 2 km for light rain up to ~15 km for intense cores). Shows where storms are and how strong, not their real structure. |

Terrain and imagery are worldwide everywhere.

### How a NEXRAD volume becomes 3D

1. List the site's files for today on `unidata-nexrad-level2` (S3) and download the newest volume (~5–20 MB; a new one about every 5 minutes, published ~5–7 minutes after the scan).
2. Decompress the bzip2 records and read every **message 31** radial: azimuth, elevation and the reflectivity gates (0.25 km apart, out to ~460 km).
3. Place each gate with the standard **4/3-earth beam model**: height `h = √(r² + (kₑa)² + 2r·kₑa·sin θ) − kₑa` and ground range `s = kₑa·asin(r·cos θ / (kₑa + h))`.
4. Keep the scan itself, tilt by tilt, laid out by azimuth (0.5° bins) and range, and hand it to the GPU.
5. **Ray-march** it, in the spirit of [OpenStorm](https://github.com/JordanSchlick/OpenStorm): for every pixel a ray steps through the scene; at each step the point is turned back into the radar's own coordinates (range, azimuth and beam elevation, with the same refraction model), the two tilts around it are sampled and blended (tilts more than 3° apart each fill only their own ~1° beam, so the gaps between high tilts stay honest), and the result is coloured and accumulated front to back until the ray is opaque or hits the ground.

### Products (single-site radars)

The product menu in the toolbar colours the volume by any moment the scan carries; opacity always follows reflectivity (and the ≥ dBZ slider), so you see the product where the echoes are:

- **Reflectivity** (dBZ).
- **Velocity** (m/s): green toward the radar, red away. Not dealiased: very fast winds can fold over to the other colour.
- **Storm-relative velocity:** velocity minus the average motion of the storm cells the radar is tracking, which makes rotation stand out.
- **Correlation coefficient (CC):** near 1 in rain and snow; low values within a strong echo can mean hail or tornado debris.
- **Differential reflectivity (ZDR):** large, flattened drops read high; hail and tumbling debris read near zero.

Split cuts (one scan with reflectivity and dual-pol, another with velocity, at the same tilt) are merged onto one layout, and products are sampled gate by gate rather than blended.

Outside NEXRAD coverage the estimated columns are gridded (2 km × 0.5 km) and ray-marched the same way. On computers without OpenGL 3, the pane falls back to drawing echoes as soft points.

## Controls

| Control | Action |
|---|---|
| Drag | Orbit (rotate and tilt) |
| Shift+drag or right-drag | Pan |
| Scroll | Zoom |
| Double-click / ⟲ | Reset the view |
| Terrain · Satellite · Radar · Storms buttons | Show or hide each |
| Rotate button | Slow automatic spin |
| ↻ | Load the newest scan now (it also refreshes every 5 minutes) |
| **≥ dBZ** slider | Hide echoes weaker than this (default 20) |
| **height ×** slider | Vertical exaggeration (default 6×); real storms are ~15 km tall over a 400 km scene, so some exaggeration helps |

## Data and credits

- Terrain: [AWS Terrain Tiles](https://registry.opendata.aws/terrain-tiles/) (Terrarium encoding: `height = R·256 + G + B/256 − 32768` m).
- Radar volumes: [NOAA NEXRAD Level II on AWS](https://registry.opendata.aws/noaa-nexrad/) (`unidata-nexrad-level2`); Europe: [EUMETNET OPERA Open Radar Data](https://eumetnet.github.io/openradardata-documentation/) (CC BY 4.0).
- Composite: LibreWXR.

Rendering is done on the CPU (a perspective camera, then terrain and echoes depth-sorted and drawn as meshes), so it works on any graphics driver; a release build stays interactive with the full voxel count.
