#!/usr/bin/env python3
"""Encoded-defect lint — a deferred/known-wrong VALUE frozen into a test,
snapshot, or fixture must carry a tracking anchor.

The *why* lives in AGENTS.md ("Encoded-defect discipline") and the ADR
(`adr-release-process-and-public-api-boundary`). This script is the
*what*. The rc.4 release shipped a wire envelope whose `language` field
was frozen to the wrong value (`"rust"` for the TypeScript adapter) with
a source comment labeling it deliberately wrong "pending a later fix" —
and because the snapshot WAS the oracle, the deferred defect sailed green
through every PR until it was published. (See memory
`snapshot-not-value-oracle`: a shape-lock snapshot is a tautological
oracle.)

This lint makes that class of deferral ACCOUNTABLE — the same discipline
as `~/.claude/rules/exclusions.md` and `scripts/bdd-tracked-lint.py`: if
you knowingly encode a wrong value, you must anchor it to an issue.

What it flags: a line containing an unambiguous "deliberately wrong"
marker that lacks a nearby anchor comment.

  Markers (case-insensitive):
    wrong-by-design | wrong by design | known-wrong | known wrong |
    wrong-on-purpose | wrong on purpose | deliberately wrong |
    intentionally wrong

  Anchor (within ANCHOR_WINDOW lines of the marker, same file):
    tracked: crap-rs#<n>     (active deferral — an open issue owns the fix)
    resolved: crap-rs#<n>    (historical/post-mortem reference to a CLOSED
                              issue — e.g. a doc comment narrating a defect
                              that has SINCE been fixed)

Why these markers and NOT bare "by design": "by design" appears benignly
across the repo (a metric is "adapter-agnostic by design", the staleness
check "warns, never fails — by design", etc.). The discriminator is the
word **wrong** (or "known-wrong" / "on-purpose"): a value described as
*wrong* on purpose is, by definition, an encoded defect. Bare "by design"
is a legitimate design statement and is deliberately NOT matched.

Honest limitation (mirrors memory `never-raises-is-not-never-hides`): this
is a HEURISTIC text matcher. It cannot detect a deferred wrong value that
carries NO marker comment at all (the writer simply stays silent) — only
the discipline of *labeling* deferrals is enforceable here; the labeling
itself is a convention. The lint's job is to ensure that WHEN a deferral
is labeled, it is also anchored. Its precise-marker design keeps the
false-match floor near zero (verified by an adversarial FP-hunt over the
whole tree at authoring time); the `--self-test` guards the rule logic.

Scope: tracked source/test/snapshot/fixture files under `crates/`
(`.rs .snap .json .ts .toml .feature`). EXCLUDES `scripts/` (this lint
and its siblings name the markers in their own source) and `*.md`
(AGENTS.md / READMEs document the rule using the marker words). A
deferred *value* lives in code/tests/snapshots/fixtures, not in prose
docs or lint source — scoping there is what keeps the lint from matching
its own definition.
"""

from __future__ import annotations

import re
import subprocess
import sys

# Unambiguous "deliberately wrong" markers. The shared discriminator is
# "wrong" / "known-wrong" / "on-purpose" — NOT bare "by design".
MARKER_RE = re.compile(
    r"wrong[-\s]by[-\s]design"
    r"|known[-\s]wrong"
    r"|wrong[-\s]on[-\s]purpose"
    r"|deliberately[-\s]wrong"
    r"|intentionally[-\s]wrong",
    re.IGNORECASE,
)

ANCHOR_RE = re.compile(r"(?:tracked|resolved):\s*crap-rs#\d+", re.IGNORECASE)

# A deferral comment usually sits on the marker line or immediately
# around it. Window is generous enough to span a short doc block but
# small enough that one anchor can't excuse an unrelated marker.
ANCHOR_WINDOW = 6

SCAN_EXTS = (".rs", ".snap", ".json", ".ts", ".toml", ".feature")


def find_violations(text: str, path: str) -> list[str]:
    """Return violation messages for marker lines in `text` lacking a
    nearby anchor. Pure function — the unit under self-test."""
    lines = text.splitlines()
    violations: list[str] = []
    for i, line in enumerate(lines):
        m = MARKER_RE.search(line)
        if not m:
            continue
        lo = max(0, i - ANCHOR_WINDOW)
        hi = min(len(lines), i + ANCHOR_WINDOW + 1)
        window = "\n".join(lines[lo:hi])
        if ANCHOR_RE.search(window):
            continue
        violations.append(
            f"{path}:{i + 1}: encoded-defect marker {m.group(0)!r} without a "
            f"`tracked: crap-rs#<n>` (active) or `resolved: crap-rs#<n>` "
            f"(historical) anchor within {ANCHOR_WINDOW} lines"
        )
    return violations


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------

def _tracked_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "crates/"],
        capture_output=True,
        text=True,
        # Pin UTF-8 (not the locale default — CP1252 on some Windows
        # runners) so non-ASCII tracked paths decode consistently.
        encoding="utf-8",
        errors="replace",
        check=True,
    ).stdout
    files = []
    for line in out.splitlines():
        p = line.strip()
        if not p or not p.endswith(SCAN_EXTS):
            continue
        if p.startswith("scripts/"):
            continue
        files.append(p)
    return files


def run_cli() -> int:
    violations: list[str] = []
    for path in _tracked_files():
        try:
            with open(path, encoding="utf-8") as fh:
                text = fh.read()
        except (OSError, UnicodeDecodeError):
            continue  # binary / unreadable — skip
        violations.extend(find_violations(text, path))

    if violations:
        print("✗ encoded-defect lint failed — a deliberately-wrong value must be anchored.")
        print("  Add `tracked: crap-rs#<n>` (open issue owns the fix) or, for a")
        print("  post-mortem reference to an already-fixed issue, `resolved: crap-rs#<n>`.")
        print("  See AGENTS.md 'Encoded-defect discipline' and memory snapshot-not-value-oracle.\n")
        for v in violations:
            print(f"  - {v}")
        return 1

    print("✓ encoded-defect lint passed (no unanchored deliberately-wrong markers).")
    return 0


# --------------------------------------------------------------------------
# self-test
# --------------------------------------------------------------------------

def self_test() -> int:
    cases = [
        # (name, text, expect_violation_count)
        ("unanchored wrong-by-design fails", "let x = 1; // wrong-by-design\n", 1),
        (
            "anchored (tracked) passes",
            "// wrong-by-design pending the fix\n// tracked: crap-rs#450 — language flip\nlet x = 1;\n",
            0,
        ),
        (
            "anchored (resolved) passes — post-mortem",
            "//! ## Wrong-by-design values that flipped at W2.5\n"
            "//! resolved: crap-rs#449 — now stamps the adapter's real language\n",
            0,
        ),
        ("bare 'by design' NOT matched", "// adapter-agnostic by design — source format is irrelevant\n", 0),
        ("bare 'by design' (warns never fails) NOT matched", "// warns, never fails — by design\n", 0),
        ("known-wrong unanchored fails", "expected = 2.0  # known-wrong placeholder\n", 1),
        (
            "known-wrong anchored passes",
            "expected = 2.0  # known-wrong; tracked: crap-rs#999 — recalibrate\n",
            0,
        ),
        ("wrong-on-purpose unanchored fails", "value: 'rust'  // wrong on purpose\n", 1),
        ("intentionally wrong unanchored fails", "x = 1 // intentionally wrong for now\n", 1),
        ("benign 'wrong' word NOT matched", "// returns an error if the path is wrong\n", 0),
        ("benign 'design' word NOT matched", "// the design of this module follows hexagonal ports\n", 0),
        (
            "anchor outside window does NOT excuse",
            "// wrong-by-design\n" + ("// filler\n" * 8) + "// tracked: crap-rs#1\n",
            1,
        ),
        (
            "two markers, one shared anchor in window, both pass",
            "// wrong-by-design here\n// and known-wrong there\n// tracked: crap-rs#7\n",
            0,
        ),
    ]
    failures = 0
    for name, text, expect in cases:
        got = len(find_violations(text, "x"))
        ok = got == expect
        print(f"  [{'PASS' if ok else 'FAIL'}] {name} (expected {expect}, got {got})")
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
