# Changelog (native fork)

Versions here are this fork's release line. The upstream service version the
fork is synced to is recorded in `tools/parity/parity.toml`.

## 1.0.0

- Native Windows app (Rust + egui) replaces the browser UI: about 5× less
  memory than Chromium app mode, no browser required.
- Full feature parity with the upstream web UI: sensors, fan profiles with
  AC/battery auto-switch, Auto/Manual/Curve modes, per-fan overrides,
  calibration with live duty markers, power profiles (RyzenAdj install on
  Windows), battery limits and history, framework_tool version switching,
  logs, themes and feature toggles.
- Single MSI installs the service and the app, with Start Menu and Desktop
  shortcuts for the app. Replaces an existing upstream install and keeps
  settings.
- In-app updates from this repository's GitHub releases (silent, no UAC).
