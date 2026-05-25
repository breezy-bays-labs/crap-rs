#!/usr/bin/env bash
# Sync packages/crap4ts/package.json from the release-plz-bumped
# crates/crap4ts/Cargo.toml so the npm publish step in
# .github/workflows/release-plz.yml finds matching versions.
#
# Normally invoked by the `sync-crap4ts-package-json` job in
# .github/workflows/release-plz.yml: when release-plz opens or
# updates a release PR that bumps crap4ts's Cargo.toml, the job mints
# a GitHub App token, checks out the release PR's head ref, runs this
# script, and pushes the package.json bump back to the same branch so
# the publish gate's version-match check passes.
#
# Reviewers may also run it locally as an escape hatch if the
# automation is unavailable:
#
#   gh pr checkout <release-PR#>
#   ./scripts/sync-crap4ts-package-json.sh
#
# No-ops cleanly when both files already report the same version.
set -euo pipefail
# Extract crap4ts's Cargo.toml version structurally via `cargo metadata`
# (more robust than grep/sed against TOML).
CARGO_VER=$(cargo metadata --format-version 1 --no-deps \
  | jq -r '.packages[] | select(.name=="crap4ts") | .version')
PKG_VER=$(node -p "require('./packages/crap4ts/package.json').version")
if [ "$CARGO_VER" = "$PKG_VER" ]; then
  echo "already in sync at $CARGO_VER; no-op."
  exit 0
fi
npm version --no-git-tag-version --prefix packages/crap4ts "$CARGO_VER"
# Belt-and-braces guard against pushing an empty diff. The version
# compare above is the primary idempotency hook, but it's tree-state
# blind — a script edit that bypasses it (or a future ambiguity where
# `npm version` reports a different version than what's on disk) would
# fall through to the push without this. When the same workflow run
# can be re-triggered by its own push (App-token pushes do fire
# pull_request:synchronize), a no-op push wastes a CI cycle on every
# re-trigger. Using `HEAD --` compares against the last commit, so the
# guard catches the no-op case whether files are unstaged or already
# staged by a prior step.
if git diff --quiet HEAD -- \
     packages/crap4ts/package.json \
     packages/crap4ts/package-lock.json; then
  echo "no tree-level change vs HEAD at $CARGO_VER; nothing to commit."
  exit 0
fi
# Stage and commit only the files npm version touched, so a reviewer
# with unrelated edits in the working tree doesn't accidentally ship
# them. `npm version` updates package.json and (if present)
# package-lock.json; the lockfile is optional for crap4ts but staging
# both keeps them in sync whenever it exists.
files=(packages/crap4ts/package.json)
[ -f packages/crap4ts/package-lock.json ] && files+=(packages/crap4ts/package-lock.json)
git add -- "${files[@]}"
git commit -m "chore(crap4ts): sync package.json to v${CARGO_VER}" -- "${files[@]}"
git push
echo "package.json synced from $PKG_VER to $CARGO_VER and pushed."
