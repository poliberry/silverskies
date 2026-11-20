# Severe weather and soundings (Pro)

Pro collects the data storm spotters and forecasters watch, from free public services, and shows it in the **Storms** and **Sounding** panes, on the radar and in [3D](3D-Radar-and-Terrain).

## Storm cells

### US: NEXRAD storm attributes

Every WSR-88D runs the NWS storm-cell identification and tracking algorithms. Pro reads their output for all ~160 radars from the [Iowa Environmental Mesonet](https://mesonet.agron.iastate.edu) (`nexrad_attr.geojson`, refreshed every 2 minutes). Each cell has:

| Field | Meaning |
|---|---|
| **TVS** | Tornado vortex signature (strong gate-to-gate rotation near the ground); **ETVS** = elevated, aloft only |
| **MESO** | Mesocyclone (rotating updraft) and its strength rank |
| **Hail** | Maximum expected hail size, probability of hail (POH) and of severe hail (POSH) |
| **dBZ / height** | Maximum reflectivity and the height it occurs |
| **Top** | Echo top |
| **VIL** | Vertically integrated liquid |
| **Motion** | Direction and speed; the map shows projected positions at +15/30/45/60 min |

Cells are ranked by threat: **tornadic** (TVS) → **rotating** (ETVS or strong meso) → **severe** (meso, ≥1" hail or POSH ≥ 50%) → **strong** (≥ 50 dBZ) → ordinary. The map uses an inverted triangle for TVS, rings for mesocyclones, a diamond for hail and dots for strong cells.

### Everywhere else: cells from the radar mosaic

Where no NEXRAD is within 250 km of the place, Pro finds storm cells itself, every 5 minutes, from the global composite:

1. Download the raw-reflectivity version of the two newest composite frames around the place (3×3 tiles at zoom 6, roughly 1,200 km across).
2. Find **cores**: connected areas of ≥ 40 dBZ covering at least 20 km² (centroids weighted by reflectivity).
3. **Track** them between the two frames: each core is matched to the nearest earlier core it could have reached at ≤ 70 kt, giving its motion.

These cells have intensity, core area and motion but no rotation or hail estimates (the composite has no velocity data), and they are labelled as mosaic cells.

## Storms heading for your places

Every 10 seconds Pro projects each storm cell (NEXRAD in the US, mosaic cells elsewhere) along its current motion and checks it against **all your saved places**. A cell counts as *arriving* when its centre will pass within 10 km in the next hour, or is already within 10 km. Only cells worth knowing about count: strong or worse, or at least 45 dBZ. A storm seen by several radars is listed once, with its most severe reading.

- **Storms pane → ARRIVING:** every arrival, grouped by place: when (*~20 min*, *now*), the cell, how close it passes (*passes overhead*, *passes 4 km away*) and what it carries (hail, rotation, dBZ, top).
- **Radar badge:** for the place the window tracks, the most threatening arrival, e.g. *SEVERE STORM ~20 min · passes overhead*, coloured by threat (above the lightning badge).
- **Warnings:** when a severe NEXRAD cell (mesocyclone, tornado signature, ≥ 1" hail or POSH ≥ 50%) is due at a saved place within 45 minutes, Pro logs a warning in the audit log and, with notifications on, shows a desktop notification, once per storm and place.

These are straight-line projections of the storm's centre: storms turn, grow and die, and their rain and hail reach well beyond the centre. Use them as a heads-up, not a forecast; official warnings come from your weather service.

## Storm reports (US)

Local storm reports from the NWS (spotters, emergency managers, the public) for the last 24 hours, via IEM: tornado, funnel cloud, hail (with size), wind damage/gusts, flood, snow/ice. Lettered markers on the map; the newest nearby ones in the Storms pane.

## Tropical cyclones (worldwide)

From [GDACS](https://www.gdacs.org) (the UN/EU disaster alert system, using JTWC and NOAA advisories), every 15 minutes: each active storm's centre, category and maximum wind, alert level (green/orange/red), past track (solid), forecast track (dashed) and wind-speed swaths. Tracks crossing the 180° meridian are kept continuous.

## SPC products (US)

- **Day 1 convective outlook:** the risk areas (TSTM, MRGL, SLGT, ENH, MDT, HIGH), drawn under the radar.
- **Mesoscale discussions:** the outlined areas, with the discussion text on hover.

## Soundings

The **Sounding** pane shows a forecast sounding for the place from [Open-Meteo](https://open-meteo.com)'s pressure-level data (temperature, dew point and wind at 1000–200 hPa), for any hour of the forecast (slider; **now** returns to the current hour).

- **Skew-T log-p:** temperature (red), dew point (green), the surface parcel (dashed yellow), isobars, skewed isotherms (0 °C in blue) and dry adiabats, with the LCL marked.
- **Hodograph:** the wind profile from the surface to 12 km, coloured by layer (0–3 km red, 3–6 green, 6–9 yellow, 9+ cyan), with the 1/3/6 km points and the Bunkers right-mover storm motion (RM). It scales to the winds shown.
- **Indices:** surface-based CAPE and CIN, lifted index, LCL, LFC and EL heights, freezing level, 700–500 hPa lapse rate, precipitable water, 0–1 and 0–6 km shear, 0–1 and 0–3 km storm-relative helicity, supercell composite (SCP) and significant tornado parameter (STP). Values past common severe thresholds are highlighted.

These are model forecasts, not observed balloon soundings.
