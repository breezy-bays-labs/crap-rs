#!/usr/bin/env bash
#
# Build the crap4rs + crap4ts binaries, run each against the committed
# `crap-examples` corpus, and assert the resulting wire envelopes are
# well-formed: schema_version, the adapter's OWN language token, and a
# non-empty function set.
#
# SINGLE SOURCE OF TRUTH for envelope verification. Two callers share it:
#   - release-plz.yml `build-crap-examples-envelopes` — runs this, then
#     uploads the envelopes to every release page.
#   - ci.yml `envelope-verify` — runs this on EVERY PR (no upload) so a
#     malformed or mislabeled envelope fails BEFORE it can be published.
#
# Why one script: the rc.4 incident shipped `language: "rust"` for the
# TypeScript adapter because the authoritative assertion lived ONLY in
# the release job (post-publish) while the per-PR canary asserted the
# opposite value. Keeping a single copy of the assertions — run on both
# the PR path and the release path — makes that
# two-checks-asserting-contradictory-truths failure structurally
# impossible: the PR gate and the release gate are the same bytes.
#
# Outputs `crap4rs-envelope.json` + `crap4ts-envelope.json` in the
# current working directory (repo root) for the caller to upload or
# discard.
set -euo pipefail

echo "::group::Build adapter binaries"
cargo build --release --bin crap4rs --bin crap4ts
echo "::endgroup::"

echo "::group::Lint committed coverage fixtures for absolute paths"
# Absolute SF: paths in committed lcov.info (or absolute path keys in
# coverage-final.json) diverge between contributors and CI runners,
# silently producing envelopes with empty coverage maps. Fail loudly
# here rather than ship a malformed envelope.
#
# Fail-fast on a missing/empty/invalid fixture BEFORE the path lint:
# the grep and jq below run inside `if` conditions, where `set -e` does
# NOT propagate their non-zero exit. A missing lcov.info would read as
# "no absolute SF: paths found" and a missing/garbage coverage-final.json
# would read as "no absolute keys found" — either way the lint would
# falsely report OK and let a malformed envelope through. Guard each
# fixture's existence (and the JSON's parseability) up front.
if [ ! -s crates/crap-examples/lcov.info ]; then
  echo "::error::crates/crap-examples/lcov.info is missing or empty"
  exit 1
fi
if grep -qE '^SF:/' crates/crap-examples/lcov.info; then
  echo "::error::lcov.info contains absolute SF: paths — regen with the sed normalization step in crap-examples/README.md"
  exit 1
fi
if [ ! -s crates/crap-examples/coverage-final.json ]; then
  echo "::error::crates/crap-examples/coverage-final.json is missing or empty"
  exit 1
fi
if ! jq -e . crates/crap-examples/coverage-final.json >/dev/null 2>&1; then
  echo "::error::crates/crap-examples/coverage-final.json is not valid JSON"
  exit 1
fi
if jq -e 'to_entries | map(select(.key | startswith("/"))) | length > 0' crates/crap-examples/coverage-final.json >/dev/null; then
  echo "::error::coverage-final.json contains absolute path keys — regen with the jq normalization step in crap-examples/README.md"
  exit 1
fi
echo "OK: both fixtures use --src-relative paths"
echo "::endgroup::"

echo "::group::crap4rs envelope (Rust modules)"
# Leading `./` on --src — the adapter's coverage-key matcher joins SF:
# paths onto --src before lookup, and bare-relative --src produces 0%
# coverage for workspace-relative SF: paths in the committed fixture
# (tracked: crap-rs#331).
./target/release/crap4rs \
  --src ./crates/crap-examples/src \
  --coverage crates/crap-examples/lcov.info \
  --format json --no-fail \
  > crap4rs-envelope.json
# Empty-file guard: a silent adapter failure could write an empty (or
# non-JSON) file; jq's raw parse error would otherwise obscure the cause.
[ -s crap4rs-envelope.json ] || { echo "::error::crap4rs-envelope.json is empty — adapter produced no output"; exit 1; }
# Well-formedness AND semantic validity. The `.scored.crap.value >= 1.0`
# and non-empty `.scored.identity.file_path` assertions catch a corrupt
# envelope (CRAP's formula floor is 1.0; a 0/negative value or a blank
# path signals a broken adapter or a coverage-join gone wrong) before it
# can be published as a baseline. NOTE the shape: `crap` is an object
# `{risk_level, value}` (a naive `.crap >= 1.0` would compare object-to-
# number, always true in jq), and the path key is `scored.identity.file_path`
# (not a top-level `.path`) — both verified against the committed
# wire_envelope_crap4rs snapshot.
jq -e '
  .schema_version == 2
  and .language == "rust"
  and (.result.functions | length) > 0
  and (.result.summary.total_functions // 0) > 0
  and all(.result.functions[].scored.crap.value; type == "number" and . >= 1.0)
  and all(.result.functions[].scored.identity.file_path; type == "string" and . != "")
' crap4rs-envelope.json >/dev/null || {
  echo "::error::crap4rs-envelope.json failed verification — expected schema_version==2, language==rust, total_functions>0, every scored.crap.value a number >= 1.0, every scored.identity.file_path a non-empty string"
  exit 1
}
echo "::endgroup::"

echo "::group::crap4ts envelope (TypeScript modules)"
# --exclude '*.test.ts' so the pedagogy doesn't include the test files
# that share the ts/ directory. Leading `./` on --src as above.
./target/release/crap4ts \
  --src ./crates/crap-examples/ts \
  --coverage crates/crap-examples/coverage-final.json \
  --format json --no-fail \
  --exclude '*.test.ts' \
  > crap4ts-envelope.json
[ -s crap4ts-envelope.json ] || { echo "::error::crap4ts-envelope.json is empty — adapter produced no output"; exit 1; }
# Same semantic-validity assertions as the crap4rs envelope above — the
# wire shape is shared crap-core (scored.crap.value / scored.identity.file_path),
# so a corrupt TypeScript envelope is caught identically.
jq -e '
  .schema_version == 2
  and .language == "typescript"
  and (.result.functions | length) > 0
  and (.result.summary.total_functions // 0) > 0
  and all(.result.functions[].scored.crap.value; type == "number" and . >= 1.0)
  and all(.result.functions[].scored.identity.file_path; type == "string" and . != "")
' crap4ts-envelope.json >/dev/null || {
  echo "::error::crap4ts-envelope.json failed verification — expected schema_version==2, language==typescript, total_functions>0, every scored.crap.value a number >= 1.0, every scored.identity.file_path a non-empty string"
  exit 1
}
echo "::endgroup::"

echo "✅ both envelopes built + verified (schema_version=2, correct per-adapter language, non-empty function set)"
