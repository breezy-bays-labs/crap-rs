#!/usr/bin/env bash
# Leaf-scope drift check: assert this repo's name is present in the
# org-wide release-plz App allowlist (the canonical TOML in
# breezy-bays-labs/.github). Run as a CI lint so a PR that drops this
# repo's name from the allowlist (or a release-plz workflow change in
# this repo that orphans it) fails loudly instead of silently breaking
# the next release-plz token mint.
#
# Scope: this repo's name only. It does NOT compare the canonical
# allowlist against the App's actual installations — that's an
# org-global check, requires an Org Admin token, and lives in a
# separate centralized cron in the .github repo (see follow-up).
#
# Dependencies (installed by the CI workflow): dasel, jq, gh.
# Token: secrets.GITHUB_TOKEN (the repo-scoped default is sufficient
# to read public contents from a sibling public repo via the
# contents API).

set -euo pipefail

THIS_REPO="crap-rs"
ALLOWLIST_REPO="breezy-bays-labs/.github"
ALLOWLIST_PATH="release-plz-allowlist.toml"
TMP_TOML="$(mktemp -t release-plz-allowlist.XXXXXX.toml)"
trap 'rm -f "$TMP_TOML"' EXIT

# Fetch the raw TOML in one round-trip. `Accept: application/vnd.github.raw`
# returns the file body verbatim — no base64 decode step required, which
# also avoids the GNU coreutils (`base64 -d`) vs BSD (`base64 -D`)
# portability split that bites local debugging on macOS.
gh api \
  "repos/${ALLOWLIST_REPO}/contents/${ALLOWLIST_PATH}" \
  -H "Accept: application/vnd.github.raw" \
  > "$TMP_TOML"

# Parse `repos = [...]` via dasel (TOML-aware) then jq for array
# iteration. The `.[]?` form tolerates a missing-or-empty array
# without throwing — that keeps the failure path informative (we
# still hit the "${THIS_REPO} missing" branch with an empty list)
# instead of bailing out with a parse error before the actionable
# message can fire.
mapfile -t CANONICAL < <(
  dasel -f "$TMP_TOML" -r toml -w json '.repos' \
    | jq -r '.[]?' \
    | sort
)

# Search the array literally (avoid bash regex / glob interpretation
# of `THIS_REPO`).
for repo in "${CANONICAL[@]}"; do
  if [ "$repo" = "$THIS_REPO" ]; then
    echo "OK: ${THIS_REPO} present in canonical allowlist"
    echo "(canonical list: ${CANONICAL[*]:-<empty>})"
    exit 0
  fi
done

# Format a stable, paste-friendly list for the error message, even
# when CANONICAL is empty (which would otherwise render as a blank
# placeholder that reads as a tooling bug rather than as drift).
if [ "${#CANONICAL[@]}" -eq 0 ]; then
  CANONICAL_REPR="<empty array — TOML may have repos = [] or be malformed>"
else
  CANONICAL_REPR="${CANONICAL[*]}"
fi

echo "::error::release-plz allowlist drift — '${THIS_REPO}' is missing from ${ALLOWLIST_REPO}/${ALLOWLIST_PATH}."
echo "  canonical list: [${CANONICAL_REPR}]"
echo "  fix: open a PR on ${ALLOWLIST_REPO} appending '${THIS_REPO}' to the repos array, land it, then re-run this check."
exit 1
