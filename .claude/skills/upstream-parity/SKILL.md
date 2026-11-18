---
name: upstream-parity
description: Sync this fork with upstream ozturkkl/framework-control and port any new or changed features into the native Windows app (native/). Use when asked to "sync with upstream", "check parity", "port upstream changes", "what's new upstream", or before cutting a native release.
---

# Upstream parity sync

This repo is a fork of https://github.com/ozturkkl/framework-control. Upstream
ships a Svelte web UI (`web/`) on top of a Rust service (`service/`). This
fork keeps only the service and replaces the web UI with a native egui app
(`native/`) that talks to the same service API and must keep feature parity
with upstream's web UI.

The fork deliberately **does not keep** upstream's `web/`, Linux packaging
(`aur/`, `aur-beta/`, `install-linux.sh`, `uninstall-linux.sh`,
`LINUX_INSTALL.MD`), docs (`CHANGELOG.md`, `ANNOUNCEMENT.md`,
`PROJECT_SUMMARY.md`, `CONTRIBUTING.md`), or its `.github` workflows,
`FUNDING.yml` and PR template. Read upstream's versions from git instead, e.g.
`git show refs/parity/upstream:web/src/components/FanControl.svelte`.

`tools/parity/parity.toml` maps every upstream API operation, web UI file and
config field to its native equivalent (or records why it's omitted).
`tools/parity/parity.py` enforces that mapping; CI runs `check` on every push.

## Workflow

1. **See what changed upstream**

   ```bash
   python3 tools/parity/parity.py upstream
   ```

   Fetches upstream `main` into `refs/parity/upstream`, lists commits and
   files changed since `synced_commit`, and runs the parity check against
   upstream's tree. Gaps it prints are new endpoints, web files or config
   fields the native app doesn't cover yet.

2. **Merge upstream** on a working branch:

   ```bash
   git merge refs/parity/upstream
   ```

   Conflict rules:
   - Files the fork removed (listed above) come back as modify/delete
     conflicts or new files: keep them deleted (`git rm -r web aur aur-beta ...`
     for anything upstream re-adds). New upstream files outside `service/`
     usually belong to the web UI or Linux packaging; delete those too.
   - `service/Cargo.toml`: keep `default = []` (the `embed-ui` feature needs
     the removed `web/dist`).
   - `service/src/update.rs`: keep `release_version()` and `is_newer()`
     (the fork's MSI version comes from `native/Cargo.toml`).
   - `README.md`, `CLAUDE.md`: keep the fork's.
   - Everything else under `service/`: take upstream.

3. **Read each upstream commit** in the report (`git show <sha>`, which works
   for `web/` too even though the fork doesn't keep it), focusing on
   `web/src/**` and `service/src/routes.rs`, `service/src/types.rs`.
   For every user-visible change, port it into `native/src/`:
   - API calls / types → `native/src/api.rs` (`Backend` trait, `HttpBackend`,
     and the `DemoBackend` in `demo.rs`). Keep unknown fields in `extra` maps.
   - UI → the matching page in `native/src/pages/` (see the `native =` hints
     in parity.toml for where each web component lives).
   - Theme changes in upstream's `web/theme.config.js`
     (`git show refs/parity/upstream:web/theme.config.js`) →
     `native/src/theme.rs`.

4. **Update `tools/parity/parity.toml`** for anything added, removed or
   renamed. Use `status = "omitted"` with a `note` only for things that make
   no sense natively (browser-only behaviour, hosted-page features).

5. **Verify**

   ```bash
   python3 tools/parity/parity.py check
   (cd native && cargo test && cargo build)
   ```

   If a local service is running, also run
   `FC_TEST_PORT=<port> cargo test -- --include-ignored` in `native/`, and
   screenshot changed pages with `--demo` (see native/README.md).

6. **Record the sync and release**
   - `python3 tools/parity/parity.py mark-synced`
   - Bump `version` in `native/Cargo.toml` and add a section to
     `native/CHANGELOG.md` (mention the upstream version you synced to).
   - Merging to `main` publishes a GitHub release with the MSI; installed
     copies pick it up through Settings → Updates.

Never mark something "ported" without implementing it: `check` verifies that
ported API paths are actually called from `native/src/api.rs`.
