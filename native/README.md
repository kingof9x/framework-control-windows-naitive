# Framework Control – native app

A desktop app for Framework Control that doesn't need a browser. It talks to
the same local service on `127.0.0.1` that the web UI uses, but draws its own
window with [egui](https://github.com/emilk/egui) instead of opening Chrome or
Edge.

- One ~5 MB `.exe` with no runtime and no WebView2.
- Uses a fraction of the memory: measured at 106 MB versus 520 MB (12
  processes) for Chromium's app mode showing the web UI. Of that 106 MB, about
  67 MB was this test machine's software OpenGL driver; the app's own share is
  roughly 30–40 MB.
- Redraws only when new data arrives (every telemetry interval, 2 s by
  default) or when you interact with it.

The background service is unchanged and still does all the hardware work. This
replaces only the browser front end.

## Features

| Page     | What you can do |
|----------|-----------------|
| Overview | Hottest sensor, fan RPM, battery %, battery power, active fan profile; temperature and fan-speed graphs |
| Fans     | Pick profile; Auto (EC) / Manual / Curve; drag-to-edit curve with live temperature marker; sensors, hysteresis, rate limit, interval |
| Power    | Separate plugged-in / on-battery profiles: TDP & thermal limit (Windows/RyzenAdj), EPP, governor, frequency limits (Linux) |
| Battery  | Charge, health, cycles, power; charge limit, charge-rate limit + SoC threshold; history graph |
| Settings | Telemetry interval, auto-install updates, version/update status, device info |

Not yet ported from the web UI: fan calibration wizard, per-fan overrides,
creating/renaming fan profiles and AC/battery auto-switch, RyzenAdj install
button, `framework_tool` version switcher, logs viewer, dashboard layout, themes
(the app follows the OS light/dark setting). Use the web UI for those for now;
both stay in sync because they share the service config.

## Build and run

```bash
cd native
cargo build --release
./target/release/framework-control-app            # connects to 127.0.0.1:8090
./target/release/framework-control-app --port 1234
./target/release/framework-control-app --demo     # sample data, no service needed
```

The port defaults to `$FRAMEWORK_CONTROL_PORT`, then 8090. On Windows release
builds no console window opens, and the app uses Segoe UI when it is installed.

Tests (the live one needs a running service):

```bash
cargo test
FC_TEST_PORT=8090 cargo test -- --include-ignored
```

## Layout

- `src/api.rs`: service types (unknown fields are preserved on write-back) and HTTP client
- `src/state.rs`: background poller thread; the UI thread never blocks on I/O
- `src/app.rs`: pages, widgets, plots, fan-curve editor
- `src/theme.rs`: light/dark visuals, system font
- `src/demo.rs`: synthetic backend for `--demo`
