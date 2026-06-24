#!/usr/bin/env bash
#
# Verify the crap4ts npm package installs cleanly on THIS runner's native
# platform and exposes the `analyze` export — the os/cpu/libc install-gating
# smoke that a path-`require()` of the .node directly cannot exercise.
#
# SINGLE SOURCE OF TRUTH for npm packaging verification. Two callers share it:
#   - release-plz.yml `build-crap4ts-cdylib` (post-merge, per-platform
#     matrix) — runs this after building + renaming the platform cdylib,
#     before uploading the artifact that becomes the published package.
#   - ci.yml `verify-npm-packaging` (the release-plz PR, native platform) —
#     runs this BEFORE the irreversible release-PR merge, so a packaging
#     defect (e.g. a stray `libc` field that makes `npm install` reject the
#     platform with EBADPLATFORM, as crap4ts 2.0.0-rc.1 shipped) fails LEFT
#     of publish instead of reaching the registry.
#
# Precondition: the platform-native .node addon is already built and copied
# into packages/crap4ts/ (the caller does `cargo build --features
# napi-binding` + the rename). This script only packs + installs + smoke-
# tests; it does not build.
#
# Why one script: keeping the pack/install/export assertions in a single
# file run on BOTH the pre-merge and post-merge paths makes them
# byte-identical — the same shift-left discipline as
# scripts/build-and-verify-example-envelopes.sh (a release-critical gate
# must not live only in the publish job).
set -euo pipefail

pkg_dir="${1:-packages/crap4ts}"

echo "::group::npm pack + install-gating ($pkg_dir)"
work="$(mktemp -d)"
( cd "$pkg_dir" && npm pack --pack-destination "$work" )
tarball="$(ls "$work"/crap4ts-*.tgz)"
echo "packed: $(basename "$tarball")"

consumer="$work/consumer"
mkdir -p "$consumer"
cd "$consumer"
npm init -y >/dev/null
# `npm install` exits non-zero on EBADPLATFORM; the explicit node_modules
# check is a backstop so the failure stays loud even if a future npm
# softens platform mismatch to a warning.
npm install --no-audit --no-fund "$tarball"
if [ ! -d node_modules/crap4ts ]; then
  echo "::error::npm did not install crap4ts — os/cpu/libc gating rejected this platform (see EBADPLATFORM above). Packaging defect."
  exit 1
fi
node -e 'const m = require("crap4ts"); if (typeof m.analyze !== "function") { throw new Error("crap4ts installed from the packed tarball but the analyze export is missing"); } console.log("OK: npm install-gating passed and analyze export present");'
echo "::endgroup::"
