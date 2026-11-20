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

Releases: add the new version's notes to `native/CHANGELOG.md`; Callum then
publishes from Actions > Build and release > Run workflow, which writes the
version into `native/Cargo.toml` for the build and attaches the MSI
(`.github/workflows/native-release.yml`). Pushing to `main` no longer
publishes a release.

<!-- github-workflow:start (same in all kingof9x repos; edit the template, not here) -->
## GitHub workflow (follow this every time)

Callum tests and merges. You write the code and open the pull request (PR).
He is not a git expert, so keep his side to: open the PR, test the build, merge.

1. **Branch.** Work on a branch named `claude/<short-topic>`. Never commit or push to `main`,
   never force-push, never merge a PR, never delete branches or tags.
2. **One PR per task, opened early.** Push the branch and open the PR straight away, before the
   build finishes. Fill in the PR template (What changed / How to test / What you should see /
   Notes and risks) in plain language. If `gh pr create` fails, use the REST API
   (`gh api repos/kingof9x/framework-control-windows-naitive/pulls -X POST ...`). If you can't open it, give this exact link:
   `https://github.com/kingof9x/framework-control-windows-naitive/compare/main...BRANCH?expand=1`
3. **Keep the branch up to date by merging.** If `main` moved on, merge `main` into your branch.
   Don't rebase or force-push a branch that is already pushed.
4. **Wait for the test build.** Every PR is built automatically. The top of the PR description
   shows ⏳ Building, then ✅ with direct download links (also posted as a 🧪 Test builds comment).
   Don't tell Callum to test until it shows ✅. If the build fails, fix it before handing off.
5. **Hand off the same way every time.** End with:
   - the PR link
   - the Actions link for the branch: `https://github.com/kingof9x/framework-control-windows-naitive/actions?query=branch%3ABRANCH`
   - the direct download link(s) from the PR (or say it's still building, with the Actions link)
   - 2–5 numbered test steps, ending with what he should see if it works
6. **Releases are Callum's.** He publishes them from Actions → Build and release → Run workflow
   (version box: `next` = last release + 1, a number, or empty for a test build only).
   Don't create releases or tags yourself, and don't bump version numbers for a release.
7. **Shared files.** The Build and release workflow, `.github/workflows/pr-test-builds.yml` and
   `.github/pull_request_template.md` are the same across all his repos. Don't change them unless
   he asks; say so if a task seems to need it.

If anything here can't be done (no push access, the PR can't be opened, the build is red),
say so plainly at once rather than at the end.
<!-- github-workflow:end -->
