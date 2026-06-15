#!/usr/bin/env python3
"""BDD Boundary-Rule "mis-leveling" lint — the mechanical *shadow* of the
org Testing Strategy Boundary Rule for crap-rs's cucumber suite.

The Boundary Rule's keystone — "is THIS behavior a product-level contract
a consumer relies on?" — is irreducibly JUDGMENT (no lint can read Gherkin
prose and decide whether it encodes an external promise vs an internal
implementation detail). This lint does NOT attempt that. It enforces the
handful of mis-leveling signals that ARE mechanically provable, and leaves
the judgment call to a CQO / council BDD audit. See AGENTS.md
"BDD hygiene" → "BDD Boundary-Rule lint" for the *why*; this script is the
*what*, the single source of truth that `.github/workflows/ci.yml` and
`lefthook.yml` both invoke (documentation rots; CI doesn't).

It is a sibling of `scripts/bdd-tracked-lint.py` (status-tag / tracked
hygiene over `.feature` files) and `scripts/mutants-skip-lint.py` (which
likewise scans Rust test source heuristically without a Rust parser). It
reuses bdd-tracked-lint's scenario-block parser idiom AND mutants-skip's
whole-file regex idiom, because it joins two input classes:

  * `crates/*/tests/*_cucumber.rs` — the cucumber HARNESSES (Rust)
  * `crates/*/tests/features/*.feature` — the Gherkin SPECS

Rules (FAIL-tier block the build; WARN-tier print `::warning::` only):

  * RULE A (FAIL) — narration/execution mismatch. A `@wired`/`@wip`
    scenario whose step prose narrates a backtick-quoted CLI invocation
    of a known analyzer binary (`crap4rs` / `crap4ts` / `crap-render`),
    while its bound harness contains ZERO process-spawn markers
    (`cargo_bin(` / `CARGO_BIN_EXE_`). The scenario advertises a
    CLI-level product contract but executes a library/adapter call —
    the Boundary Rule's "push it down" clause made mechanical. The fix
    is honest narration (rewrite to adapter-level prose, as
    `json_reporter` already does) or actually spawning the binary.
  * RULE A-OPTOUT (FAIL) — if a harness carries the token
    `// bdd-lint: lib-direct-by-design`, it MUST be the full tracked
    shape `// bdd-lint: lib-direct-by-design — tracked: crap-rs#<n> —
    <reason>` (U+2014 em-dash, non-empty reason). A malformed marker
    fails with a shape-fix message.
  * RULE A-COHERENCE (FAIL) — a harness carrying a *well-formed*
    lib-direct opt-out admits it is library-level, so its bound feature
    must NOT narrate a CLI run in any `@wired`/`@wip` scenario. (Forces
    the honest fix rather than papering a lying narration with a marker.)
  * RULE B (FAIL) — orphan/unresolvable harness link. Every
    `*_cucumber.rs` must declare exactly one parseable
    `(filter_)run_and_exit("tests/features/<X>.feature")` path, and that
    `.feature` must exist on disk. Zero / multiple / dangling all fail.
    This is the precondition that makes RULE A's join sound (without it
    RULE A could silently no-op on a harness it failed to map — the
    CI-silent-pass failure class).
  * RULE D (WARN) — `bdd-asserts-only` marker shape. If a harness carries
    a `// bdd-asserts-only: <crate>::<path> — tracked: crap-rs#<n>`
    annotation (a contributor's voluntary, honest declaration that the
    harness knowingly mirrors a named lower-level test), validate its
    shape. Presence-triggered only; absence is NEVER a violation. This
    is the honest mechanizable form of "this scenario duplicates a named
    unit test" — the *inference* of that duplication is below the
    false-match floor and is explicitly NOT attempted.

A `--self-test` flag exercises every rule branch against in-memory
fixtures (a temp `crates/<crate>/tests/` tree with a harness + feature)
so the lint logic is itself regression-guarded; CI runs it as a separate
preceding step.

Limitations (the accepted, documented gaps):

  * Spawn detection is an ABSENCE-based whole-file substring scan, not
    string-literal/block-comment aware. A `cargo_bin`/`CARGO_BIN_EXE_`
    token sitting in a comment or string literal in an otherwise
    lib-direct harness can only SUPPRESS a RULE A fire (a false NEGATIVE
    — the safe direction for a hard gate), never create one.
  * HARNESS-LEVEL, not step-level, granularity. A mixed harness (some
    scenarios shell the bin, some lib-direct) with >=1 spawn marker
    anywhere never fires RULE A. No such mixed harness exists today; the
    escalation if one appears is a per-scenario annotation, not brittle
    per-step regex resolution.
  * CLI-narration keys on a BACKTICK-quoted binary name. Dropping the
    backticks ("the operator runs crap4rs --top 10") evades the gate.
    The corpus uses backticks universally today.
  * Only sync `(filter_)run_and_exit(...)` calls with a LITERAL string
    path are recognized for the RULE B join. A variable/const/function
    path (`run_and_exit(PATH)`) is not resolved — it reads as an orphan
    (a loud RULE B failure, the safe direction), not a silent skip. Every
    harness uses a literal path today.
  * RULE A narration is detected on `When` steps only. A CLI narration
    split onto an `And` continuation of a `When` would be missed (an
    accepted, safe false-negative). `Then`/`Given` lines, comments,
    docstring bodies, and `Examples:` table cells are deliberately NOT
    scanned — a backtick binary name there is an assertion, a config-file
    reference, or example data, not narration.

If any of these gaps produces a real regression, extend the lint.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

# ── Harness (Rust) scanners ──────────────────────────────────────────
# Process-spawn markers: the assert_cmd form `cargo_bin(` and the
# hand-rolled `Command::new(env!("CARGO_BIN_EXE_<bin>"))` form. Keyed on
# these two tokens only (NOT a bare `Command::new`, which a harness uses
# for `git` fixture setup without ever spawning the analyzer).
SPAWN_RE = re.compile(r"cargo_bin\(|CARGO_BIN_EXE_")
# The feature path a harness binds via (filter_)run_and_exit. `\s*` spans
# the newline for the ~quarter of harnesses whose rustfmt wraps the string
# literal onto the line after `.filter_run_and_exit(`.
RUN_EXIT_RE = re.compile(
    r'(?:filter_)?run_and_exit\(\s*"(tests/features/[^"]+\.feature)"'
)
# RULE A-OPTOUT: token presence vs full shape. The shape mirrors
# bdd-tracked-lint's TRACKED_RE (U+2014 em-dash, non-empty reason).
OPTOUT_TOKEN_RE = re.compile(r"//\s*bdd-lint:\s*lib-direct-by-design")
OPTOUT_SHAPE_RE = re.compile(
    r"^\s*//\s*bdd-lint:\s*lib-direct-by-design\s+—\s+tracked:\s*crap-rs#\d+\s+—\s+\S.*$"
)
# RULE D: the voluntary "this harness mirrors a named lower test" marker.
ASSERTS_ONLY_TOKEN_RE = re.compile(r"//\s*bdd-asserts-only:")
# The target must be a `::`-qualified Rust path (`<crate>::<path>`) — a
# bare token before `tracked:` is a malformed marker, not a real test
# reference.
ASSERTS_ONLY_SHAPE_RE = re.compile(
    r"^\s*//\s*bdd-asserts-only:\s*\S+::\S+\s+—\s+tracked:\s*crap-rs#\d+\s+—\s+\S.*$"
)

# ── Feature (Gherkin) scanners ───────────────────────────────────────
STATUS_WIRED_RE = re.compile(r"(?:^|\s)@(wired|wip)(?=\s|$)")
TAG_LINE_RE = re.compile(r"^\s*@")
SCENARIO_LINE_RE = re.compile(r"^\s*Scenario(?::|\s+Outline:)\s*(.*)$")
# A `When` step narrating a backtick-quoted CLI invocation of a known
# analyzer binary. Backtick, optional space, the binary name on a word
# boundary, NOT immediately followed by `.` (so a config filename like
# `crap4rs.toml` is excluded — it names a file, not a run). Library-shaped
# prose ("the JSON is formatted", "the oxc walker analyzes the source")
# carries no backtick-quoted binary and does NOT match — that
# honest-narration distinction is what makes RULE A precise rather than
# merely zero-spawn-keyed.
CLI_NARRATION_RE = re.compile(r"`\s*(crap4rs|crap4ts|crap-render)\b(?!\.)")
# Narration is what the ACTOR does — a `When` step. A backtick binary name
# in a comment aside, a `Then`/`Given` line, an `And`/`But` continuation,
# a docstring body, or an `Examples:` table cell is NOT narration (it is an
# assertion about the CLI, a config-file reference, or example data) and
# must not trip RULE A. Restricting to `When` keeps the gate precise; a
# CLI narration split onto an `And` continuation is an accepted (safe)
# false-negative — see Limitations.
WHEN_STEP_RE = re.compile(r"^\s*When\b")

HARNESS_GLOB = "crates/*/tests/*_cucumber.rs"


@dataclass
class ScenarioBlock:
    file: Path
    header_line: int
    tag_lines: list[tuple[int, str]]
    body_lines: list[tuple[int, str]]
    title: str

    def is_active(self) -> bool:
        """True iff the scenario carries `@wired` or `@wip` — the states
        a harness is supposed to actually exercise. `@unwired` scenarios
        are skipped by the harness filter, so a narration mismatch on
        them is moot (and owned by bdd-tracked-lint's deferral rules)."""
        return any(STATUS_WIRED_RE.search(txt) for _, txt in self.tag_lines)

    def cli_narration_lines(self) -> list[tuple[int, str]]:
        return [
            (ln, txt)
            for ln, txt in self.body_lines
            if WHEN_STEP_RE.match(txt) and CLI_NARRATION_RE.search(txt)
        ]


def parse_blocks(feature_path: Path) -> list[ScenarioBlock]:
    """Group contiguous tag line(s) with the next `Scenario:` /
    `Scenario Outline:` header and its body (up to the next tag-line /
    scenario-header / EOF). Mirrors bdd-tracked-lint's parser."""
    lines = feature_path.read_text(encoding="utf-8").splitlines()
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
        sc = SCENARIO_LINE_RE.match(line)
        if sc:
            header_line = i + 1
            title = sc.group(1).strip()
            body: list[tuple[int, str]] = []
            j = i + 1
            while j < n:
                nxt = lines[j]
                if TAG_LINE_RE.match(nxt) or SCENARIO_LINE_RE.match(nxt):
                    break
                body.append((j + 1, nxt))
                j += 1
            blocks.append(
                ScenarioBlock(feature_path, header_line, pending_tags, body, title)
            )
            pending_tags = []
            i = j
            continue
        if stripped and not stripped.startswith("#"):
            pending_tags = []
        i += 1
    return blocks


@dataclass
class Finding:
    fatal: bool  # True → FAIL (exit 1); False → WARN (::warning::, exit unaffected)
    message: str


def resolve_feature(harness: Path, rel_path: str) -> Path:
    """A `run_and_exit("tests/features/X.feature")` path is relative to
    the harness's CRATE manifest dir (where cargo runs the test), i.e.
    `crates/<crate>/`, NOT the repo root. The harness lives at
    `crates/<crate>/tests/<name>_cucumber.rs`, so the crate dir is two
    parents up."""
    crate_dir = harness.parent.parent
    # `rel_path` is a forward-slash literal from the Rust source; split it
    # into components so the join is separator-correct on every platform.
    return crate_dir.joinpath(*rel_path.split("/"))


def line_of(text: str, needle_re: re.Pattern[str]) -> int:
    for idx, line in enumerate(text.splitlines(), start=1):
        if needle_re.search(line):
            return idx
    return 1


def check_harness(harness: Path, repo_root: Path) -> list[Finding]:
    findings: list[Finding] = []
    rel = harness.relative_to(repo_root)
    text = harness.read_text(encoding="utf-8")

    # ── RULE B: exactly one resolvable, existing feature path ────────
    paths = RUN_EXIT_RE.findall(text)
    bound_feature: Path | None = None
    if len(paths) == 0:
        findings.append(
            Finding(
                True,
                f"\nbdd-mislevel-lint: orphan harness — no feature link (RULE B)\n"
                f"  {rel}\n"
                f"  declares no `(filter_)run_and_exit(\"tests/features/<X>.feature\")` "
                f"path, so it binds to no spec.\n\n"
                f"  Fix: add the run_and_exit call naming the `.feature` this "
                f"harness exercises (the join RULE A relies on).",
            )
        )
    elif len(paths) > 1:
        findings.append(
            Finding(
                True,
                f"\nbdd-mislevel-lint: ambiguous harness link — {len(paths)} "
                f"feature paths (RULE B)\n"
                f"  {rel}\n"
                f"  declares {len(paths)} run_and_exit paths ({', '.join(paths)}); "
                f"a harness must bind exactly one feature.\n\n"
                f"  Fix: split the harness or keep the single canonical "
                f"`tests/features/<X>.feature` path.",
            )
        )
    else:
        feat = resolve_feature(harness, paths[0])
        if not feat.exists():
            findings.append(
                Finding(
                    True,
                    f"\nbdd-mislevel-lint: dangling harness link (RULE B)\n"
                    f"  {rel}  ->  {paths[0]}\n"
                    f"  names a `.feature` that does not exist on disk.\n\n"
                    f"  Fix: correct the run_and_exit path or restore the "
                    f"missing feature file.",
                )
            )
        else:
            bound_feature = feat

    # ── RULE A-OPTOUT: validate the lib-direct opt-out shape ─────────
    optout_valid = False
    if OPTOUT_TOKEN_RE.search(text):
        if any(OPTOUT_SHAPE_RE.match(line) for line in text.splitlines()):
            optout_valid = True
        else:
            findings.append(
                Finding(
                    True,
                    f"\nbdd-mislevel-lint: malformed lib-direct opt-out (RULE A-OPTOUT)\n"
                    f"  {rel}:{line_of(text, OPTOUT_TOKEN_RE)}\n"
                    f"  carries a `// bdd-lint: lib-direct-by-design` token in the "
                    f"wrong shape.\n\n"
                    f"  Fix: use the exact shape (U+2014 em-dash, non-empty reason)\n"
                    f"    // bdd-lint: lib-direct-by-design — tracked: crap-rs#<n> — <reason>",
                )
            )

    # ── RULE D: validate the bdd-asserts-only marker shape (WARN) ────
    if ASSERTS_ONLY_TOKEN_RE.search(text):
        if not any(ASSERTS_ONLY_SHAPE_RE.match(line) for line in text.splitlines()):
            findings.append(
                Finding(
                    False,
                    f"bdd-mislevel-lint: malformed bdd-asserts-only marker (RULE D) "
                    f"in {rel}:{line_of(text, ASSERTS_ONLY_TOKEN_RE)} — expected "
                    f"`// bdd-asserts-only: <crate>::<path> — tracked: crap-rs#<n> — <reason>`",
                )
            )

    # ── RULE A / A-COHERENCE: narration vs execution ────────────────
    # Only meaningful when (a) we resolved the bound feature and (b) the
    # harness has ZERO spawn markers (it executes a library/adapter call,
    # not the binary). With a spawn marker the harness genuinely runs the
    # CLI, so CLI narration is honest — no finding.
    if bound_feature is not None and not SPAWN_RE.search(text):
        narrating: list[tuple[int, str]] = []  # (feature_line, scenario_title)
        for block in parse_blocks(bound_feature):
            if not block.is_active():
                continue
            hits = block.cli_narration_lines()
            if hits:
                narrating.append((hits[0][0], block.title))
        if narrating:
            feat_rel = bound_feature.relative_to(repo_root)
            loc = "\n".join(
                f"    {feat_rel}:{ln}  (Scenario: {title})" for ln, title in narrating
            )
            if optout_valid:
                findings.append(
                    Finding(
                        True,
                        f"\nbdd-mislevel-lint: opted-out harness still narrates a CLI "
                        f"run (RULE A-COHERENCE)\n"
                        f"  {rel}\n"
                        f"  carries a valid `lib-direct-by-design` opt-out (it executes "
                        f"a library call, not the binary), but its bound feature still "
                        f"narrates a CLI invocation:\n{loc}\n\n"
                        f"  Fix: rewrite the narration to adapter-level prose "
                        f"(e.g. `When the walker analyzes the source`) so the spec "
                        f"stops claiming a CLI run it doesn't make.",
                    )
                )
            else:
                findings.append(
                    Finding(
                        True,
                        f"\nbdd-mislevel-lint: narration/execution mismatch (RULE A)\n"
                        f"  {rel}\n"
                        f"  has NO process-spawn marker (`cargo_bin(` / "
                        f"`CARGO_BIN_EXE_`) — it executes a library/adapter call — yet "
                        f"its bound `@wired`/`@wip` scenario narrates a CLI run:\n{loc}\n\n"
                        f"  The scenario advertises a CLI-level contract but exercises a "
                        f"library boundary (the Boundary Rule's push-it-down clause).\n"
                        f"  Fix (pick one):\n"
                        f"   - rewrite the narration to adapter-level prose (e.g. "
                        f"`When the walker analyzes the source`), as json_reporter does; OR\n"
                        f"   - make the harness actually spawn the binary "
                        f"(`cargo_bin`/`CARGO_BIN_EXE_`) if it IS a CLI contract; OR\n"
                        f"   - if it is deliberately library-level, declare it with\n"
                        f"     `// bdd-lint: lib-direct-by-design — tracked: crap-rs#<n> "
                        f"— <reason>` AND fix the narration (RULE A-COHERENCE still "
                        f"forbids CLI prose).",
                    )
                )

    return findings


def lint(repo_root: Path) -> int:
    fatal: list[str] = []
    warns: list[str] = []
    harnesses = 0
    for harness in sorted(repo_root.glob(HARNESS_GLOB)):
        if not harness.is_file():
            continue
        harnesses += 1
        for f in check_harness(harness, repo_root):
            (fatal if f.fatal else warns).append(f.message)

    for w in warns:
        print(f"::warning::{w}", file=sys.stderr)

    if fatal:
        for msg in fatal:
            print(msg, file=sys.stderr)
        n = len(fatal)
        print(
            f"\nbdd-mislevel-lint: {n} mis-leveling "
            f"{'violation' if n == 1 else 'violations'}; see above "
            f"({harnesses} harnesses scanned, {len(warns)} warning(s))",
            file=sys.stderr,
        )
        return 1

    print(
        f"bdd-mislevel-lint: ok ({harnesses} harnesses scanned; RULE A "
        f"narration/execution + RULE B feature-link clean"
        + (f"; {len(warns)} warning(s)" if warns else "")
        + ")"
    )
    return 0


def self_test() -> int:
    """Exercise every rule branch against in-memory crate trees (a
    harness + a feature in the `crates/<crate>/tests/` layout the live
    lint globs). Each case asserts an exit code AND an expected output
    substring so WARN-tier rules (exit 0) are still verified to fire."""
    import contextlib
    import io
    import tempfile

    # (name, harness_src, feature_src, run_and_exit_path, expected_exit,
    #  expected_substr, forbidden_substr)
    Case = tuple[str, str, str, str | None, int, str, str]

    LIB_HARNESS = (
        "fn main() {\n"
        '    World::cucumber().run_and_exit("__PATH__");\n'
        "    let _ = OxcWalker::extract();\n"  # library call, no spawn marker
        "}\n"
    )
    SPAWN_HARNESS = (
        'const BIN: &str = env!("CARGO_BIN_EXE_crap4rs");\n'
        "fn main() {\n"
        '    World::cucumber().run_and_exit("__PATH__");\n'
        "    Command::new(BIN).output();\n"
        "}\n"
    )
    CLI_FEATURE = (
        "Feature: F\n\n  @wired\n  Scenario: cli run\n"
        "    When the operator runs `crap4ts --coverage cov.json --src .`\n"
        "    Then ok\n"
    )
    LIB_FEATURE = (
        "Feature: F\n\n  @wired\n  Scenario: lib shaped\n"
        "    When the JSON is formatted\n    Then ok\n"
    )
    # @unwired CLI narration must NOT trip RULE A (not actively exercised).
    UNWIRED_CLI_FEATURE = (
        "Feature: F\n\n  @unwired\n  Scenario: deferred cli\n"
        "    # tracked: crap-rs#169 — fixture\n"
        "    When the operator runs `crap4rs --coverage lcov.info`\n    Then ok\n"
    )
    # False-positive guards (adversarial-review regression fixtures): a
    # backtick binary name in a comment, a `Then` assertion, a config
    # filename, or `Examples:` data is NOT a `When`-step narration and must
    # NOT trip RULE A on a lib-direct harness.
    COMMENT_FP_FEATURE = (
        "Feature: F\n\n  @wired\n  Scenario: comment aside\n"
        "    # the `crap4rs` docs recommend this\n"
        "    When the JSON is formatted\n    Then ok\n"
    )
    THEN_FP_FEATURE = (
        "Feature: F\n\n  @wired\n  Scenario: then assertion\n"
        "    When the JSON is formatted\n    Then `crap4rs` exits 0\n"
    )
    CONFIG_FP_FEATURE = (
        "Feature: F\n\n  @wired\n  Scenario: config file\n"
        "    When the config `crap4ts.toml` is parsed\n    Then ok\n"
    )
    EXAMPLES_FP_FEATURE = (
        "Feature: F\n\n  @wired\n  Scenario Outline: outline\n"
        "    When config is <cfg>\n    Then ok\n\n"
        "  Examples:\n      | cfg |\n      | `crap4rs` mode |\n"
    )

    def optout(valid: bool) -> str:
        line = (
            "// bdd-lint: lib-direct-by-design — tracked: crap-rs#999 — deliberate adapter test\n"
            if valid
            else "// bdd-lint: lib-direct-by-design (no tracked issue)\n"
        )
        return line + LIB_HARNESS

    # Two run_and_exit calls → RULE B ambiguous.
    AMBIGUOUS_HARNESS = (
        "fn main() {\n"
        '    World::cucumber().run_and_exit("tests/features/f.feature");\n'
        '    World::cucumber().run_and_exit("tests/features/g.feature");\n'
        "    let _ = OxcWalker::extract();\n"
        "}\n"
    )
    VALID_ASSERTS_ONLY = (
        "// bdd-asserts-only: crap-core::json::tests — tracked: crap-rs#169 — mirrors the unit suite\n"
        + SPAWN_HARNESS
    )

    cases: list[Case] = [
        ("ruleA-cli-narration-lib-harness-FAILS",
         LIB_HARNESS, CLI_FEATURE, "tests/features/f.feature", 1, "RULE A", ""),
        ("lib-narration-lib-harness-PASSES",
         LIB_HARNESS, LIB_FEATURE, "tests/features/f.feature", 0, "ok", ""),
        ("cli-narration-spawn-harness-PASSES",
         SPAWN_HARNESS, CLI_FEATURE, "tests/features/f.feature", 0, "ok", ""),
        ("unwired-cli-narration-lib-harness-PASSES",
         LIB_HARNESS, UNWIRED_CLI_FEATURE, "tests/features/f.feature", 0, "ok", ""),
        # FP guards (adversarial-review regression): a backtick binary in a
        # comment / Then step / config filename / Examples cell must NOT
        # fire RULE A on a lib-direct harness.
        ("fp-comment-backtick-binary-PASSES",
         LIB_HARNESS, COMMENT_FP_FEATURE, "tests/features/f.feature", 0, "ok", ""),
        ("fp-then-assertion-backtick-binary-PASSES",
         LIB_HARNESS, THEN_FP_FEATURE, "tests/features/f.feature", 0, "ok", ""),
        ("fp-config-filename-PASSES",
         LIB_HARNESS, CONFIG_FP_FEATURE, "tests/features/f.feature", 0, "ok", ""),
        ("fp-examples-table-data-PASSES",
         LIB_HARNESS, EXAMPLES_FP_FEATURE, "tests/features/f.feature", 0, "ok", ""),
        ("valid-optout-lib-narration-PASSES",
         optout(True), LIB_FEATURE, "tests/features/f.feature", 0, "ok", ""),
        ("valid-optout-cli-narration-COHERENCE-FAILS",
         optout(True), CLI_FEATURE, "tests/features/f.feature", 1, "RULE A-COHERENCE", ""),
        ("malformed-optout-FAILS",
         optout(False), LIB_FEATURE, "tests/features/f.feature", 1, "RULE A-OPTOUT", ""),
        ("ruleB-dangling-path-FAILS",
         LIB_HARNESS, LIB_FEATURE, "tests/features/nonexistent.feature", 1, "dangling", ""),
        ("ruleB-no-runexit-FAILS",
         "fn main() { let _ = OxcWalker::extract(); }\n", LIB_FEATURE, None, 1, "orphan", ""),
        ("ruleB-ambiguous-two-runexit-FAILS",
         AMBIGUOUS_HARNESS, LIB_FEATURE, None, 1, "ambiguous", ""),
        ("ruleD-malformed-asserts-only-WARNS",
         "// bdd-asserts-only: crap-core::json::tests (no tracked)\n" + SPAWN_HARNESS,
         CLI_FEATURE, "tests/features/f.feature", 0, "RULE D", ""),
        ("ruleD-asserts-only-without-qualified-path-WARNS",
         "// bdd-asserts-only: notqualified — tracked: crap-rs#169 — reason\n" + SPAWN_HARNESS,
         CLI_FEATURE, "tests/features/f.feature", 0, "RULE D", ""),
        ("ruleD-valid-asserts-only-PASSES-no-warn",
         VALID_ASSERTS_ONLY, CLI_FEATURE, "tests/features/f.feature", 0,
         "feature-link clean)", "RULE D"),
        ("ruleB-rustfmt-wrapped-runexit-resolves",
         'fn main() {\n    World::cucumber().filter_run_and_exit(\n'
         '        "__PATH__",\n        |_, _, _| true,\n    );\n'
         '    Command::new(env!("CARGO_BIN_EXE_crap4rs")).output();\n}\n',
         CLI_FEATURE, "tests/features/f.feature", 0, "ok", ""),
    ]

    failures = 0
    for name, harness_src, feature_src, path, expected_exit, expected_substr, forbidden in cases:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            tests = root / "crates" / "fixture" / "tests"
            (tests / "features").mkdir(parents=True)
            (tests / "features" / "f.feature").write_text(feature_src, encoding="utf-8")
            harness_text = harness_src.replace("__PATH__", path) if path else harness_src
            (tests / f"{name}_cucumber.rs").write_text(harness_text, encoding="utf-8")
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
                got = lint(root)
            out = buf.getvalue()
            ok = (
                got == expected_exit
                and expected_substr in out
                and (not forbidden or forbidden not in out)
            )
            if not ok:
                print(
                    f"bdd-mislevel-lint --self-test: FAIL {name}: "
                    f"expected exit {expected_exit} + substr {expected_substr!r}"
                    + (f" + NOT {forbidden!r}" if forbidden else "")
                    + f", got exit {got}",
                    file=sys.stderr,
                )
                print("Captured output:\n" + out, file=sys.stderr)
                failures += 1

    if failures:
        print(
            f"bdd-mislevel-lint --self-test: {failures} case(s) failed",
            file=sys.stderr,
        )
        return 1
    print(f"bdd-mislevel-lint --self-test: ok ({len(cases)} cases passed)")
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        sys.exit(self_test())
    sys.exit(lint(Path(__file__).resolve().parent.parent))
