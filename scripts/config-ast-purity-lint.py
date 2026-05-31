#!/usr/bin/env python3
"""Config-AST-purity lint — mechanical guard for crap-rs#342.

`crates/crap-core/src/adapters/config.rs` is language-agnostic: config
discovery flows through `AdapterMeta::config_file_names`, so the loader
never needs to know any adapter's per-adapter config name. The ONE correct
home for the per-adapter literals (`crap4rs.toml` / `crap4ts.toml`) is each
adapter binary's `AdapterMeta` declaration (`crap4rs/src/main.rs`,
`crap4ts/src/main.rs`) — NOT crap-core. A per-adapter literal creeping into
config.rs (source or its inline tests) is a layering smell: it couples the
shared loader to a specific adapter and would have to be touched if a third
`*-core` adapter with its own config name were ever added.

This lint fails the build if config.rs contains any double-quoted
**per-adapter** literal:

  "crap4rs.toml"  "crap4ts.toml"  "crap4rs"  "crap4ts"

## Allowed (NOT flagged)

* `"crap.toml"` — the unified, language-neutral canonical name. It is
  shared by every adapter and is the right thing for the loader/tests to
  reference; it is not adapter-specific.
* `///` doc comments and `//` line comments that mention an adapter by
  name in prose. The gate matches only `"..."` STRING LITERALS; comments
  are stripped before matching. (config.rs's discovery rustdoc legitimately
  names `crap4rs.toml` / `crap4ts.toml` as examples of legacy fallbacks.)
* Bare language keys like `"rust"` / `"typescript"` (the `[language.*]`
  section names) — these are not adapter binary names.

## Scope

`crates/crap-core/src/adapters/config.rs` ONLY. The adapter binaries are
intentionally excluded — their `config_file_names: &["crap.toml",
"crap4rs.toml"]` declarations are the correct, only home for those literals.

## Limitations

* Block comments (`/* ... */`) are not stripped. config.rs uses only `//`
  and `///` comments; if a block comment is introduced and mentions a
  forbidden literal in prose, extend the stripper.
* String literals are matched textually, so a forbidden token assembled at
  runtime (`format!("crap4{}", "rs")`) is not caught. No such construction
  exists; closing it would need real parsing.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

TARGET = Path("crates/crap-core/src/adapters/config.rs")

# Per-adapter literals that must not appear as STRING LITERALS in config.rs.
# `"crap.toml"` is deliberately absent (the allowed unified name); none of
# these patterns match it.
FORBIDDEN = [
    '"crap4rs.toml"',
    '"crap4ts.toml"',
    '"crap4rs"',
    '"crap4ts"',
]


def strip_comments(text: str) -> str:
    """Drop `//`-to-EOL (covers `///` doc comments and `//` line comments)
    so an adapter name mentioned in prose does not count. Block comments
    are out of scope (see Limitations)."""
    return "\n".join(line.split("//", 1)[0] for line in text.splitlines())


def lint(repo_root: Path) -> int:
    target = repo_root / TARGET
    if not target.exists():
        print(f"config-ast-purity-lint: {TARGET} not found", file=sys.stderr)
        return 1

    raw = target.read_text()
    code = strip_comments(raw)
    code_lines = code.splitlines()

    errors = 0
    for lineno, line in enumerate(code_lines, start=1):
        for literal in FORBIDDEN:
            if literal in line:
                print(
                    f"\nconfig-ast-purity-lint: forbidden per-adapter literal "
                    f"{literal}\n"
                    f"  {TARGET}:{lineno}\n"
                    f"  crap-core's config adapter must stay name-agnostic "
                    f"(crap-rs#342).\n"
                    f"  Per-adapter config names belong in each binary's "
                    f"AdapterMeta\n"
                    f"  (crap4rs/src/main.rs, crap4ts/src/main.rs), not here.\n\n"
                    f"  Fix: use the unified `\"crap.toml\"`, or — in tests — a\n"
                    f"  synthetic name (discovery is name-agnostic, so a\n"
                    f"  synthetic canonical/legacy pair exercises the same path).\n",
                    file=sys.stderr,
                )
                errors += 1

    if errors:
        print(
            f"config-ast-purity-lint: {errors} forbidden per-adapter literal(s) "
            f"in {TARGET}; see above",
            file=sys.stderr,
        )
        return 1

    print(f"config-ast-purity-lint: ok ({TARGET} carries no per-adapter literals)")
    return 0


if __name__ == "__main__":
    sys.exit(lint(Path(__file__).resolve().parent.parent))
