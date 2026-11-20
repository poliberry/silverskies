# Data sources

Everything the apps show comes from these services. None needs an account except where a key is noted; keys are optional extras.

| Data | Service | Key | Coverage | Used by |
|---|---|---|---|---|
| Forecast (current, hourly, 7-day) | [Open-Meteo](https://open-meteo.com) | No | Worldwide | Both (default) |
| Forecast (alternative) | [AccuWeather](https://developer.accuweather.com) Core API | **Yes** | Worldwide | Both (optional) |
| Place search | Open-Meteo geocoding, with [Nominatim / OpenStreetMap](https://nominatim.org) as fallback | No | Worldwide | Both |
| "Use my location" | [ipapi.co](https://ipapi.co) IP geolocation (approximate) | No | Worldwide | Both |
| Radar composite, nowcast, IR satellite | [LibreWXR](https://librewxr.net) | No | Worldwide | Both |
| Alerts | NWS, ECCC, LibreWXR (WMO CAP) | No | US, Canada, worldwide | Both |
| Australian warnings | [WillyWeather](https://www.willyweather.com.au) (BoM) | **Yes** | Australia | Both (optional) |
| Basemap | Esri, HERE, Garmin, © OpenStreetMap contributors | No | Worldwide | Both |
| Shaded relief | Esri World Hillshade | No | Worldwide | Pro |
| NEXRAD single site | NOAA NCEP GeoServer (WMS) | No | US | Pro |
| NEXRAD Level II volumes (3D) | [NOAA on AWS](https://registry.opendata.aws/noaa-nexrad/) (`unidata-nexrad-level2`) | No | US | Pro |
| Terrain elevation (3D) | [AWS Terrain Tiles](https://registry.opendata.aws/terrain-tiles/) | No | Worldwide | Pro |
| Storm cells, storm reports | [Iowa Environmental Mesonet](https://mesonet.agron.iastate.edu) | No | US | Pro |
| Storm cells (elsewhere) | Computed from the LibreWXR composite | No | Worldwide | Pro |
| Tropical cyclones | [GDACS](https://www.gdacs.org) | No | Worldwide | Pro |
| SPC outlook, mesoscale discussions | NOAA mapservices / api.weather.gov | No | US | Pro |
| Radar sites, METAR winds | api.weather.gov | No | US (METAR worldwide where listed) | Pro |
| Forecast soundings | Open-Meteo pressure levels | No | Worldwide | Pro |
| Wind/temperature/precipitation overlays, air quality | [OpenWeatherMap](https://openweathermap.org/api) | **Yes** | Worldwide | Pro (optional) |
| Weather radio streams | [noaaweatherradio.org](https://noaaweatherradio.org) directory | No | US | Pro |
| Updates | GitHub Releases API | No | — | Both |
| AI | The provider you configure ([AI assistant](AI-Assistant)) | Usually | — | Both (optional) |

## Being a good citizen

- Requests identify the app (`SilverSkies/2.0 (desktop app; https://github.com/poliberry/silverskies)`), as api.weather.gov asks.
- Forecasts refresh every 10 minutes by default; AccuWeather forecasts are reused for an hour, because free keys allow 50 requests a day (3 per place per refresh).
- Map tiles are cached on disk; radar frames are only downloaded for the area on screen.
- Open-Meteo rate limits shared IPs: the sounding pane backs off and retries after two minutes.
- If a refresh fails, the last good forecast is shown (from `forecast-cache.json`) with a note.

## Attribution

Credits are shown on the maps (basemap, imagery, terrain). Weather data remains the property of its providers; check each service's terms before using the apps commercially.
