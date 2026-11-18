#!/usr/bin/env python3
"""Keep the native app in feature parity with upstream Framework Control.

Commands
  check [--ref REF]     Verify every upstream API operation, web UI file and
                        config field is accounted for (ported, omitted with a
                        reason, or mirrored in native/src/api.rs). By default
                        the service comes from the working tree and the web UI
                        (which this repo doesn't keep) from upstream at the
                        synced commit; --ref reads both from another git ref,
                        e.g. the fetched upstream head.
  upstream              Fetch upstream, list what changed since the last
                        synced commit, and run `check` against upstream's tree
                        so new endpoints/components/settings show up as gaps.
  mark-synced [REF]     Record REF (default: the fetched upstream head) as
                        synced in parity.toml once its changes are ported.

Exit status is 1 when there are gaps, so CI can enforce parity.
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = Path(__file__).with_name("parity.toml")
UPSTREAM_REF = "refs/parity/upstream"
NATIVE_API = ROOT / "native/src/api.rs"
WEB_DIRS = ["web/src", "web/src/components", "web/src/lib"]
WEB_EXTS = (".svelte", ".ts", ".css")


def git(*args: str, check: bool = True) -> str:
    r = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True)
    if check and r.returncode != 0:
        sys.exit(f"git {' '.join(args)} failed:\n{r.stderr.strip()}")
    return r.stdout


class Tree:
    """Read files from the working tree or from a git ref."""

    def __init__(self, ref: str | None):
        self.ref = ref

    def read(self, path: str) -> str:
        if self.ref is None:
            p = ROOT / path
            return p.read_text(encoding="utf-8") if p.exists() else ""
        return git("show", f"{self.ref}:{path}", check=False)

    def files(self, directory: str) -> list[str]:
        if self.ref is None:
            d = ROOT / directory
            return sorted(p.name for p in d.iterdir() if p.is_file()) if d.is_dir() else []
        out = git("ls-tree", "--name-only", f"{self.ref}:{directory}", check=False)
        return sorted(line for line in out.splitlines() if line and "." in line)


# ------------------------------------------------------------- scanners ---

def api_operations(tree: Tree) -> dict[str, tuple[str, str]]:
    """operation_id -> (method, path) from the service's poem-openapi routes."""
    src = tree.read("service/src/routes.rs")
    ops = {}
    for attr in re.findall(r"#\[oai\((.*?)\)\]", src, flags=re.S):
        op = re.search(r'operation_id\s*=\s*"([^"]+)"', attr)
        path = re.search(r'path\s*=\s*"([^"]+)"', attr)
        method = re.search(r'method\s*=\s*"([^"]+)"', attr)
        if op and path:
            ops[op.group(1)] = ((method.group(1) if method else "get").upper(), path.group(1))
    return ops


def web_files(tree: Tree) -> list[str]:
    names = []
    for d in WEB_DIRS:
        names += [f for f in tree.files(d) if f.endswith(WEB_EXTS)]
    return sorted(set(names))


def rust_structs(src: str) -> dict[str, list[tuple[str, str]]]:
    """struct name -> [(field, type)] for `pub struct X { pub f: T, ... }`."""
    out = {}
    for name, body in re.findall(r"pub struct (\w+)\s*\{(.*?)\n\}", src, flags=re.S):
        out[name] = re.findall(r"pub (\w+):\s*([^,\n]+)", body)
    return out


def config_fields(tree: Tree) -> list[str]:
    """`Struct.field` for every field reachable from `Config` or `PartialConfig`."""
    structs = rust_structs(tree.read("service/src/types.rs"))
    seen, queue, fields = set(), ["Config", "PartialConfig"], []
    while queue:
        name = queue.pop()
        if name in seen or name not in structs:
            continue
        seen.add(name)
        for field, ty in structs[name]:
            fields.append(f"{name}.{field}")
            queue += [t for t in re.findall(r"\b([A-Z]\w+)\b", ty) if t in structs]
    return fields


# ---------------------------------------------------------------- check ---

def ensure_commit(manifest: dict, sha: str) -> None:
    """Make an upstream commit available locally (CI checkouts are shallow)."""
    if subprocess.run(["git", "cat-file", "-e", f"{sha}^{{commit}}"], cwd=ROOT, capture_output=True).returncode == 0:
        return
    git("fetch", "--quiet", "--depth=1", manifest["upstream"]["repo"], sha)


def check(ref: str | None, quiet: bool = False) -> int:
    manifest = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
    tree = Tree(ref)
    # This repo doesn't ship the web UI, so read it from upstream at the synced commit.
    if ref is None:
        synced = manifest["upstream"]["synced_commit"]
        ensure_commit(manifest, synced)
        web_tree = Tree(synced)
    else:
        web_tree = tree
    native_api = NATIVE_API.read_text(encoding="utf-8")
    native_fields = set(re.findall(r"pub (\w+):", native_api))
    gaps: list[str] = []
    notes: list[str] = []

    # API operations
    api = manifest.get("api", {})
    ops = api_operations(tree)
    for op, (method, path) in sorted(ops.items()):
        entry = api.get(op)
        if entry is None:
            gaps.append(f"API  {op} ({method} {path}) is new upstream: port it or mark it omitted in parity.toml")
        elif entry.get("status") == "ported" and f'{path}"' not in native_api and f"{path}?" not in native_api:
            gaps.append(f"API  {op} is marked ported but {path} is not called from native/src/api.rs")
        elif entry.get("status") == "omitted" and not entry.get("note"):
            gaps.append(f"API  {op} is omitted without a note explaining why")
    for op in sorted(set(api) - set(ops)):
        notes.append(f"API  {op} no longer exists upstream: remove it from parity.toml (and the native client)")

    # Web UI files
    web = manifest.get("web", {})
    files = web_files(web_tree)
    for f in files:
        entry = web.get(f)
        if entry is None:
            gaps.append(f"WEB  {f} is new upstream: port its feature or mark it omitted/internal in parity.toml")
        elif entry.get("status") == "omitted" and not entry.get("note"):
            gaps.append(f"WEB  {f} is omitted without a note explaining why")
    for f in sorted(set(web) - set(files)):
        notes.append(f"WEB  {f} no longer exists upstream: remove it from parity.toml")

    # Config fields
    ignore = manifest.get("config_ignore", {})
    for qual in config_fields(tree):
        struct, field = qual.split(".")
        if qual in ignore or f"{struct}.*" in ignore:
            continue
        if field not in native_fields:
            gaps.append(f"CFG  {qual} is not modelled in native/src/api.rs (add it, or list it in [config_ignore])")

    where = ref or f"working tree (web UI from upstream {manifest['upstream']['synced_commit'][:12]})"
    if not quiet or gaps:
        print(f"Parity check against {where}: {len(ops)} API operations, {len(files)} web files, "
              f"{len(config_fields(tree))} config fields.")
    for n in notes:
        print(f"  note: {n}")
    if gaps:
        print(f"\n{len(gaps)} gap(s):")
        for g in gaps:
            print(f"  - {g}")
        return 1
    print("OK: everything upstream is accounted for.")
    return 0


# ------------------------------------------------------------- upstream ---

def fetch_upstream(manifest: dict) -> str:
    up = manifest["upstream"]
    git("fetch", "--quiet", up["repo"], f"+{up['branch']}:{UPSTREAM_REF}")
    return git("rev-parse", UPSTREAM_REF).strip()


def upstream() -> int:
    manifest = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
    synced = manifest["upstream"]["synced_commit"]
    head = fetch_upstream(manifest)
    print(f"Upstream {manifest['upstream']['repo']} @ {head[:12]} (synced: {synced[:12]})\n")
    if head == synced:
        print("Already in sync with upstream.")
        return check(UPSTREAM_REF, quiet=True)

    has_base = subprocess.run(["git", "cat-file", "-e", f"{synced}^{{commit}}"], cwd=ROOT).returncode == 0
    rng = f"{synced}..{head}" if has_base else head
    print("## Commits to review\n")
    print(git("log", "--oneline", "--no-merges", rng, "--", "web", "service") or "(none touching web/ or service/)")
    print("## Files changed\n")
    print(git("diff", "--stat", f"{synced}", head, "--", "web/src", "service/src") if has_base else "(synced commit not found locally)")
    if has_base:
        version_old = re.search(r'^version\s*=\s*"([^"]+)"', git("show", f"{synced}:service/Cargo.toml"), re.M)
        version_new = re.search(r'^version\s*=\s*"([^"]+)"', git("show", f"{head}:service/Cargo.toml"), re.M)
        if version_old and version_new and version_old.group(1) != version_new.group(1):
            print(f"Service version: {version_old.group(1)} -> {version_new.group(1)}\n")
    print("## Parity against upstream head\n")
    rc = check(UPSTREAM_REF)
    print(
        "\nNext: merge upstream (`git merge " + UPSTREAM_REF + "`), port the changes listed above into native/, "
        "update parity.toml, run `parity.py check`, then `parity.py mark-synced`."
    )
    return rc


def mark_synced(ref: str | None) -> int:
    manifest = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
    ref = ref or UPSTREAM_REF
    sha = git("rev-parse", ref).strip()
    m = re.search(r'^version\s*=\s*"([^"]+)"', git("show", f"{sha}:service/Cargo.toml"), re.M)
    text = MANIFEST.read_text(encoding="utf-8")
    text = re.sub(r'^synced_commit = ".*"$', f'synced_commit = "{sha}"', text, flags=re.M)
    if m:
        text = re.sub(r'^synced_version = ".*"$', f'synced_version = "{m.group(1)}"', text, flags=re.M)
    MANIFEST.write_text(text, encoding="utf-8")
    print(f"Marked {sha[:12]} ({m.group(1) if m else '?'}) as synced in {MANIFEST.relative_to(ROOT)}.")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("check")
    c.add_argument("--ref")
    sub.add_parser("upstream")
    m = sub.add_parser("mark-synced")
    m.add_argument("ref", nargs="?")
    args = ap.parse_args()
    if args.cmd == "check":
        return check(args.ref)
    if args.cmd == "upstream":
        return upstream()
    return mark_synced(args.ref)


if __name__ == "__main__":
    sys.exit(main())
