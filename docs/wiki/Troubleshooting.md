# Troubleshooting

**The radar is blank or stuck on "loading frames…"**
The frame index comes from your LibreWXR host (Settings → Data sources). Check it's reachable; the audit log (Pro) shows errors. Frames update about every 10 minutes.

**Forecasts say "Couldn't reach the forecast service"**
The last good forecast is shown meanwhile. Check your connection; with AccuWeather, check the key and the daily request limit (free keys: 50/day).

**Sounding failed: HTTP 429 (rate limited)**
Open-Meteo limits busy shared IP addresses (VPNs, offices). The pane retries after two minutes.

**The 3D view says "global radar mosaic … heights estimated"**
There's no US NEXRAD within 230 km, so measured volumes aren't available; the echoes are placed by intensity. Terrain and imagery are still real. See [3D radar and terrain](3D-Radar-and-Terrain).

**The 3D view is slow**
Use a release build; raise the **≥ dBZ** threshold; turn off satellite imagery or storms; make the pane smaller.

**No alerts where I expected some**
Check the alert type isn't hidden (Pro: Settings → Alerts). Australian warnings need a WillyWeather key.

**Weather radio or spoken alerts are silent**
They need an audio device and, on Linux, a speech engine: install `espeak-ng` (`sudo apt install espeak-ng`, `sudo dnf install espeak-ng`, `sudo pacman -S espeak-ng`). Pro uses it directly, falling back to speech-dispatcher; set `SS_TTS=speechd` to prefer speech-dispatcher. At startup the audit log says which engine it uses ("text-to-speech: espeak-ng"), or "text-to-speech unavailable" and why.

**Updates: "This copy can't update itself"**
You're running a portable zip or a build from source; download from the release page. A `.deb` install opens the package in your software installer instead.

**Reset everything**
Quit, then delete the app's folder ([Settings and files](Settings-and-Files)).

**Still stuck?**
[Open an issue](https://github.com/poliberry/silverskies/issues) with your OS, the app version (About) and what the audit log says.
