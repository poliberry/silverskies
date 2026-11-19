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
- **Wind barbs** in a column on the right (pennant = 50 kt, full feather = 10 kt, half = 5 kt), thinned so they never overlap.
- **Hodograph:** the wind profile from the surface to 12 km, coloured by layer (0–3 km red, 3–6 green, 6–9 yellow, 9+ cyan), with the 1/3/6 km points and the Bunkers right-mover storm motion (RM). It scales to the winds shown.
- **Indices:** surface-based CAPE and CIN, lifted index, LCL, LFC and EL heights, freezing level, 700–500 hPa lapse rate, precipitable water, 0–1 and 0–6 km shear, 0–1 and 0–3 km storm-relative helicity, supercell composite (SCP) and significant tornado parameter (STP). Values past common severe thresholds are highlighted.

These are model forecasts, not observed balloon soundings.
