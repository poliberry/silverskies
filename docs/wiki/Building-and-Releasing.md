# Building, testing and releasing

## Build

You need [Rust](https://rustup.rs) (stable). On Linux, also the usual GUI and audio development packages:

```bash
sudo apt-get install -y libxkbcommon-dev libwayland-dev libxcb-render0-dev \
  libxcb-shape0-dev libxcb-xfixes0-dev libgtk-3-dev libasound2-dev \
  libspeechd-dev libssl-dev pkg-config
```

```bash
cargo run --release -p silverskies        # Silver Skies
cargo run --release -p silverskies-pro    # Silver Skies Pro
```

Debug builds work but are slow in the radar and 3D views; dependencies are always optimised (`[profile.dev.package."*"] opt-level = 2`).

## Check

CI runs these on every push; run them before a pull request:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Useful extras:

```bash
cargo run --release -p ss-net --example smoke     # hit the live APIs once each
cargo run --release -p ss-net --example scene3d   # build 3D scenes for a few places
cargo run -p render-icons                         # regenerate app icons from the SVGs
```

## Releases

Every push to `main` runs the **Release** workflow:

1. **version:** the build number is one more than the highest existing `v<version>.<n>` tag (so each new version starts at build 1).
2. **build:** both apps × Windows (installer via Inno Setup, portable zip), macOS arm64 and x86_64 (`.app` in `.dmg` and `.zip`), Linux (AppImage via linuxdeploy, `.deb`). `SS_BUILD` is compiled in, so About shows "2.0.2 (build 3)".
3. **release:** a GitHub Release tagged `v<version>.<build>` with every file and the commits since the previous release as notes.

Bump `version` in the workspace `Cargo.toml` to start a new version. Running the workflow by hand builds everything as workflow artifacts without publishing.

The apps' [updater](Installing-and-Updating#updates) reads these releases: it relies on the asset names above and the SHA-256 digests GitHub records for each file.

## The wiki

These pages live in `docs/wiki/` and are published by the **Wiki** workflow on pushes to `main` (see `docs/wiki/README.md`). Edit them there.
