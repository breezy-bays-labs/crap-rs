#!/usr/bin/env bash
# Sync packages/crap4ts/package.json from the release-plz-bumped
# crates/crap4ts/Cargo.toml so the npm publish step in
# .github/workflows/release-plz.yml finds matching versions.
#
# Run this on every auto-opened release PR that includes a crap4ts
# Cargo.toml bump, before merging:
#
#   gh pr checkout <release-PR#>
#   ./scripts/sync-crap4ts-package-json.sh
#
# No-ops cleanly when both files already report the same version.
# Until a GitHub-App-driven release-plz identity ships (so that
# release-plz-opened PRs can fire downstream workflows on push), this
# manual step is the published v0 sync path.
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
git add packages/crap4ts/package.json
git commit -m "chore(crap4ts): sync package.json to v${CARGO_VER}"
git push
echo "package.json synced from $PKG_VER to $CARGO_VER and pushed."
