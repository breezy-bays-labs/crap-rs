#!/usr/bin/env python3
"""Config-discovery isolation lint — mechanical guard for crap-rs#339 C-2.

#339 makes config discovery walk UPWARD from the run's `--src` anchor. That
is desired for the production scorecard (a subdir `--src` finds the repo-root
`crap.toml`, crap-rs#346) but HAZARDOUS for analyzer-shelling canary /
integration tests: any test that shells the adapter binary with an IN-REPO
`--src` (e.g. `../crap4rs/src`, `{manifest_dir}/src`) and NO `--config` now
climbs into the repo-root `crap.toml` and picks up its `threshold`/`metric`/
`exclude`/`title`. Its wire-envelope / parity / schema snapshot would then
shift on a SIBLING merge that edits root `crap.toml`, not on a deliberate
analyzer change — defeating the canary, the exact #224 silently-dead-gate
failure mode.

The fix is an explicit `--config <empty fixture>`, which short-circuits
discovery (cli/mod.rs `load_file_config`). This lint is that fix's enforcement
("documentation rots; CI doesn't"): it fails the build if any
analyzer-shelling test passes an in-repo `--src` without a covering `--config`.

## What counts as an adapter invocation

Two shapes, both detected:

  * `Command::cargo_bin("crap4rs"|"crap4ts")` ...
  * `let b = env!("CARGO_BIN_EXE_crap4rs"|"...crap4ts"); Command::new(b)...`
    (the binary is bound to a `BINARY`/`binary` const/let, then `Command::new`
    shells it — the cargo_bin-only shape would miss these, e.g.
    `scorecard_row_integration.rs`).

The builder CHAIN is aggregated from the invocation start to the terminal
`.output()` / `.assert()` — args are split across multiple `.args([...])` /
`.arg(...)` calls (e.g. `default_gate_threshold` puts `--config` in the first
array and a runtime `extra_args` in the middle).

## The hazard rule (structural, NOT crate-name based)

FLAG an adapter invocation iff ALL of:

  1. the chain has **no `--config`** argument, AND
  2. the command has **no `.current_dir(...)`** call (a `current_dir`
     re-roots relative `--src` into a tempdir → hermetic), AND
  3. the chain has a `--src` whose immediately-following value is a
     **string literal** (`"..."`) or a `format!(...)` — an in-repo path
     baked into the test source. A `--src` followed by a bare runtime
     variable (`.arg(&root)`, `--src", path`) is a tempdir path → exempt.

Keying on structure (not on `crap4`/`/src` content) also catches a future
bare `--src "src"` with no `current_dir` (which would resolve into the crate
dir and leak) and a `--src "../crap-core/src"` hazard a content rule misses.

## Limitations (documented, not chased — mirrors mutants-skip-lint.py)

* **Only the `.args([..., "--src", "literal", ...])` ARRAY form is
  matched.** `SRC_LITERAL_RE` keys on `"--src"` immediately followed (after
  a comma) by a string-literal / `format!`. The split-statement
  `.push("--src".into()); .push(<value>)` form (e.g.
  `github_annotations_cucumber.rs`) and the chained
  `.arg("--src").arg(<value>)` form are NOT matched, so an in-repo `--src`
  expressed that way is a false NEGATIVE. Both shapes were audited at
  introduction (crap-rs#339): every such invocation today either runs under
  `.current_dir(<tempdir>)` or passes a tempdir-derived `--src`, so the gap
  is currently inert. If a future test adds an in-repo `--src` via `.push`
  or chained `.arg`, broaden `SRC_LITERAL_RE` to cover those forms (the
  chain-window aggregation already spans them; only the regex is narrow).
* `.current_dir(<an in-repo dir>)` + literal `--src` + no `--config` is a
  false NEGATIVE. No test does this (it's a bizarre shape); closing it would
  need to distinguish in-repo from tempdir `current_dir` arguments, which
  needs runtime knowledge the lint can't have. Out of scope.
* Helper-mediated invocations where the bin name or `--src` is passed as a
  parameter are not traced. None present today.
* Brace/bracket tracking is line-comment-aware but not block-comment- or
  string-brace-aware. No current test exhibits the pattern.

If any gap produces a real regression, extend the lint.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Scan both crates whose tests shell the adapter binaries with in-repo
# `--src`. crap4ts/tests is intentionally out of scope: by convention its
# analyzer-shelling tests build a tempdir fixture and pass `.arg(&root)`
# (a runtime variable) — exempt by rule 3 anyway, and excluding the dir
# keeps the scan focused on where the hazard actually lives.
SCAN_DIRS = [
    Path("crates/crap-core/tests"),
    Path("crates/crap4rs/tests"),
]

# An adapter invocation begins at either marker.
CARGO_BIN_RE = re.compile(r'cargo_bin\(\s*"(?:crap4rs|crap4ts)"\s*,?\s*\)')
# `env!("CARGO_BIN_EXE_crap4rs")` bound to a name → `Command::new(name)`.
BIN_EXE_BIND_RE = re.compile(
    r'(?:const|let)\s+([A-Za-z_][A-Za-z_0-9]*)\s*(?::\s*&str\s*)?=\s*'
    r'env!\(\s*"CARGO_BIN_EXE_crap4(?:rs|ts)"\s*\)'
)
COMMAND_NEW_RE = re.compile(r'Command::new\(\s*([A-Za-z_][A-Za-z_0-9]*)\s*\)')

# A `--src` whose next token is a string literal or a format! macro is an
# in-repo path baked into the test source (the hazard). A `--src` followed
# by a bare identifier (`path`, `&root`) is a runtime tempdir path (exempt).
# `\s*` spans the newline rustfmt inserts between `"--src",` and its value.
SRC_LITERAL_RE = re.compile(r'"--src"\s*,\s*(?:"|&?format!\()')
HAS_CONFIG_RE = re.compile(r'"--config"')
HAS_CURRENT_DIR_RE = re.compile(r'\.current_dir\(')


def strip_line_comments(text: str) -> str:
    """Drop `//`-to-EOL on each line so a `--config` mentioned in a comment
    does not count as a real argument (and brace counting stays honest)."""
    out = []
    for line in text.splitlines():
        out.append(line.split("//", 1)[0])
    return "\n".join(out)


def chain_window(text: str, start: int) -> tuple[str, int]:
    """Return the builder-chain substring from `start` to the terminal
    `.output()`/`.assert()` (inclusive), plus its end offset. Falls back to
    the next `;` if no terminal is found."""
    terminal = re.compile(r'\.(?:output|assert)\(\)')
    m = terminal.search(text, start)
    if m:
        return text[start : m.end()], m.end()
    semi = text.find(";", start)
    end = semi if semi != -1 else len(text)
    return text[start:end], end


def find_invocations(text: str) -> list[int]:
    """Return start offsets of every adapter invocation in `text`.

    A `cargo_bin("crap4rs"|...)` match is itself the invocation. An
    `env!("CARGO_BIN_EXE_...")` binding records the bound name; each later
    `Command::new(<that name>)` is an invocation start."""
    starts: list[int] = []
    bound: set[str] = set()
    for m in BIN_EXE_BIND_RE.finditer(text):
        bound.add(m.group(1))
    for m in CARGO_BIN_RE.finditer(text):
        starts.append(m.start())
    if bound:
        for m in COMMAND_NEW_RE.finditer(text):
            if m.group(1) in bound:
                starts.append(m.start())
    return sorted(starts)


def lint_file(rs_file: Path, repo_root: Path) -> list[str]:
    raw = rs_file.read_text()
    text = strip_line_comments(raw)
    findings: list[str] = []
    for start in find_invocations(text):
        chain, _end = chain_window(text, start)
        if HAS_CONFIG_RE.search(chain):
            continue
        if HAS_CURRENT_DIR_RE.search(chain):
            continue
        if not SRC_LITERAL_RE.search(chain):
            continue
        lineno = text.count("\n", 0, start) + 1
        rel = rs_file.relative_to(repo_root)
        findings.append(
            f"\nconfig-discovery-lint: in-repo `--src` without `--config`\n"
            f"  {rel}:{lineno}\n"
            f"  This adapter invocation passes a string-literal/format! `--src`\n"
            f"  (an in-repo path) with no `--config` and no `.current_dir(...)`.\n"
            f"  Walk-upward discovery (crap-rs#339) will climb into the repo-root\n"
            f"  `crap.toml` (crap-rs#346) and the snapshot/parity output can shift\n"
            f"  on an unrelated sibling merge.\n\n"
            f"  Fix: pass an explicit empty `--config` fixture (e.g.\n"
            f"  `tests/fixtures/empty-config.toml`) to short-circuit discovery,\n"
            f"  OR run the command under `.current_dir(<tempdir>)` with a\n"
            f"  tempdir-relative `--src`. See scripts/config-discovery-lint.py.\n"
        )
    return findings


def lint(repo_root: Path) -> int:
    findings: list[str] = []
    for scan_dir in SCAN_DIRS:
        abs_dir = repo_root / scan_dir
        if not abs_dir.exists():
            continue
        for rs_file in sorted(abs_dir.rglob("*.rs")):
            findings.extend(lint_file(rs_file, repo_root))

    if findings:
        for f in findings:
            print(f, file=sys.stderr)
        print(
            f"config-discovery-lint: {len(findings)} unisolated in-repo `--src` "
            f"invocation(s); see above",
            file=sys.stderr,
        )
        return 1

    print("config-discovery-lint: ok (every in-repo `--src` adapter invocation "
          "passes `--config` or runs under a tempdir current_dir)")
    return 0


if __name__ == "__main__":
    sys.exit(lint(Path(__file__).resolve().parent.parent))
