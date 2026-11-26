#!/usr/bin/env python3
"""Documentation hygiene gate for release.

Checks the two things a release must guarantee about `docs/`:

  refinement  - completed plans and investigation reports do not live in the
                tree (git history still has them), and every document under
                `docs/` is reachable from an index instead of being an orphan.
  alignment   - every internal Markdown link resolves to a file that exists.

Exit code 0 means the tree passes; 1 means it does not, and the reasons are
printed. No network, no build, no dependency outside the standard library.

Usage:
    python3 scripts/check-docs.py            # from the repository root
    python3 scripts/check-docs.py --verbose  # also print the size table

The thresholds are not arbitrary: `docs/README.md` states that this tree keeps
only documents that describe the current product, and a link that points at a
deleted report is exactly the kind of rot that rule exists to prevent.
"""

from __future__ import annotations

import argparse
import os
import posixpath
import re
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Directories that never hold product documentation. `.tmp-sync` is a
# gitignored scratch directory from an older sync tool; it is not part of the
# tree a release ships, so its links are none of this gate's business.
SKIP_DIRS = {".git", "node_modules", "target", "dist", "binaries", ".workbuddy", ".tmp-sync"}

# Completed plans and evidence reports belong to git history, not the tree.
FORBIDDEN_DIRS = ("docs/plans", "docs/reports")

# Documents that are entry points themselves and need no inbound link.
ENTRY_DOCS = {
    "README.md",
    "AGENTS.md",
    "CLAUDE.md",
    "CHANGELOG.md",
    "docs/README.md",
}

MD_LINK = re.compile(r"\[[^\]]*\]\(\s*<?([^)>\s]+)>?\s*\)")
REF_LINK = re.compile(r"^\s*\[[^\]]+\]:\s*<?([^>\s]+)>?", re.M)

# Link syntax inside a code span or a fenced block is an example, not a link.
# A guide that documents "write `[text](target.md)`" must not fail its own gate.
FENCED_BLOCK = re.compile(r"```.*?```", re.S)
INLINE_CODE = re.compile(r"`[^`\n]*`")


def strip_code(body: str) -> str:
    return INLINE_CODE.sub("", FENCED_BLOCK.sub("", body))


def collect_markdown(root: str) -> list[str]:
    found = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for name in filenames:
            if name.endswith(".md"):
                found.append(
                    os.path.relpath(os.path.join(dirpath, name), root).replace(os.sep, "/")
                )
    return sorted(found)


def internal_targets(body: str) -> list[str]:
    text = strip_code(body)
    targets = []
    for raw in MD_LINK.findall(text) + REF_LINK.findall(text):
        if "://" in raw or raw.startswith("mailto:") or raw.startswith("#"):
            continue
        path = raw.split("#")[0].split("?")[0].strip()
        if path.endswith(".md"):
            targets.append(path)
    return targets


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verbose", action="store_true", help="print the size table")
    args = parser.parse_args()

    os.chdir(REPO_ROOT)
    files = collect_markdown(".")
    fileset = set(files)
    bodies = {}
    for rel in files:
        with open(rel, encoding="utf-8", errors="replace") as handle:
            bodies[rel] = handle.read()

    failures: list[str] = []

    # 1. Alignment: every internal .md link must resolve.
    broken = []
    link_count = 0
    inbound: dict[str, int] = {rel: 0 for rel in files}
    for rel, body in bodies.items():
        for raw in internal_targets(body):
            link_count += 1
            resolved = (
                raw.lstrip("/")
                if raw.startswith("/")
                else posixpath.normpath(posixpath.join(posixpath.dirname(rel), raw))
            )
            if resolved in fileset:
                if resolved != rel:
                    inbound[resolved] += 1
            else:
                broken.append((rel, raw, resolved))
    if broken:
        failures.append(f"{len(set(broken))} broken internal link(s)")
        for src, raw, resolved in sorted(set(broken)):
            print(f"  broken  {src} -> {raw}  (resolved {resolved})")

    # 2. Refinement: completed plans and evidence reports stay out of the tree.
    for directory in FORBIDDEN_DIRS:
        if os.path.isdir(directory):
            names = [n for n in os.listdir(directory) if n.endswith(".md")]
            if names:
                failures.append(f"{directory}/ still holds {len(names)} Markdown file(s)")

    # 3. Refinement: no orphan document under docs/.
    orphans = [
        rel
        for rel in files
        if rel.startswith("docs/")
        and rel not in ENTRY_DOCS
        and inbound.get(rel, 0) == 0
    ]
    if orphans:
        failures.append(f"{len(orphans)} document(s) under docs/ are not linked from anywhere")
        for rel in orphans:
            print(f"  orphan  {rel}")

    # 4. Report; sizes are informational, not a gate.
    total_bytes = sum(os.path.getsize(rel) for rel in files)
    print(
        f"markdown files: {len(files)}  total: {total_bytes / 1024:.1f} KiB  "
        f"internal links: {link_count}"
    )
    if args.verbose:
        for rel in sorted(files, key=lambda p: -os.path.getsize(p)):
            size = os.path.getsize(rel) / 1024
            print(f"  {size:8.1f} KiB  {rel}")

    if failures:
        print("documentation gate: FAIL")
        for reason in failures:
            print(f"  - {reason}")
        return 1
    print("documentation gate: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
