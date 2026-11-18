# native/: Framework Control desktop app

Rust + [egui](https://github.com/emilk/egui) front end for the Framework
Control service. See the [top-level README](../README.md) for the overview,
install and release process.

## Run

```bash
cargo run -- --demo                       # synthetic data, no service needed
cargo run -- --port 8090                  # talk to a running service
cargo run -- --demo --page fans           # open on a page: overview|fans|power|battery|settings
```

The port comes from `--port`, then `$FRAMEWORK_CONTROL_PORT`, then the
installed service's `FrameworkControlService.xml` next to the exe, then 8090.
Windows release builds have no console window and use Segoe UI.

## Test

```bash
cargo test                                          # unit tests
FC_TEST_PORT=8090 cargo test -- --include-ignored   # + round-trip against a live service
python3 ../tools/parity/parity.py check             # upstream parity
```

### Screenshots

Every UI state in the README can be reproduced in demo mode.
`FC_DEMO_SCENE` (`fan2`, `calibrate`, `logs`, `light`) opens a specific state:

```bash
FC_DEMO_SCENE=calibrate cargo run -- --demo --page fans
```

## Layout

| File | What it does |
|---|---|
| `src/main.rs` | CLI args, port discovery, window setup |
| `src/api.rs` | Service types (unknown fields kept in `extra` maps so writes never drop data) and the `Backend` trait + HTTP client |
| `src/demo.rs` | `DemoBackend` with synthetic sensors, fans and battery |
| `src/state.rs` | Background poller thread: fetches data, applies config patches, wakes the UI only on new data |
| `src/task.rs` | One-off background jobs (installs, log fetches) |
| `src/app.rs` | Window shell, navigation, feature toggles, autosaving drafts |
| `src/pages/*.rs` | Overview, Fans, Power, Battery, Tools, Settings |
| `src/launcher.rs` | Companion tools (LED Matrix Monitor, Framework 16 Lights): where to find them, running check, launching, remembered locations (`%APPDATA%\FrameworkControl\native-app.json`) |
| `src/plots.rs` | Graphs and the interactive fan-curve editor |
| `src/calibration.rs` | Fan calibration wizard (same procedure as the web UI) |
| `src/theme.rs` | Native versions of the web UI's 8 DaisyUI themes |
| `src/version.rs` | Release URLs, version comparison |
| `installer/` | WiX v3 MSI source and `build-msi.ps1` |

## How edits are saved

Edited sections (fan, power, battery) are kept as local drafts and saved 600 ms
after the last change, once the mouse is released, matching the web UI's
autosave. The poller counts applied writes, and drafts only re-sync from the
server once every write sent so far has landed, so a config poll that was in
flight can't undo an edit.
