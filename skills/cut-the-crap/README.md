# /cut-the-crap

Reference Claude Code skill for crap4rs. Drives over-threshold functions below the CRAP gate by consuming `crap4rs --format advice`.

## Install

```bash
git clone https://github.com/breezy-bays-labs/crap-rs
cp -r crap-rs/skills/cut-the-crap ~/.claude/skills/
```

Or, if you already have the repo checked out, run `cp -r skills/cut-the-crap ~/.claude/skills/` from the repo root.

The skill is then invocable as `/cut-the-crap` from any Claude Code session — no restart required (skills are auto-discovered from `~/.claude/skills/`).

## Usage

```text
/cut-the-crap                       # cover-then-split, apply changes
/cut-the-crap --explain-only        # produce plan, do not modify
/cut-the-crap --threshold 15        # custom CRAP threshold
/cut-the-crap --src crates/foo/src  # custom source root
```

Prerequisites:

- `crap4rs` on PATH (`cargo install crap4rs` or `cargo binstall crap4rs`).
- An existing `lcov.info`. Generate one with `cargo llvm-cov --lcov --output-path lcov.info` if missing.

## What it does

1. Runs `crap4rs --format advice` and reads the per-function `Diagnostic`.
2. For each over-threshold function, applies the cover-then-split heuristic:
   - `root_cause: low_coverage` → write tests, re-evaluate.
   - `root_cause: high_complexity` → name + apply the recommended `ProposedSplit`.
   - `root_cause: both` → cover first, re-evaluate, then split if still over.
3. Writes a structured plan to `tmp/cut-the-crap-plan.md` before applying.
4. Re-runs the analyzer to confirm the gate is green.

See `SKILL.md` for the full process specification.

## Closes

- crap4rs#77

Depends on crap4rs#76 (`--format advice` JSON shape).
