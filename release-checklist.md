# Release Checklist

Operational steps for cutting a `crap4rs` / `crap-core` release from this
workspace. The checklist encodes two load-bearing rules that surfaced
during prior cuts:

1. **Publish order is fixed** — `crap-core` must propagate to crates.io
   before `crap4rs` can publish (the latter resolves
   `crap-core = { workspace = true }` against crates.io, not the local
   workspace).
2. **The tag points at the commit with FINAL metadata** — `cargo publish`
   reads `Cargo.toml` from the tagged commit; whatever `repository` URL,
   `version`, and other manifest fields are present at that commit are
   what land on crates.io permanently. If the release also renames the
   repo or updates URLs, the tag goes on the post-rename URL-update
   commit, NOT the release PR's merge commit (see auto-memory
   `release_tag_merge_commit`).

`crap4ts` is `"private": true` (npm) and release-publishing for it is
disabled — it is never published from this checklist. Real walker work
will revisit publish strategy in a future pipeline.

## Re-export audit gate (before every minor release)

Before bumping a minor version, audit
`crates/crap4rs/src/lib.rs` and
`crates/crap4rs/src/adapters/mod.rs` end-to-end. For every re-exported
symbol, record one of three dispositions:

- **stable** — the symbol stays as-is through the next major.
- **narrowing** — the symbol will be removed or renamed at the next
  major (typically because consumers should migrate to `crap_core::*`).
- **alias-dropping** — a v0.4 / earlier name that aliases a renamed
  v0.5+ symbol; the alias drops at the next major.

The full audit table for the v0.5.0 release lives in the PR body for
[crap4rs#156](https://github.com/breezy-bays-labs/crap4rs/pull/156)
(which closes [#138](https://github.com/breezy-bays-labs/crap4rs/issues/138))
— copy that format for future audits. The audit is a CAO C2
future-proofing gate; library consumers read the resulting table to
plan their migration ahead of the next major.

## Standard release sequence (no rename)

For releases that do NOT involve renaming the repo or updating any
`Cargo.toml` `repository` URL:

```bash
# 0. Open a release PR with the version bump + CHANGELOG entry.
# Wait for review + merge to main.

# 1. Sync local main to the merge commit.
git fetch origin
git checkout main
git pull --ff-only

# 2. Verify the merge commit has the bumped version.
git show HEAD:crates/crap4rs/Cargo.toml | rg '^version'
# expect: version = "X.Y.Z"

# 3. Tag the merge commit.
git tag vX.Y.Z origin/main
git push origin vX.Y.Z

# 4. Publish in fixed order.
cargo publish -p crap-core
# wait ~30s for crates.io propagation
cargo publish -p crap4rs
# do NOT publish crap4ts (private)

# 5. Verify GitHub release.yml ran on the tag push and binstall
# artifacts uploaded successfully.
gh release view vX.Y.Z --repo breezy-bays-labs/<repo>
```

## Release-with-rename sequence (v0.5.0 case)

When the release also renames the repo or updates any `repository` URL
in `Cargo.toml`, the tag MUST go on the post-rename URL-update commit,
not on the release PR's merge commit. Otherwise crates.io permanently
publishes manifests pointing at the old URL.

```bash
# 0. Release PR (this PR for v0.5.0) merges to main. Cargo.toml
# repository URLs still point at the OLD repo at this point.

# 1. Rename via GitHub UI:
#    Settings -> Repository name -> breezy-bays-labs/<new-name>
# GitHub publishes an HTTP redirect that carries old URLs for
# >= 1 year per GitHub support policy.

# 2. Update local working directory.
mv ~/github/<old-name> ~/github/<new-name>
cd ~/github/<new-name>
# Update .cargo/config.toml if target-dir is an absolute path.
git remote set-url origin git@github.com:breezy-bays-labs/<new-name>.git

# 3. Push a follow-up commit on main that updates every
# repository URL.
git checkout main
git pull --ff-only
# Edit:
#   - Cargo.toml (workspace root) -> repository
#   - crates/<each>/Cargo.toml if repository is overridden (most use
#     repository.workspace = true; one workspace-root edit suffices)
#   - README.md badges + links pointing at github.com/breezy-bays-labs/<old-name>
#   - .github/actions/scorecard/action.yml or other internal Action refs
#     that reference the repo by name
# Confirm:
git grep -n 'breezy-bays-labs/<old-name>'
# expect: no matches
git add -p   # review every URL change
git commit -m "chore(workspace): update repository URLs after rename"
git push origin main

# 4. Verify the URL-update commit before tagging.
git show HEAD:Cargo.toml | rg '^repository'
git show HEAD:crates/crap4rs/Cargo.toml | rg '^version|^repository'
# expect (for v0.5.0):
#   repository = "https://github.com/breezy-bays-labs/<new-name>"
#   version    = "0.5.0"

# 5. Tag THIS commit (the URL-update commit), NOT the release PR merge.
git tag v0.5.0 origin/main
git push origin v0.5.0

# 6. Publish in fixed order.
cargo publish -p crap-core
# wait ~30s for crates.io propagation
cargo publish -p crap4rs
# do NOT publish crap4ts (private; alpha shell; no real walker yet)

# 7. Verify release.yml ran on tag push.
gh release view v0.5.0 --repo breezy-bays-labs/<new-name>
```

## Version bump locations

For any release, edit the version in these places (use this list as a
grep target):

- `crates/crap4rs/Cargo.toml` — `version = "X.Y.Z"`
- `crates/crap-core/Cargo.toml` — only if `crap-core` itself is being
  released (semver-tied to `crap4rs` is one option, but currently
  independent).
- `crates/crap4ts/Cargo.toml` — only if the TS adapter graduates from
  alpha; `2.0.0-alpha.N` bumps stay private until publish strategy is
  revisited.

Cargo workspace inheritance is per-field, not per-crate, so each crate
keeps its own `version`. Do not attempt to centralize it under
`[workspace.package]` — the three crates version independently.

## Why the order matters

- **`cargo publish -p crap-core` first**: `crap4rs/Cargo.toml` has
  `crap-core = { workspace = true }` which resolves through the
  workspace root. The workspace root pins
  `crap-core = { path = "crates/crap-core", version = "0.1.0" }`. When
  `crap4rs` publishes, cargo strips the `path = ` field and ships only
  `version = "0.1.0"` — crates.io then resolves that against the
  published registry, NOT the local workspace. If `crap-core` isn't
  live on crates.io at publish time, `crap4rs`'s publish fails with
  "no matching package named `crap-core`."
- **~30s propagation wait**: crates.io's index updates eventually-
  consistent. Empirically a ~30s wait after `cargo publish -p crap-core`
  is sufficient; longer is fine. If `cargo publish -p crap4rs` errors
  with "no matching package," wait another 30s and retry.
- **Tag-the-final-metadata commit**: `cargo publish` packages
  `Cargo.toml` from the tagged commit (technically: from `git HEAD`
  during the publish invocation, but the tag is what gates the publish
  workflow). If the tagged commit has stale `repository` URLs, those
  URLs go on crates.io's package page permanently — they are not
  edit-able post-publish. The repo-rename ceremony above is the
  canonical example; the same rule applies to any in-flight
  manifest-field change at release time.

## Cross-references

- The tag-ordering rule has a documented origin: the v0.4.0 release on
  2026-05-04, where the `v0.4.0` tag landed at the last commit before
  PR [#127](https://github.com/breezy-bays-labs/crap4rs/pull/127)
  merged (Cargo.toml = 0.3.0 at the tagged commit). The release
  workflow built 0.3.0 binaries labelled as v0.4.0, uploaded them to
  the v0.4.0 GitHub release, and `cargo publish` errored with
  "version already exists." Recovery: delete release + tag, re-tag the
  merge commit, push tag, workflow re-fires correctly. Encoded in the
  "Standard release sequence" and "Release-with-rename sequence"
  blocks above so future cuts don't repeat it.
- `CHANGELOG.md` — entries follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.
- `MIGRATION.md` — per-release migration notes for `cargo add crap4rs`
  library consumers.
