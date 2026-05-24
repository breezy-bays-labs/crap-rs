#!/usr/bin/env python3
"""BDD tracked-comment lint — mechanical enforcement of the AGENTS.md
"BDD hygiene" Rule 2: every `@unwired` or `@wip` scenario must carry a
`# tracked: crap-rs#<n>` comment inside its scenario block, naming the
issue that captures the wiring deferral.

The *why* lives in `AGENTS.md` ("BDD hygiene" → Rule 2 → "@unwired and
@wip require a tracking comment"). This script is the *what*: replaces
the documented rule with a CI + lefthook gate so a future agent cannot
land a deferred scenario without naming the deferral's owning issue.

Same mechanical-enforcement pattern as `scripts/mutants-skip-lint.py`
(documentation rots; CI doesn't). Single source of truth in this
script; `.github/workflows/ci.yml` and `lefthook.yml` both invoke it.

Scope: every `*.feature` file under `crates/*/tests/features/`.

Algorithm:

  1. Parse each `.feature` file into scenario blocks. A block bundles
     the contiguous tag line(s) immediately preceding a `Scenario:` or
     `Scenario Outline:` header with that header and its body
     (everything up to the next tag-line / scenario-header / EOF).
  2. For each block, if any tag line carries `@unwired` or `@wip`,
     scan the body lines for a `# tracked: crap-rs#<n>` comment.
  3. Report (file, scenario line, scenario title, tag) for every
     missing tracked-comment. Exit 1 on any miss, 0 otherwise.

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

# A status tag MUST appear on its own tag line (Gherkin convention),
# leading whitespace allowed, followed by `@unwired` or `@wip` as a
# discrete token (separated by whitespace from other tags on the same
# line, if any).
STATUS_TAG_RE = re.compile(r"(?:^|\s)@(unwired|wip)(?=\s|$)")
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


def lint(repo_root: Path) -> int:
    errors: list[str] = []
    scanned = 0
    deferred = 0

    for abs_dir in sorted(repo_root.glob(FEATURE_DIR_GLOB)):
        if not abs_dir.is_dir():
            continue
        for feature_path in sorted(abs_dir.rglob("*.feature")):
            for block in parse_blocks(feature_path):
                scanned += 1
                status = block.status_tag()
                if status not in ("@unwired", "@wip"):
                    continue
                deferred += 1
                if block.has_tracked_comment():
                    continue
                rel = feature_path.relative_to(repo_root)
                errors.append(
                    f"\nbdd-tracked-lint: missing tracked-comment on "
                    f"{status} scenario\n"
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

    if errors:
        for msg in errors:
            print(msg, file=sys.stderr)
        print(
            f"\nbdd-tracked-lint: {len(errors)} uncovered {('scenario' if len(errors) == 1 else 'scenarios')}; "
            f"see above ({scanned} scanned, {deferred} deferred)",
            file=sys.stderr,
        )
        return 1

    print(
        f"bdd-tracked-lint: ok ({deferred} deferred scenarios all carry "
        f"a `# tracked: crap-rs#<n>` comment; {scanned} scanned)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(lint(Path(__file__).resolve().parent.parent))
