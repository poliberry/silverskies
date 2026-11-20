# Silver Skies Pro command reference

Press `:` to open the command line. Commands are case-insensitive; Tab completes command names, panes, layers and arguments; ↑/↓ recalls history. Every function key, menu item and many buttons simply run one of these commands, and each is echoed in the audit log.

## Commands

| Command | What it does | Aliases |
|---|---|---|
| `:goto <place> \| goto <lat> <lon>` | Search for a place or jump to coordinates | `:g`, `:go` |
| `:add [label]` | Save the current location | `:a` |
| `:remove <name\|number>` | Remove a saved location | `:rm`, `:del`, `:delete` |
| `:select <name\|number>` | Switch to a saved location | `:sel` |
| `:refresh` | Refresh weather and alerts now | `:r` |
| `:units [f\|c]` | Show or set temperature units | `:u` |
| `:time [12\|24]` | Show or set the clock format |  |
| `:theme [id]` | Show or set the theme |  |
| `:move <panel> <left\|center\|right> [position]` | Move a panel to a column | `:mv` |
| `:swap <panel> <panel>` | Swap two docked panels |  |
| `:hide <panel>` | Hide a panel |  |
| `:show <panel>` | Show a hidden panel |  |
| `:popout <panel>` | Open a panel in its own window | `:pop` |
| `:reset-layout` | Restore the default panel layout | `:reset`, `:resetlayout` |
| `:radar new\|popout\|recenter\|live\|scheme <n>\|opacity <0-100>\|nowcast\|satellite\|smooth\|snow\|arrows\|cells\|legend [on\|off]` | Radar window and map options | `:rad` |
| `:alerts [list\|show <n>\|clear\|filter [text]]` | Inspect and filter alerts | `:al` |
| `:speak [text]` | Read text (or the top alert) aloud | `:tts`, `:say` |
| `:mute` | Stop speech and weather radio audio |  |
| `:radio [toggle\|live [url]\|simulated\|stop\|volume <0-100>\|retry\|test]` | Control the weather radio |  |
| `:set <key> <value>` | Change a setting |  |
| `:open <window>` | Open a window (radar, conditions, audit-log, radio, settings) |  |
| `:settings` | Open the settings window | `:prefs`, `:preferences` |
| `:close` | Close the current window |  |
| `:assistant` | Open the AI weather assistant chat | `:chat`, `:ai` |
| `:help [command]` | Show help | `:h`, `:?` |
| `:quit` | Quit Silver Skies | `:q`, `:exit` |
| `:demo [group\|event]` | Fire demo alerts |  |
| `:zoom <level>` | Set the map zoom level | `:z` |
| `:layer <alerts\|satellite\|nowcast\|spc\|md\|barbs\|owm-wind\|owm-temp\|owm-precip\|aqi\|sites\|legend\|nexrad\|arrows\|cells> [on\|off]` | Toggle a map layer | `:l` |
| `:product <name>` | Select the radar product | `:p` |
| `:level2 <ref\|vel\|srv\|cc\|zdr\|off>` | Show a Level II product (every tilt) on the single-site radar | `:l2` |
| `:tilt <n\|up\|down>` | Choose the Level II tilt (1 = lowest) | |
| `:site [id]` | Select or clear the radar site | `:s` |
| `:play` | Play the radar loop |  |
| `:pause` | Pause the radar loop |  |
| `:frame <+n\|-n>` | Step the radar loop | `:f` |
| `:fullscreen` | Toggle fullscreen | `:fs` |

## Panes

Accepted wherever a `<panel>` is expected (`:show`, `:hide`, `:move`, `:swap`, `:popout`):
`locations` (`loc`), `radar`, `audit-log` (`log`, `alerts`), `conditions` (`cond`, `current`), `forecast` (`7day`, `daily`, `week`), `hourly` (`hours`), `radio` (`wx`), `storms` (`cells`, `lsr`, `severe`), `sounding` (`skewt`, `hodo`, `raob`), `radar-3d` (`3d`, `volume`, `terrain`, `topo`).

## Map layers

`:layer <name> [on|off]` (no on/off toggles): `alerts`, `satellite`, `nowcast`, `spc`, `md`, `barbs`, `owm-wind`, `owm-temp`, `owm-precip`, `aqi`, `sites`, `legend`, `nexrad`, `arrows`, `cells`, `terrain`, `storms`, `reports`, `tropical`.

`tropical` also accepts `cyclones`, `hurricanes`, `typhoons`, `tc`.

## Settings (`:set <key> <value>`)

| Key | Values |
|---|---|
| `autoRefreshMinutes` | 0–1440 (0 = off) |
| `librewxrHost` | an http(s) URL |
| `notifications` | on / off |
| `spcOutlook` | on / off |
| `topGlow` | on / off |
| `devTools` | on / off |
| `stormPolygons` | on / off |
| `ttsAlerts` | on / off |
| `ttsFilter` | `extreme`, `severe`, `moderate`, `all` |
| `ttsRate` | 0.5–2.0 |
| `radarOpacity` | 0–100 |
| `radarScheme` | a colour-scheme number |

Units, clock and theme have their own commands (`:units`, `:time`, `:theme`). Everything else is in the Settings window.

## Examples

```
:goto Oklahoma City          search and show a place
:goto 35.22 -97.44           jump to coordinates
:add Home                    save it as "Home"
:layer storms on             show NEXRAD storm cells
:site KTLX                   single-site NEXRAD from Twin Lakes
:product velocity            switch the single-site product
:popout 3d                   open the 3D radar in its own window
:move sounding right 1       put the sounding pane at the top of the right column
:set ttsFilter severe        speak severe and extreme alerts only
```
