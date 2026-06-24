#!/usr/bin/env python3
"""Version-ownership lint — mechanical enforcement of crap-rs#448's
release-process contract: **release-plz owns every `[package] version`
bump.** A non-release PR must not hand-edit a crate's published version.

The *why* lives in AGENTS.md ("Version ownership") and the ADR
(`adr-release-process-and-public-api-boundary`). This script is the
*what*: it replaces the documented rule with a CI + lefthook gate so a
future contributor (human or agent) cannot silently bump a crate version
inside a feature PR — the drift that stranded crap-core 0.6.0/0.7.0 as
phantom versions (changelogged on `main`, never tagged/published) and
produced the confusing 0.5→0.8→0.9 jump cutting release #373.

Same mechanical-enforcement pattern as `scripts/bdd-tracked-lint.py` and
`scripts/mutants-skip-lint.py` (documentation rots; CI doesn't). Single
source of truth in this script; `.github/workflows/ci.yml` and
`lefthook.yml` both invoke it. The CI job and the lefthook hook both run
ONLY on non-release branches (the `release-plz-*` head ref is excluded at
the job/hook level), so this script never sees release-plz's own bumps.

Algorithm (value-comparison, NOT diff-line parsing):

  1. Enumerate the Cargo.toml files that changed between the base commit
     and HEAD (`git diff --name-only BASE HEAD`).
  2. For each, read the BEFORE blob (`git show BASE:<path>`) and the
     AFTER blob (`git show HEAD:<path>`), and extract two values from
     each via a TOML parse: `[package].version` and
     `[workspace.package].version`.
  3. Flag a violation when a value that EXISTED in BEFORE changed in
     AFTER. A brand-new crate (no BEFORE `[package]`) is exempt — its
     initial version is not a "bump". A deleted file is exempt.

Why value-comparison and not a diff regex: a Cargo.toml's
`[dependencies]` / `[dev-dependencies]` / `[build-dependencies]` tables
are full of `version = "..."` lines (and inline `foo = { version =
"..." }`), which feature PRs legitimately change constantly (adding a
dep, a Dependabot bump). A regex over diff lines cannot reliably tell a
dependency version from the package version without reconstructing TOML
section context. Parsing the actual `[package].version` value sidesteps
that entire false-positive class — only the real published-version field
is compared.

A `--self-test` flag exercises the core comparison against in-memory
(before, after) TOML pairs — bump fails, no-change passes, new-crate
passes, dependency-only change passes, workspace-version bump fails — so
the lint logic itself is regression-guarded without needing a git repo.

Limitations:

* **Escape hatch is the release-plz path, by design.** There is no
  inline override marker: every legitimate `[package] version` bump goes
  through release-plz's release PR (which this gate never runs on). A
  brand-new crate is the only feature-PR case that touches a version,
  and it is exempt (no prior value). If a one-off manual bump is ever
  genuinely required, do it on a `release-plz-*` branch or split it into
  release-plz's PR — do not weaken this gate.
* **TOML parse errors are skipped, not failed.** If a Cargo.toml is
  invalid TOML in BEFORE or AFTER, the version can't be compared and the
  file is skipped with a stderr note. Invalid TOML is caught by the
  build/fmt gates; this lint does not double-report it (and must not
  false-fail the whole gate on an unrelated parse issue).
* **Requires git history.** The CI job checks out with `fetch-depth: 0`
  and passes the PR base SHA; lefthook diffs against `origin/main`. With
  no base resolvable, the script exits 0 with a stderr note rather than
  blocking (fail-open on missing history — a release gate must not wedge
  on a shallow checkout).
"""

from __future__ import annotations

import os
import subprocess
import sys
import tomllib
from typing import Optional


def _versions_from_toml(text: str) -> dict[str, Optional[str]]:
    """Return {'package': <[package].version or None>,
    'workspace': <[workspace.package].version or None>} for a Cargo.toml.

    A parse failure raises tomllib.TOMLDecodeError (handled by callers).
    """
    data = tomllib.loads(text)
    pkg = data.get("package", {})
    ws = data.get("workspace", {}).get("package", {})
    return {
        "package": pkg.get("version") if isinstance(pkg, dict) else None,
        "workspace": ws.get("version") if isinstance(ws, dict) else None,
    }


def find_violation(before: Optional[str], after: Optional[str], path: str) -> Optional[str]:
    """Compare BEFORE/AFTER Cargo.toml blobs; return a violation message
    or None.

    `before`/`after` are the file contents (str), or None when the file
    did not exist on that side (new crate / deleted file).
    """
    if before is None or after is None:
        # New crate (no BEFORE) or deletion (no AFTER) — not a bump.
        return None
    try:
        b = _versions_from_toml(before)
        a = _versions_from_toml(after)
    except tomllib.TOMLDecodeError as exc:
        print(f"  note: skipping {path} — TOML parse error ({exc})", file=sys.stderr)
        return None
    for key, label in (("package", "[package] version"), ("workspace", "[workspace.package] version")):
        bv, av = b[key], a[key]
        # Only flag a value that EXISTED before and changed. A newly
        # introduced value (bv is None) is a new crate / new workspace
        # table, not a bump.
        if bv is not None and av != bv:
            return f"{path}: {label} changed {bv!r} -> {av!r} in a non-release PR"
    return None


# --------------------------------------------------------------------------
# git plumbing (CLI path; not exercised by --self-test)
# --------------------------------------------------------------------------

def _git(args: list[str]) -> Optional[str]:
    """Run a git command; return stdout, or None on non-zero exit."""
    try:
        out = subprocess.run(
            ["git", *args],
            capture_output=True,
            text=True,
            check=True,
        )
        return out.stdout
    except subprocess.CalledProcessError:
        return None


def _resolve_base() -> Optional[str]:
    """Resolve the base commit to diff against.

    Precedence: $BASE_SHA (CI passes pull_request.base.sha) ->
    $BASE_REF -> origin/main -> main. Returns a rev string git can
    resolve, or None if nothing resolves (fail-open).
    """
    candidates = []
    if os.environ.get("BASE_SHA"):
        candidates.append(os.environ["BASE_SHA"])
    if os.environ.get("BASE_REF"):
        candidates.append(os.environ["BASE_REF"])
        candidates.append(f"origin/{os.environ['BASE_REF']}")
    candidates += ["origin/main", "main"]
    for rev in candidates:
        if _git(["rev-parse", "--verify", "--quiet", f"{rev}^{{commit}}"]) is not None:
            return rev
    return None


def _show(rev: str, path: str) -> Optional[str]:
    """`git show rev:path`; None if the path doesn't exist at that rev."""
    return _git(["show", f"{rev}:{path}"])


def run_cli() -> int:
    base = _resolve_base()
    if base is None:
        print(
            "version-edit-lint: could not resolve a base commit "
            "(BASE_SHA/BASE_REF/origin/main all unresolvable) — skipping (fail-open).",
            file=sys.stderr,
        )
        return 0

    name_only = _git(["diff", "--name-only", base, "HEAD"])
    if name_only is None:
        print(
            f"version-edit-lint: `git diff {base} HEAD` failed — skipping (fail-open).",
            file=sys.stderr,
        )
        return 0

    cargo_tomls = [
        line.strip()
        for line in name_only.splitlines()
        if line.strip() == "Cargo.toml" or line.strip().endswith("/Cargo.toml")
    ]

    violations: list[str] = []
    for path in cargo_tomls:
        before = _show(base, path)
        after = _show("HEAD", path)
        v = find_violation(before, after, path)
        if v:
            violations.append(v)

    if violations:
        print("✗ version-ownership lint failed — feature PRs must not bump crate versions.")
        print("  release-plz owns every [package] version bump; do it on the release PR.")
        print("  See AGENTS.md 'Version ownership' and crap-rs#448.\n")
        for v in violations:
            print(f"  - {v}")
        return 1

    n = len(cargo_tomls)
    print(f"✓ version-ownership lint passed ({n} changed Cargo.toml file(s) checked; no version bumps).")
    return 0


# --------------------------------------------------------------------------
# self-test
# --------------------------------------------------------------------------

def self_test() -> int:
    pkg = lambda v: f'[package]\nname = "x"\nversion = "{v}"\n'
    cases = [
        # (name, before, after, expect_violation)
        ("bump fails", pkg("0.5.0"), pkg("0.8.0"), True),
        ("no change passes", pkg("0.5.0"), pkg("0.5.0"), False),
        ("new crate passes", None, pkg("0.1.0"), False),
        ("deleted crate passes", pkg("0.5.0"), None, False),
        (
            "dependency-only change passes",
            '[package]\nname = "x"\nversion = "0.5.0"\n[dependencies]\nserde = "1.0.190"\n',
            '[package]\nname = "x"\nversion = "0.5.0"\n[dependencies]\nserde = "1.0.200"\n',
            False,
        ),
        (
            "inline dep-table version change passes",
            '[package]\nname = "x"\nversion = "0.5.0"\n[dependencies]\nfoo = { version = "1.0" }\n',
            '[package]\nname = "x"\nversion = "0.5.0"\n[dependencies]\nfoo = { version = "2.0" }\n',
            False,
        ),
        (
            "workspace version bump fails",
            '[workspace.package]\nversion = "0.5.0"\n',
            '[workspace.package]\nversion = "0.6.0"\n',
            True,
        ),
        (
            "virtual-manifest no [package] passes",
            '[workspace]\nmembers = ["a"]\n',
            '[workspace]\nmembers = ["a", "b"]\n',
            False,
        ),
        (
            "version.workspace inherit (no literal bump) passes",
            '[package]\nname = "x"\nversion.workspace = true\n',
            '[package]\nname = "x"\nversion.workspace = true\n[dependencies]\nq = "1"\n',
            False,
        ),
        (
            "malformed after is skipped (fail-open)",
            pkg("0.5.0"),
            '[package\nname = "x" version = ',
            False,
        ),
    ]
    failures = 0
    for name, before, after, expect in cases:
        got = find_violation(before, after, "Cargo.toml") is not None
        ok = got == expect
        print(f"  [{'PASS' if ok else 'FAIL'}] {name} (expected violation={expect}, got={got})")
        if not ok:
            failures += 1
    if failures:
        print(f"\n✗ self-test: {failures} case(s) failed")
        return 1
    print(f"\n✓ self-test: all {len(cases)} cases passed")
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        sys.exit(self_test())
    sys.exit(run_cli())
