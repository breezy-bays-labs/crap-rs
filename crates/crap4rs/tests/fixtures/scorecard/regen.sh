#!/usr/bin/env bash
# Regenerate the vendored mokumo scorecard schema fixture.
#
# Copies `~/Github/mokumo/.config/scorecard/schema.json` to this
# directory and rewrites the "Vendored at commit" pin in `SOURCE.md`
# with the latest commit SHA touching the schema. After running, inspect
# the diff — a bumped `schema_version` likely requires reporter or
# integration-test updates and should land in a paired PR.

set -euo pipefail

MOKUMO_REPO="${MOKUMO_REPO:-$HOME/Github/mokumo}"
THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ ! -d "$MOKUMO_REPO" ]]; then
  echo "error: mokumo checkout not found at $MOKUMO_REPO" >&2
  echo "Set MOKUMO_REPO to override." >&2
  exit 1
fi

SCHEMA_SRC="$MOKUMO_REPO/.config/scorecard/schema.json"
if [[ ! -f "$SCHEMA_SRC" ]]; then
  echo "error: schema not found at $SCHEMA_SRC" >&2
  exit 1
fi

cp "$SCHEMA_SRC" "$THIS_DIR/schema.json"

PINNED_SHA="$(git -C "$MOKUMO_REPO" log --oneline -1 --format=%h .config/scorecard/schema.json)"
PINNED_VERSION="$(grep -oE '"schema_version"\s*:\s*[0-9]+' "$THIS_DIR/schema.json" | head -1 | grep -oE '[0-9]+$' || true)"

if [[ -n "$PINNED_VERSION" ]]; then
  echo "info: vendored schema_version=$PINNED_VERSION at commit $PINNED_SHA"
fi

# Best-effort SOURCE.md pin update — only if the line shape matches.
if grep -q '| Vendored at commit |' "$THIS_DIR/SOURCE.md"; then
  sed -i.bak \
    "s|^| Vendored at commit |.*$|| Vendored at commit | \`$PINNED_SHA\` (regenerated $(date -u +%Y-%m-%d)) ||" \
    "$THIS_DIR/SOURCE.md" && rm "$THIS_DIR/SOURCE.md.bak"
fi

echo "done. inspect:"
echo "  git -C \"$THIS_DIR/../..\" diff -- tests/fixtures/scorecard/"
