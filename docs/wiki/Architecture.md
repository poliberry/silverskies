# Architecture

Silver Skies is a Rust workspace: two apps on [egui/eframe](https://github.com/emilk/egui) (immediate-mode UI, OpenGL via glow), sharing a set of library crates.

```
apps/
  silverskies/   the simple app (Material 3 Expressive)
  pro/           the workstation app
crates/
  ss-core/       pure domain logic: types, alerts, units, WMO codes, themes, command grammar,
                 SAME codec, geometry, storms, cell tracking, soundings, 3D radar geometry
  ss-net/        API clients (one module per service), Level II decoder, 3D scene builder
  ss-store/      per-app JSON files with atomic writes
  ss-audio/      tone synthesis, native text-to-speech, live stream playback
  ss-map/        slippy-map engine: viewport, async tile cache, layers
  ss-assets/     fonts, weather icons, Material Symbols, SVG rasterising, menu bar
  ss-ai/         AI providers, prompts, the assistant chat window
  ss-update/     updates from GitHub Releases
tools/
  render-icons/  regenerates app icons from the brand SVGs
```

## Data flow

```
 UI thread (egui, ~60 fps when animating)          Tokio runtime (worker threads)
 ─────────────────────────────────────────         ──────────────────────────────
  App::ui()                                         Tasks::refresh_location()
   ├─ drain_events() ◄──── crossbeam channel ◄────── Net::weather(), alerts…
   ├─ periodic(): timers, updates                    Net::scene3d(), soundings…
   └─ panes read Shared (RwLock) and draw            post(AppEvent) + request_repaint()
```

- **ss-core** has no I/O: parsing, classification, maths. Almost all of it is unit-tested.
- **ss-net** wraps one shared `reqwest` client (rustls, gzip). Each service is a module with pure `parse_*` functions (tested with fixtures) and an `impl Net` async method.
- The apps never block the UI thread on the network: work is spawned on Tokio and results come back as events through a channel, followed by a repaint request.
- **Pro** keeps all state in `Shared` (behind a `parking_lot::RwLock`) so every window — the main one and any child viewports (radar windows, popped-out panes, settings, alert windows) — draws from the same data. Commands from any source (keys, menus, the command line, buttons) go through one `commands::execute`.
- **Silver Skies** keeps its state on the `App` struct and talks to a `Worker`.

## The map engine (ss-map)

- `Viewport`: Web Mercator maths, visible tiles, smooth zoom animation.
- `TileCache`: deduplicated async downloads with a disk cache; decoding on blocking threads; textures uploaded on the UI thread; LRU eviction at 512 textures. Two download pools — **visible** (10) and **background** (4) — and queued requests nobody has asked for in 30 frames are dropped.
- Layers: raster tiles, WMS images, polygons (with triangulation and path cleaning), markers, wind barbs, storm symbols, and `RadarLoopLayer` (all frames, crossfades, hold-last-good).

## Rendering notes

- Pro's 3D view renders with OpenGL inside egui (a paint callback; the window has a depth buffer): level-of-detail terrain meshes, depth-tested and shaded by elevation, and the radar ray-marched as a volume in a fragment shader (NEXRAD tilts as a 2D texture array sampled in polar coordinates, the mosaic as a 3D texture). Without OpenGL 3 it falls back to CPU-projected, depth-sorted meshes and soft sprites.
- Text uses baseline-aligned helpers (`ss_assets::text`) so mixed fonts line up.
- Pro uses Saira (OFL; digits remapped to tabular), DejaVu for symbols and a Material Symbols subset for icons; Silver Skies uses Google Sans.

## Testing

`cargo test --workspace` runs about 450 tests: parsers against recorded API responses, alert classification and merging, the command grammar, layout maths, cell detection and tracking, beam geometry, update version comparison and asset selection, and UI helpers. Network-dependent checks use local mock servers (`SS_OPENMETEO_BASE`, `SS_UPDATE_URL`) or opt-in samples (`SS_LEVEL2_SAMPLE`).
