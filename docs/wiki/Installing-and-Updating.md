# Installing and updating

Every push to `main` publishes a release on the [Releases page](https://github.com/poliberry/silverskies/releases) with both apps for every platform. Releases are tagged `v<version>.<build>`, e.g. `v2.0.2.3` is version 2.0.2, build 3.

## Downloads

| Platform | Silver Skies | Silver Skies Pro | Notes |
|---|---|---|---|
| Windows (x64) | `SilverSkies-Setup-<v>.exe` | `SilverSkiesPro-Setup-<v>.exe` | Installer; per-user, no admin rights needed. Also a portable `.zip`. |
| macOS, Apple silicon | `SilverSkies-<v>-macos-arm64.dmg` | `SilverSkiesPro-<v>-macos-arm64.dmg` | Drag to Applications. A `.zip` of the app is also published. |
| macOS, Intel | `…-macos-x86_64.dmg` | `…-macos-x86_64.dmg` | |
| Linux (x86-64) | `SilverSkies-<v>-x86_64.AppImage` | `SilverSkiesPro-<v>-x86_64.AppImage` | Make it executable and run it. |
| Debian / Ubuntu | `silverskies_<ver>-<build>_amd64.deb` | `silverskies-pro_<ver>-<build>_amd64.deb` | `sudo apt install ./file.deb` |

The apps are not code-signed. On macOS, the first time, right-click the app and choose **Open**. On Windows, SmartScreen may ask you to confirm (**More info → Run anyway**).

## First run

Neither app needs an account or API key. On first launch:

1. Search for a place (Silver Skies: the search bar; Pro: type it into the amber field, or `:goto <place>`), or use **Use my location**.
2. Save it (Silver Skies: places you pick from search are added to the sidebar; Pro: **F1** or `:add`).
3. Forecasts refresh every 10 minutes by default; radar frames every few minutes.

Silver Skies Pro, on its first run, copies the places and settings of Silver Skies if you already use it.

## Updates

Both apps update themselves from GitHub Releases.

- **When:** at startup (after a few seconds) and once a day while running. Turn this off in Settings → Updates (Silver Skies) or Settings → About (Pro).
- **What you see:** Silver Skies shows an *Update available* card in the bottom-right corner; Pro shows an amber **UPDATE x.y.z.b** chip in the header and a line in the audit log. Either leads to **Update and restart**, **Release notes** or **Skip this version**.
- **Safety:** the download is checked against the SHA-256 checksum GitHub publishes for each file before anything is changed. A corrupt download is deleted and nothing is installed.

How the update is applied depends on how the app was installed:

| Installed from | What the update does |
|---|---|
| Windows installer | Runs the new installer silently; it closes the app, installs over it and starts the new version. |
| macOS `.dmg` / `.zip` (an app in Applications) | Unpacks the new app beside the old one, swaps them, and restarts. |
| Linux AppImage | Replaces the AppImage file and restarts it. |
| Linux `.deb` | Downloads the new package and opens it in your software installer (installing needs your password). |
| Windows portable zip, or built from source | Opens the release page so you can download it yourself. |

A development build (compiled locally) is only offered a newer *version*, never a newer build of the same version.
