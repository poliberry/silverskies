# 3D radar and terrain (Pro)

The **3D RADAR** pane shows the atmosphere around a place as a 3D volume above the real landscape, with storms and tornado signatures marked. Open it from the tool strip (cube icon), with `:show 3d`, or in its own window with `:popout 3d` (recommended: it likes space).

## What you see

- **Terrain:** a 400 × 400 km elevation model of the area, draped with **satellite imagery** and lit by the sun, under a sky. The satellite button switches to colouring by height instead.
- **Radar echoes:** reflectivity in 3D, coloured like the 2D radar (green → yellow → red → magenta/white for the most intense cores). Weak echoes are see-through; cores are solid.
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
| Anywhere else | The global radar **composite** (2D) | **Estimated** from intensity: stronger echoes are drawn taller (about 2 km for light rain up to ~15 km for intense cores). Shows where storms are and how strong, not their real structure. |

Terrain and imagery are worldwide everywhere.

### How a NEXRAD volume becomes 3D

1. List the site's files for today on `unidata-nexrad-level2` (S3) and download the newest volume (~5–20 MB; a new one about every 5 minutes, published ~5–7 minutes after the scan).
2. Decompress the bzip2 records and read every **message 31** radial: azimuth, elevation and the reflectivity gates (0.25 km apart, out to ~460 km).
3. Place each gate with the standard **4/3-earth beam model**: height `h = √(r² + (kₑa)² + 2r·kₑa·sin θ) − kₑa` and ground range `s = kₑa·asin(r·cos θ / (kₑa + h))`.
4. Grid the gates into 1.25 km × 0.5 km voxels (keeping the maximum dBZ per voxel), up to 140,000 of them.

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
- Imagery: Esri World Imagery — Esri, Maxar, Earthstar Geographics.
- Radar volumes: [NOAA NEXRAD Level II on AWS](https://registry.opendata.aws/noaa-nexrad/) (`unidata-nexrad-level2`).
- Composite: LibreWXR.

Rendering is done on the CPU (a perspective camera, then terrain and echoes depth-sorted and drawn as meshes), so it works on any graphics driver; a release build stays interactive with the full voxel count.
