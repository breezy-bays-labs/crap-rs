#!/usr/bin/env bash
# Coverage staleness forcing function for the crap-examples sample.
#
# WHY THIS EXISTS
#   crap-examples ships committed coverage fixtures (lcov.info,
#   coverage-final.json). The release pipeline turns those fixtures into
#   baseline "envelopes" that the scorecard report's Delta tab compares
#   against. If a contributor edits the sample SOURCE but forgets to
#   regenerate the committed fixture, the two drift apart: the published
#   baseline then describes coverage that no longer matches the code,
#   and every downstream consumer's Delta tab silently compares against
#   a stale baseline — with no error anywhere. This check is the forcing
#   function that surfaces that drift.
#
# WHY IT WARNS INSTEAD OF FAILING
#   On any base-ref problem the script exits early with a `::notice::`
#   (never a non-zero status), and even the drift case emits only a
#   `::warning::`. A false negative (missed drift) leaves a warning a
#   human can still act on; a false-positive hard-fail would wedge the
#   smoke job on a transient base-ref glitch. Decay-resistance beats
#   strictness here.
#
# SINGLE SOURCE OF TRUTH
#   This script is the one implementation, invoked from
#   .github/workflows/quick-start-smoke.yml and exercised across all
#   four branches by crates/crap-core/tests/coverage_staleness_check.rs.
#   Keeping the logic in one file (rather than inline workflow YAML)
#   lets the regression test guard it on every PR.
#
# INPUT
#   BASE_REF — the git ref to diff HEAD against. Read from the first
#   positional argument if given, else the $BASE_REF environment
#   variable (the workflow sets it from
#   `github.event.pull_request.base.sha || github.event.before`).
#
# BRANCHES (each emits a GitHub Actions workflow command)
#   * empty / all-zero SHA  -> ::notice:: skipped (no valid base ref)
#   * base unreachable      -> ::notice:: skipped (base <ref> unreachable)
#   * sample src changed,
#       fixture not regen'd  -> ::warning:: drift signal
#   * otherwise             -> silent (exit 0)
set -euo pipefail

BASE_REF="${1:-${BASE_REF:-}}"

# Null SHA (`0000…0`) is what `github.event.before` yields on a
# first-push / orphan-history branch; an empty value means the env var
# was never populated. Neither gives us a base to diff against.
if [ -z "$BASE_REF" ] || [ "$BASE_REF" = "0000000000000000000000000000000000000000" ]; then
  echo "::notice::coverage staleness check skipped (no valid base ref)"
  exit 0
fi

# A shallow checkout (or a force-push that orphaned the old tip) may not
# contain the base commit. Try a one-shot fetch, then re-check.
if ! git cat-file -e "${BASE_REF}^{commit}" 2>/dev/null; then
  git fetch --no-tags --depth=1 origin "$BASE_REF" 2>/dev/null || true
fi
if ! git cat-file -e "${BASE_REF}^{commit}" 2>/dev/null; then
  echo "::notice::coverage staleness check skipped (base $BASE_REF unreachable)"
  exit 0
fi

SRC_CHANGED=$(git diff --name-only "$BASE_REF"...HEAD | grep -E '^crates/crap-examples/(src|ts)/' || true)
COV_CHANGED=$(git diff --name-only "$BASE_REF"...HEAD | grep -E '^crates/crap-examples/(lcov\.info|coverage-final\.json)$' || true)
if [ -n "$SRC_CHANGED" ] && [ -z "$COV_CHANGED" ]; then
  echo "::warning::crap-examples source changed without coverage regen. See crates/crap-examples/README.md § Regenerating the fixtures."
fi
