# Configuration

A `crap.toml` file records the options you would otherwise pass on every run. Config values are merged with CLI flags at load time; flags always win. For the flags themselves, see the [CLI reference](cli-reference.md). For the CRAP math the thresholds gate against, see [understanding CRAP](understanding-crap.md).

## Discovery and `--config`

Without `--config`, the analyzer walks upward from its anchor directory looking for a config file. The anchor is the first `--src` root, or the working directory when `--src` is unset. Within each directory it resolves the candidate names in priority order; the **nearest ancestor directory that holds any candidate wins**, and the walk stops there.

```text
.
├── crap.toml          # discovered from any subdirectory at or below here
└── crates/
    └── core/src/      # a run anchored here climbs to ./crap.toml
```

The walk has no `.git` or workspace boundary — it climbs to the filesystem root. A stray `crap.toml` in a parent of your project (or in `$HOME`) is discovered when no nearer file exists. Pass `--config <path>` to bypass discovery entirely and load exactly that file; the tool has no opinion on what you name it, so no deprecation or shadow notice fires for an explicit path.

Discovery is by **existence, not parseability**. A present-but-malformed canonical file still wins and surfaces its parse error — it never silently falls through to an older file that happens to parse. Only a `NotFound` advances to the next candidate; any other I/O error (a `PermissionDenied` on a higher-priority file) stops the search and reports the path.

### Legacy names and shadow warnings

The canonical name is `crap.toml`. Each adapter also honors one legacy fallback for back-compat: `crap4rs.toml` (crap4rs) and `crap4ts.toml` (crap4ts). Discovering a legacy name emits a rename nudge:

```text
warning: using deprecated config name `crap4rs.toml`; rename it to `crap.toml`
```

When a lower-priority candidate also exists **in the winning directory**, it is reported as safe to remove:

```text
warning: `crap4rs.toml` is shadowed by `crap.toml` and ignored; it is safe to remove
```

Shadow detection is same-directory only — a file in a parent directory is never called redundant.

## Merge precedence

Each option resolves on a first-match-wins chain: **CLI flag → per-language section → top-level config key → default**. A bare CLI default (a flag the user did not set) does not count as "set" and defers to config. `--src` is the exception: a non-empty `--src` replaces the config `src` list wholesale, an empty `--src` defers to config, and an unset config defers to the `["src"]` default. `--src` has no per-language override.

Threshold resolution is more specific — a literal cutoff always beats a named preset at the same level:

| Order | Source |
|-------|--------|
| 1 | `--threshold N` (CLI literal) |
| 2 | `--strict` / `--lenient` (CLI preset) |
| 3 | `[language.<name>]` `threshold` |
| 4 | `[language.<name>]` `preset` |
| 5 | top-level `threshold` |
| 6 | top-level `preset` |
| 7 | no-flag default (the `default` preset, **15**) |

The presets are flat across both metrics today: `strict = 8`, `default = 15`, `lenient = 25`. This is a calibration convention, not an empirically derived constant.

## Reference

`crap.toml` deserializes with `deny_unknown_fields`: an unrecognized or misspelled key fails the run at load time rather than being silently ignored. Every key is optional — an absent key defers to the merge chain above.

### Top-level keys

| Key | Type | Meaning |
|-----|------|---------|
| `threshold` | float | Custom numeric CRAP cutoff. Mutually exclusive with `preset`. |
| `preset` | `"strict"` \| `"default"` \| `"lenient"` | Named threshold preset. Mutually exclusive with `threshold`. |
| `metric` | `"cognitive"` \| `"cyclomatic"` | Complexity metric. The Rust adapter defaults to `cognitive`; crap4ts is `cyclomatic`-only. See the note on per-language placement below. |
| `missing_coverage_policy` | `"pessimistic"` \| `"optimistic"` \| `"skip"` | How to score a function whose source file is entirely absent from coverage. The default, `pessimistic` (0% covered → `c²+c`), is a deliberate choice, not ground truth. `optimistic` treats it as 100% covered; `skip` omits it from the report. |
| `src` | string \| array of strings | Source root(s) to walk. A single root stays src-relative (byte-identical to the long-standing `src = "string"` form); multiple roots are keyed git-toplevel-relative and require a git work tree. |
| `exclude` | array of strings | Glob patterns matched against project-relative file paths; matching files are dropped from analysis. Each exclusion should carry a tracking issue or ADR. |
| `overrides` | array of tables | Per-path threshold overrides. See [`[[overrides]]`](#overrides). |
| `views` | table of tables | Saved view presets. See [`[views.<name>]`](#viewsname). |
| `language` | table of tables | Per-language overrides. See [`[language.<name>]`](#languagename). |
| `output` | table | Reporter knobs. See [`[output]`](#output). |
| `delta` | table | Delta-gate knobs. See [`[delta]`](#delta). |

`threshold` and `preset` are mutually exclusive — a config that sets both is rejected. This carve-out applies at the top level and recursively inside each `[language.<name>]` section.

### `[[overrides]]`

An array of tables, each applying a CRAP cutoff to files matching a glob:

```toml
[[overrides]]
pattern = "src/domain/**"   # glob matched against project-relative paths
threshold = 8.0             # finite, positive
```

### `[views.<name>]`

A reusable bundle of report-shaping flags, selected at runtime via `--view <name>`. Folding a preset into the run is OR-merge: a CLI bool of `false` reads as "unset" and a preset's `true` still applies.

| Key | Type | Meaning |
|-----|------|---------|
| `top` | int | Limit to the N highest-CRAP functions. |
| `min_coverage` | float | Keep functions with coverage at or above this percent. |
| `max_coverage` | float | Keep functions with coverage at or below this percent. |
| `sort` | `"crap"` \| `"coverage"` \| `"complexity"` \| `"path"` | Sort key. |
| `only_failing` | bool | Show only functions that exceed their threshold. |
| `no_fail` | bool | Report without failing the process on breaches. |
| `group_by` | `"file"` | Group rows by file. |
| `minimal_view` | bool | Render the compact view. |

Coverage bounds are validated at load time: each must be in `[0, 100]`, and `min_coverage` must not exceed `max_coverage`.

### `[language.<name>]`

Per-language override sections keyed by language name (`[language.rust]`, `[language.typescript]`). Each adapter reads **only its own section** and overlays a set value over the shared top-level default; languages the running adapter does not recognize are never selected. A section may assert any subset of `threshold`, `preset`, `metric`, and `exclude`.

`metric` lives here, not only at the top level — a per-language `metric` is the idiomatic place to set it for a multi-language project, since it sits above the top-level `metric` in the merge chain. The same `threshold`/`preset` mutual exclusion applies within each section.

```toml
metric = "cognitive"        # shared default

[language.rust]
threshold = 8.0             # crap4rs gates Rust at 8
metric = "cognitive"

[language.typescript]
threshold = 15.0            # crap4ts gates TS at 15
```

### `[output]`

| Key | Type | Meaning |
|-----|------|---------|
| `annotation_limit` | int `1..=100` | Cap on `::warning` annotations emitted by the `github-annotations` reporter per run. Matches the `--annotation-limit` range. |
| `title` | string | Scorecard header label. Absent renders the unlabeled header. |
| `subtitle` | string | Line rendered beneath the title. |

### `[delta]`

| Key | Type | Meaning |
|-----|------|---------|
| `epsilon` | float `>= 0.0` | Threshold-border jitter half-width (absolute CRAP points) for `--delta-gate`. A new violation crossing the threshold but staying within `epsilon` of it — on both the baseline and current side — is treated as border jitter and not counted. Default `0.0` disables suppression. This is a jitter knob, not a noise-only guarantee: a genuinely new in-band violation is suppressed too. |

The `threshold`-vs-`preset` exclusivity, the `annotation_limit` range, and the `epsilon` finiteness are all enforced at load time so config and CLI agree on the legal values.

## Generated artifacts and editor wiring

`crap4rs init` (and `crap4ts init`) writes an exhaustive annotated `crap.toml` — every supported key, with each key's documentation as an inline comment. The committed `crap.example.toml` at the repo root is the same generator output, byte for byte. `init` refuses to overwrite an existing file; pass `--force` to regenerate.

Because the file documents every option, the live config you keep is a trimmed subset. The example shows `threshold` live with `preset` commented out as the mutually-exclusive alternative — uncomment one and delete the other.

The committed `crap.schema.json` is the JSON Schema for `crap.toml`, generated from the same field documentation that drives the annotated example and the docs.rs hovers. Point your editor at it for completion and validation:

```toml
#:schema ./crap.schema.json
```

The schema and the example share one prose source, so there is no second hand-authored description to drift.
