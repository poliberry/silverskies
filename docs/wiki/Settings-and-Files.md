# Settings and files

Each app keeps its own files in a per-user folder:

| OS | Silver Skies | Silver Skies Pro |
|---|---|---|
| Linux | `~/.config/silverskies/` | `~/.config/silverskiespro/` |
| macOS | `~/Library/Application Support/com.silverskies.silverskies/` | `~/Library/Application Support/com.silverskies.silverskiespro/` |
| Windows | `%APPDATA%\silverskies\silverskies\config\` | `%APPDATA%\silverskies\silverskiespro\config\` |

Pro shows its folder under Settings → About (with **Copy path**); Silver Skies shows it at the bottom of Settings.

| File | Contents |
|---|---|
| `config.json` | All settings: units, clock, theme, forecast source and API keys, radar options, alert overrides, radio and speech, AI, update preferences |
| `locations.json` | Saved places and which one is active |
| `session.json` | Pro: pane layout, open windows and their positions |
| `simple.json` | Silver Skies: its own preferences (reduce motion, automatic AI summaries …) |
| `forecast-cache.json` | The last good forecast for each place (shown if a refresh fails) |
| `dedupe.json` | Alerts already notified about |
| `tiles/` | Map tile cache (safe to delete) |

Files are written atomically (write to a temporary file, then rename), so a crash never leaves a half-written config. Unknown or missing keys fall back to defaults, so older files keep loading.

**API keys** are stored in `config.json` in plain text, readable only by your user account; they are sent only to their own service. Pro's About page shows the config with secrets masked.

## Moving or resetting

- To move to another computer, copy the folder.
- To reset an app, quit it and delete its folder.
- Pro, on its very first run, copies `config.json`, `locations.json` and `session.json` from Silver Skies' folder if it exists (never overwriting).

## config.json reference

Keys are camelCase. The most useful:

| Key | Default | Meaning |
|---|---|---|
| `provider` | `"open-meteo"` | Forecast source (`open-meteo` or `accu-weather`) |
| `accuWeatherApiKey`, `willyWeatherApiKey`, `openWeatherMapApiKey` | `null` | Optional keys |
| `libreWxrHost` | `"https://api.librewxr.net"` | Radar/alert server |
| `units` | `"F"` | `F` or `C` |
| `timeFormat` | `"12"` | `"12"` or `"24"` |
| `theme` / `themeId` | `"system"` / `"default"` | Light/dark preference; named theme or `custom` |
| `autoRefreshMinutes` | `10` | 0 = off |
| `notificationsEnabled` | `true` | Desktop notifications (Pro) |
| `radar.*` | | Radar layers and options (scheme, opacity, terrain, stormCells, stormReports, tropical, nexradSite, …) |
| `alertTypeOverrides` | `{}` | Per alert class colour/visibility |
| `ttsAlertsEnabled`, `ttsSeverityFilter`, `ttsRate` | off, severe, 0.95 | Spoken alerts |
| `weatherRadioEnabled`, `weatherRadioMode` | off, simulated | Weather radio |
| `ai` | disabled | AI provider settings |
| `checkForUpdates` | `true` | Automatic update checks |
| `skippedUpdate` | `null` | A release tag you chose to skip |
