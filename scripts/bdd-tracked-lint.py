#!/usr/bin/env python3
"""BDD hygiene lint — mechanical enforcement of two AGENTS.md
"BDD hygiene" rules over every `Scenario:` / `Scenario Outline:`:

* **Rule 1 (status-tag presence):** every scenario carries EXACTLY ONE
  status tag from `@wired` / `@unwired` / `@wip`. Zero status tags
  (the scenario silently escapes the wired/unwired/wip lexicon) and
  more than one (mutually-exclusive tags can't coexist) both fail.
* **Rule 2 (tracking-comment presence):** every `@unwired` or `@wip`
  scenario carries a `# tracked: crap-rs#<n> — <reason>` comment inside
  its scenario block, naming the issue that owns the wiring deferral.

The *why* lives in `AGENTS.md` ("BDD hygiene" → Rules 1 and 2). This
script is the *what*: replaces the documented rules with a CI +
lefthook gate so a future agent cannot land an untagged / multiply-
tagged scenario (Rule 1) or a deferred scenario without naming the
deferral's owning issue (Rule 2).

Same mechanical-enforcement pattern as `scripts/mutants-skip-lint.py`
(documentation rots; CI doesn't). Single source of truth in this
script; `.github/workflows/ci.yml` and `lefthook.yml` both invoke it.

Scope: every `*.feature` file under `crates/*/tests/features/`.

Algorithm:

  1. Parse each `.feature` file into scenario blocks. A block bundles
     the contiguous tag line(s) immediately preceding a `Scenario:` or
     `Scenario Outline:` header with that header and its body
     (everything up to the next tag-line / scenario-header / EOF).
  2. Rule 1: count the status tags (`@wired`/`@unwired`/`@wip`) across
     the block's tag lines. Report a violation unless the count is
     exactly 1.
  3. Rule 2: if the (single) status tag is `@unwired` or `@wip`, scan
     the body lines for a `# tracked: crap-rs#<n>` comment; report a
     violation if absent.
  4. Collect violations from BOTH rules across all files; exit 1 if any
     fired, 0 otherwise. Rule 1 does not short-circuit Rule 2 — a
     scenario can carry exactly one `@unwired` tag (Rule 1 ok) yet
     still miss its tracked-comment (Rule 2 fails).

A `--self-test` flag exercises both rule branches against in-memory
fixtures (zero-tag fail, multi-tag fail, single-tag pass, untracked
deferral fail) so the lint logic itself is regression-guarded.

Limitations:

* **Block tracking is comment-aware but assumes well-formed Gherkin.**
  Tag lines must start with `@` (Gherkin spec); the lint trusts that.
  Pathological inputs (commented-out scenario headers inside an
  `Examples:` table cell, etc.) are out of scope — no current
  `.feature` file exhibits the pattern.
* **Tracked-comment form is fixed.** The regex matches `# tracked:
  crap-rs#<digits>`. Other shapes (e.g. `# tracked: org/repo#N`,
  `tracked:` without leading `#`) are not recognized. The
  fixed-shape rule keeps grep / audit cheap.
* **Examples-table semantics:** the body of a `Scenario Outline:`
  includes its `Examples:` table; the lint sees that as a body line
  like any other, so a tracked-comment may live either above the
  `Examples:` keyword or anywhere inside the table — both pass.

If any of these gaps produces a real regression, extend the lint.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

# Rule 2 helper: the *deferral* status tags only (`@unwired` / `@wip`),
# the two that require a `# tracked:` comment. A status tag MUST appear
# on its own tag line (Gherkin convention), leading whitespace allowed,
# as a discrete token (separated by whitespace from other tags on the
# same line, if any).
STATUS_TAG_RE = re.compile(r"(?:^|\s)@(unwired|wip)(?=\s|$)")
# Rule 1 helper: ALL three mutually-exclusive status tags. Distinct
# from STATUS_TAG_RE (which omits `@wired`) so a `@wired` scenario reads
# as one status tag, not zero. Word-bounded so a scope tag like
# `@smoke` (or a future `@wired_up`) is NOT miscounted as a status tag.
STATUS_COUNT_RE = re.compile(r"(?:^|\s)@(wired|unwired|wip)(?=\s|$)")
TAG_LINE_RE = re.compile(r"^\s*@")
SCENARIO_LINE_RE = re.compile(r"^\s*Scenario(?::|\s+Outline:)\s*(.*)$")
# Enforce the AGENTS.md "BDD hygiene" Rule 2 example shape exactly:
#   # tracked: crap-rs#<digits> — <one-line non-empty reason>
# `^\s*#` so the comment must be a true Gherkin comment line (not an
# inline trailing comment in a step). The em-dash is U+2014 (the exact
# glyph the AGENTS.md example uses); ASCII `--` is rejected. The
# reason must contain at least one non-space character so an empty
# trailer (`crap-rs#123 — `) cannot pass.
TRACKED_RE = re.compile(r"^\s*#\s*tracked:\s*crap-rs#\d+\s+—\s+\S.*$")

# Dynamic discovery — every `crates/*/tests/features/` directory is in
# scope. Hard-coding `crap4rs` + `crap4ts` worked today but silently
# omits any future adapter crate (e.g. a `crap4py`); the glob keeps the
# lint future-proof without per-crate maintenance.
FEATURE_DIR_GLOB = "crates/*/tests/features"


@dataclass
class ScenarioBlock:
    file: Path
    header_line: int  # 1-indexed line of the `Scenario:` header
    tag_lines: list[tuple[int, str]]  # 1-indexed; line text
    body_lines: list[tuple[int, str]]  # 1-indexed; line text
    title: str

    def status_tag(self) -> str | None:
        for _, txt in self.tag_lines:
            m = STATUS_TAG_RE.search(txt)
            if m:
                return f"@{m.group(1)}"
        return None

    def status_tags(self) -> list[str]:
        """Every status tag (`@wired`/`@unwired`/`@wip`) across all of
        this block's tag lines, in source order. Rule 1 requires the
        list to have length exactly 1; this method is the count source.
        Multiple status tags on one line and across several tag lines
        are both captured."""
        found: list[str] = []
        for _, txt in self.tag_lines:
            for m in STATUS_COUNT_RE.finditer(txt):
                found.append(f"@{m.group(1)}")
        return found

    def has_tracked_comment(self) -> bool:
        return any(TRACKED_RE.search(txt) for _, txt in self.body_lines)


def parse_blocks(feature_path: Path) -> list[ScenarioBlock]:
    """Walk `feature_path` line-by-line. Group contiguous tag lines with
    the next `Scenario:`/`Scenario Outline:` header; treat everything
    up to the next tag-line, scenario-header, or EOF as that scenario's
    body."""
    lines = feature_path.read_text().splitlines()
    blocks: list[ScenarioBlock] = []

    i = 0
    pending_tags: list[tuple[int, str]] = []
    n = len(lines)
    while i < n:
        line = lines[i]
        stripped = line.strip()

        if TAG_LINE_RE.match(line):
            pending_tags.append((i + 1, line))
            i += 1
            continue

        sc_match = SCENARIO_LINE_RE.match(line)
        if sc_match:
            header_line = i + 1
            title = sc_match.group(1).strip()
            body: list[tuple[int, str]] = []
            j = i + 1
            while j < n:
                next_line = lines[j]
                # End of scenario when we hit the next scenario's tag
                # block OR a bare `Scenario:`/`Scenario Outline:`
                # without preceding tags.
                if TAG_LINE_RE.match(next_line) or SCENARIO_LINE_RE.match(next_line):
                    break
                body.append((j + 1, next_line))
                j += 1

            blocks.append(
                ScenarioBlock(
                    file=feature_path,
                    header_line=header_line,
                    tag_lines=pending_tags,
                    body_lines=body,
                    title=title,
                )
            )
            pending_tags = []
            i = j
            continue

        # Non-tag, non-scenario line outside a scenario block (e.g.
        # `Feature:` keyword, freeform prose, `Background:` body).
        # Anything dangling in `pending_tags` belongs to a structural
        # block we don't enforce here — drop and move on. (A tag block
        # left dangling without a following scenario is malformed
        # Gherkin; cucumber would already reject it, so no point
        # double-reporting.)
        if stripped and not stripped.startswith("#"):
            pending_tags = []
        i += 1

    return blocks


def check_block(block: ScenarioBlock, rel: Path) -> list[str]:
    """Apply both BDD-hygiene rules to one scenario block. Returns a
    list of formatted violation messages (empty when the block is
    clean). Rule 1 and Rule 2 are independent — a block can fail both,
    or pass Rule 1 yet fail Rule 2, so neither short-circuits the
    other."""
    violations: list[str] = []

    # Rule 1: exactly one status tag.
    statuses = block.status_tags()
    if len(statuses) == 0:
        violations.append(
            f"\nbdd-tracked-lint: missing status tag (Rule 1)\n"
            f"  {rel}:{block.header_line}  Scenario: {block.title}\n"
            f"  carries no status tag. Every scenario MUST carry exactly "
            f"one of `@wired` / `@unwired` / `@wip`.\n\n"
            f"  Fix: add a status tag on the line directly above the "
            f"`Scenario:` header. Default to `@unwired` (with a "
            f"`# tracked: crap-rs#<n>` comment in the body) for a scenario "
            f"no harness exercises yet; see AGENTS.md \"BDD hygiene\" "
            f"Rule 1 for why."
        )
    elif len(statuses) > 1:
        violations.append(
            f"\nbdd-tracked-lint: multiple status tags (Rule 1)\n"
            f"  {rel}:{block.header_line}  Scenario: {block.title}\n"
            f"  carries {len(statuses)} status tags "
            f"({', '.join(statuses)}). The status tags are mutually "
            f"exclusive — a scenario MUST carry exactly one.\n\n"
            f"  Fix: keep the single tag that reflects the scenario's "
            f"true wiring state and remove the rest; see AGENTS.md "
            f"\"BDD hygiene\" Rule 1 for why."
        )

    # Rule 2: `@unwired` / `@wip` scenarios need a `# tracked:` comment.
    # Uses status_tag() (the deferral-only scanner) so the rule fires
    # whenever a deferral tag is present, independent of Rule 1's count.
    status = block.status_tag()
    if status in ("@unwired", "@wip") and not block.has_tracked_comment():
        violations.append(
            f"\nbdd-tracked-lint: missing tracked-comment on "
            f"{status} scenario (Rule 2)\n"
            f"  {rel}:{block.header_line}  Scenario: {block.title}\n"
            f"  carries `{status}` but no `# tracked: crap-rs#<n>` "
            f"comment in its body.\n\n"
            f"  Fix: add a comment of the exact shape\n"
            f"    # tracked: crap-rs#<n> — <one-line reason>\n"
            f"  inside the scenario block (between the `Scenario:` "
            f"header and the next scenario). Open or reuse a "
            f"tracking issue first; see AGENTS.md \"BDD hygiene\" "
            f"Rule 2 for why."
        )

    return violations


def lint(repo_root: Path) -> int:
    errors: list[str] = []
    scanned = 0
    deferred = 0

    for abs_dir in sorted(repo_root.glob(FEATURE_DIR_GLOB)):
        if not abs_dir.is_dir():
            continue
        for feature_path in sorted(abs_dir.rglob("*.feature")):
            rel = feature_path.relative_to(repo_root)
            for block in parse_blocks(feature_path):
                scanned += 1
                if block.status_tag() in ("@unwired", "@wip"):
                    deferred += 1
                errors.extend(check_block(block, rel))

    if errors:
        for msg in errors:
            print(msg, file=sys.stderr)
        n = len(errors)
        print(
            f"\nbdd-tracked-lint: {n} BDD-hygiene "
            f"{'violation' if n == 1 else 'violations'}; see above "
            f"({scanned} scanned, {deferred} deferred)",
            file=sys.stderr,
        )
        return 1

    print(
        f"bdd-tracked-lint: ok (Rule 1: all {scanned} scenarios carry "
        f"exactly one status tag; Rule 2: all {deferred} deferred "
        f"scenarios carry a `# tracked: crap-rs#<n>` comment)"
    )
    return 0


def self_test() -> int:
    """Exercise both rule branches against in-memory fixtures so the
    lint logic is regression-guarded independently of the live corpus.
    Each fixture is written to a temp `crates/<crate>/tests/features/`
    tree (the same layout the live lint globs) and run through `lint`,
    asserting the expected exit code. Returns 0 on all-pass, 1 on any
    failure."""
    import tempfile

    cases: list[tuple[str, str, int]] = [
        (
            "rule1-zero-tag-fails",
            "Feature: F\n\n  Scenario: no status tag\n"
            "    When x\n    Then y\n",
            1,
        ),
        (
            "rule1-multi-tag-fails",
            "Feature: F\n\n  @wired\n  @unwired\n  Scenario: two status tags\n"
            "    # tracked: crap-rs#169 — fixture\n    When x\n    Then y\n",
            1,
        ),
        (
            "rule1-multi-tag-same-line-fails",
            "Feature: F\n\n  @wired @wip\n  Scenario: two on one line\n"
            "    # tracked: crap-rs#169 — fixture\n    When x\n    Then y\n",
            1,
        ),
        (
            "wired-single-tag-passes",
            "Feature: F\n\n  @wired\n  Scenario: one wired tag\n"
            "    When x\n    Then y\n",
            0,
        ),
        (
            "unwired-tracked-passes",
            "Feature: F\n\n  @unwired\n  Scenario: deferred and tracked\n"
            "    # tracked: crap-rs#169 — fixture reason\n"
            "    When x\n    Then y\n",
            0,
        ),
        (
            "rule2-unwired-untracked-fails",
            "Feature: F\n\n  @unwired\n  Scenario: deferred but untracked\n"
            "    When x\n    Then y\n",
            1,
        ),
        (
            "scope-tag-not-counted-as-status",
            "Feature: F\n\n  @wired @smoke\n  Scenario: status plus scope\n"
            "    When x\n    Then y\n",
            0,
        ),
        (
            "outline-zero-tag-fails",
            "Feature: F\n\n  Scenario Outline: untagged outline\n"
            "    When <x>\n    Then ok\n\n  Examples:\n      | x |\n      | 1 |\n",
            1,
        ),
    ]

    failures = 0
    for name, body, expected in cases:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            feat_dir = root / "crates" / "fixture" / "tests" / "features"
            feat_dir.mkdir(parents=True)
            (feat_dir / f"{name}.feature").write_text(body)
            # Silence the per-case stdout/stderr; only the exit code matters.
            import contextlib
            import io

            buf = io.StringIO()
            with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
                got = lint(root)
            if got != expected:
                print(
                    f"bdd-tracked-lint --self-test: FAIL {name}: "
                    f"expected exit {expected}, got {got}",
                    file=sys.stderr,
                )
                failures += 1

    if failures:
        print(
            f"bdd-tracked-lint --self-test: {failures} case(s) failed",
            file=sys.stderr,
        )
        return 1
    print(f"bdd-tracked-lint --self-test: ok ({len(cases)} cases passed)")
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        sys.exit(self_test())
    sys.exit(lint(Path(__file__).resolve().parent.parent))
