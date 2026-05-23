#!/usr/bin/env python3
"""Mutants-skip lint — mechanical enforcement of the carry-forward rule that
every test which shells `cargo_bin("crap4rs"|"crap4ts")` under a scoped
`cargo mutants` gate that does NOT build that bin must carry a `--skip`
substring token in `.cargo/mutants.toml`'s `additional_cargo_test_args`.

The *why* lives in the top-of-file comment of `.cargo/mutants.toml`. This
script is the *what*: replaces the documented rule (which #224 root-caused as
silently violated by a new test that landed without the token) with a check
CI and lefthook run on every push.

Scope-by-directory (mirrors the two scoped mutants gates):

  * `crates/crap-core/tests/**` is scanned under `--package crap-core`
    (the `view.rs` per-PR gate). Neither `crap4rs` nor `crap4ts` bin is
    built — both literal calls require a `--skip`.
  * `crates/crap4ts/tests/**` is scanned under `--package crap4ts` (the
    walker per-merge gate). The `crap4ts` bin IS built, so only literal
    `cargo_bin("crap4rs")` calls require a `--skip`.

Limitations (all verified empirically against the current codebase):

* **Helper fns silently skipped.** Literals nested inside helper fns
  (e.g. `fn crap4rs_threshold(...) { Command::cargo_bin("crap4rs")... }`
  in `default_gate_threshold.rs`) are silently skipped — those helpers
  are dangerous only if they're called from a non-`--skip`'d test fn,
  and that requires call-graph analysis the lint does not perform. The
  pattern in scope today is "helper called exclusively from `--skip`'d
  tests" (the `crap4{rs,ts}_threshold` helpers in
  `default_gate_threshold.rs` are called only from `default_gate_*`
  tests covered by `--skip default_gate`).
* **Parameterised helpers not detected.** Helper-mediated calls with
  variable bin names (`run_bin("crap4rs", ...)` → internal
  `cargo_bin(name)`) are not detected. None present today.
* **Sync `#[test]` only.** `#[tokio::test]` / `#[async_std::test]` /
  other test-runner attributes are not detected by `TEST_ATTR_RE`.
  None present today; if introduced, extend the regex.
* **Brace tracking is line-comment-aware but not string- or
  block-comment-aware.** `//` line comments are stripped before
  brace counting, but `/* { */` block comments and `{` inside
  string literals can theoretically skew the fn range. No current
  test file exhibits the pattern.

If any of these gaps produces a real regression, extend the lint.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

SCOPES: list[tuple[Path, str, frozenset[str]]] = [
    (Path("crates/crap-core/tests"), "crap-core", frozenset({"crap4rs", "crap4ts"})),
    (Path("crates/crap4ts/tests"), "crap4ts", frozenset({"crap4rs"})),
]

# DOTALL on CARGO_BIN_RE allows the literal to span lines — rustfmt
# can wrap `cargo_bin("crap4rs")` across newlines if the surrounding
# call gets long, often with a trailing comma:
#   cargo_bin(
#       "crap4rs",
#   )
# The line-by-line scan would have missed those. Trailing comma is
# optional so both the formatted-wide and rustfmt-wrapped shapes match.
CARGO_BIN_RE = re.compile(r'cargo_bin\(\s*"(crap4rs|crap4ts)"\s*,?\s*\)', re.DOTALL)
TEST_ATTR_RE = re.compile(r"^\s*#\[test\]\s*$")
FN_DECL_RE = re.compile(r"^\s*fn\s+([a-zA-Z_][a-zA-Z_0-9]*)\s*[(<]")


def extract_skip_tokens(mutants_toml: Path) -> list[str]:
    # Anchor at start-of-line (MULTILINE) so a commented-out
    # `# additional_cargo_test_args = [...]` line does not match.
    body_match = re.search(
        r"^additional_cargo_test_args\s*=\s*\[(.*?)\]",
        mutants_toml.read_text(),
        re.DOTALL | re.MULTILINE,
    )
    if not body_match:
        return []
    strings = re.findall(r'"([^"]+)"', body_match.group(1))
    # Reject any `-`-prefixed string so unrelated cargo-test flags
    # (e.g. `--nocapture`, `--test-threads=1`) cannot accidentally
    # be treated as `--skip` substring tokens.
    return [s for s in strings if not s.startswith("-")]


def find_test_fn_ranges(lines: list[str]) -> list[tuple[str, int, int]]:
    """Return [(fn_name, start_line, end_line)] for every #[test] fn in the
    file. Line numbers are 0-indexed and `end_line` is inclusive (the line
    containing the matching outer `}`)."""
    ranges: list[tuple[str, int, int]] = []
    n = len(lines)
    i = 0
    while i < n:
        if TEST_ATTR_RE.match(lines[i]):
            j = i + 1
            while j < n:
                line = lines[j]
                stripped = line.strip()
                if not stripped or stripped.startswith("//") or stripped.startswith("#"):
                    j += 1
                    continue
                break
            if j < n:
                fn_match = FN_DECL_RE.match(lines[j])
                if fn_match:
                    fn_name = fn_match.group(1)
                    depth = 0
                    seen = False
                    end = j
                    for k in range(j, n):
                        # Strip `//` line comments before counting — a
                        # comment containing `{` or `}` would otherwise
                        # skew fn-range detection. Block comments and
                        # string-literal braces are out of scope (see
                        # module docstring "Limitations").
                        for c in lines[k].split("//", 1)[0]:
                            if c == "{":
                                depth += 1
                                seen = True
                            elif c == "}":
                                depth -= 1
                        if seen and depth == 0:
                            end = k
                            break
                    ranges.append((fn_name, j, end))
                    i = end + 1
                    continue
        i += 1
    return ranges


def is_covered(fn_name: str | None, tokens: list[str]) -> bool:
    return fn_name is not None and any(tok in fn_name for tok in tokens)


def lint(repo_root: Path) -> int:
    mutants_toml = repo_root / ".cargo" / "mutants.toml"
    if not mutants_toml.exists():
        print(f"mutants-skip-lint: {mutants_toml} not found", file=sys.stderr)
        return 1

    tokens = extract_skip_tokens(mutants_toml)
    if not tokens:
        print(
            "mutants-skip-lint: no --skip tokens in additional_cargo_test_args",
            file=sys.stderr,
        )
        return 1

    errors = 0
    for test_dir, scope, gate_bins in SCOPES:
        abs_dir = repo_root / test_dir
        if not abs_dir.exists():
            continue
        for rs_file in sorted(abs_dir.rglob("*.rs")):
            text = rs_file.read_text()
            lines = text.splitlines()
            ranges = find_test_fn_ranges(lines)
            # Scan whole-file text so the regex can match a
            # `cargo_bin(...)` call split across lines by rustfmt
            # (CARGO_BIN_RE is DOTALL). Recompute the line number
            # from the match's character offset.
            for m in CARGO_BIN_RE.finditer(text):
                bin_name = m.group(1)
                if bin_name not in gate_bins:
                    continue
                lineno0 = text.count("\n", 0, m.start())
                fn_name = next(
                    (name for name, s, e in ranges if s <= lineno0 <= e),
                    None,
                )
                if fn_name is None:
                    # Literal in helper fn — see "Limitations" in module
                    # docstring. Out of scope; silently skip.
                    continue
                if is_covered(fn_name, tokens):
                    continue
                rel = rs_file.relative_to(repo_root)
                print(
                    f"\nmutants-skip-lint: uncovered adapter-bin shelling test\n"
                    f"  {rel}:{lineno0 + 1}  fn {fn_name}\n"
                    f'  shells cargo_bin("{bin_name}") under '
                    f"`--package {scope}` mutants scope\n"
                    f"  but `{fn_name}` is not covered by any --skip substring in\n"
                    f"  .cargo/mutants.toml's additional_cargo_test_args.\n\n"
                    f"  Fix: add a substring of `{fn_name}` (or the literal name) to\n"
                    f"  that array. See the explanatory comment at the top of\n"
                    f"  .cargo/mutants.toml for why.\n",
                    file=sys.stderr,
                )
                errors += 1

    if errors > 0:
        print(
            f"mutants-skip-lint: {errors} uncovered test fn(s); see above",
            file=sys.stderr,
        )
        return 1

    print(
        f"mutants-skip-lint: ok ({len(tokens)} --skip token(s) cover all "
        f"adapter-bin shelling tests in scope)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(lint(Path(__file__).resolve().parent.parent))
