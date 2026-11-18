# Framework Control (native Windows fork)

Fork of https://github.com/ozturkkl/framework-control that replaces the
browser UI with a native Rust/egui app.

- `service/` – upstream's Rust background service (loopback HTTP API). Keep
  changes here minimal so upstream merges stay easy.
- Upstream's browser UI (`web/`), Linux packaging and docs are deliberately
  not kept here. The parity tool reads upstream's UI straight from upstream
  git (`refs/parity/upstream` / the synced commit).
- `native/` – the native app (`cargo run -- --demo` runs it with sample data).
  Installer: `native/installer/` (WiX v3, built by `build-msi.ps1`).
- `tools/parity/` – upstream parity manifest + checker.

To sync with upstream or check feature parity, use the `upstream-parity`
skill (`.claude/skills/upstream-parity/SKILL.md`).

Releases: bump `native/Cargo.toml` and `native/CHANGELOG.md`; pushing to
`main` builds the MSI and publishes a GitHub release
(`.github/workflows/native-release.yml`).
