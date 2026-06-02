//! Config file adapter — loads the adapter's TOML config (the ordered
//! file names are supplied by the binary via
//! `AdapterMeta::config_file_names`) and converts to domain types.
//!
//! Handles TOML parsing and config file discovery. All CLI-representable
//! options are supported. Per-path threshold overrides use glob patterns.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use documented::DocumentedFields;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::cli::AdapterMeta;

use crate::domain::threshold::{ThresholdOverride, ThresholdPreset, is_valid_threshold};
use crate::domain::types::ComplexityMetric;
use crate::domain::view::{CoverageRange, CoverageRangeError, GroupKey, SortKey};

// ── Parsed config types (re-exported from domain) ──────────────────
//
// The parsed-projection POD types (`FileConfig`, `OutputConfig`,
// `LangConfig`, `ViewPreset`) live in `crate::domain::config` — they are
// the language-agnostic shape the core merges and analyzes (#341). They
// are re-exported here so this adapter (which constructs them in
// `parse_config`) and existing consumers (`cli::mod`, `cli::view_args`)
// keep importing them from `adapters::config` unchanged. The
// wire/schema family below (`ConfigSchema` et al., schemars + documented)
// stays in this adapter layer.
pub use crate::domain::config::{FileConfig, LangConfig, OutputConfig, ViewPreset};

// ── Config schema — the public, documented WIRE type ───────────────
//
// WHY two config types. crap-core carries a `ConfigSchema` family (the
// wire/documented type, below) AND a `FileConfig` family (the parsed
// projection, above). They look redundant but serve opposite ends:
//
//   - `ConfigSchema` holds the values *as the user types them in
//     `crap.toml`* — `preset = "strict"` is a string, `src` is a
//     string-or-array. This is the type the JSON Schema and the
//     annotated example must describe, because that is what an editor
//     validates and what a human reads. Its `///` docs are the single
//     prose source: schemars renders them as property `description`s,
//     docs.rs renders them as hovers, and the eventual annotated
//     example (a later change) attaches them as TOML comments.
//   - `FileConfig` holds the *parsed* values — `preset:
//     Option<ThresholdPreset>`, `src: Vec<PathBuf>` — the shape the
//     analyzer consumes after validation.
//
// Annotating the wire type rather than `FileConfig` is forced twice
// over: putting `Serialize`/`JsonSchema` on `FileConfig` would emit a
// schema describing parsed enums (`"Strict"`) that does NOT match what
// the user writes (`"strict"`), and would leak serde onto the domain
// enums, which crap-core's language-agnostic-core rule forbids. The
// wire types are `String`-typed at the boundary and already live in the
// adapters layer, so the derives are purely additive here.

/// The crap config schema — the documented wire shape of `crap.toml`.
///
/// Every field carries a `///` doc that is the single prose location for
/// that option; schemars renders each as the JSON Schema property
/// `description`, and the same text surfaces on docs.rs. Deserializes
/// with `deny_unknown_fields` so a typo or a stale key fails loudly at
/// load time rather than being silently ignored.
#[derive(Debug, Deserialize, JsonSchema, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct ConfigSchema {
    /// Custom numeric CRAP cutoff. Functions scoring above this fail the
    /// run. Mutually exclusive with `preset` (set one, not both).
    pub threshold: Option<f64>,
    /// Named threshold preset: `"strict"`, `"default"`, or `"lenient"`.
    /// Mutually exclusive with `threshold`.
    pub preset: Option<String>,
    /// Complexity metric: `"cognitive"` (default for the Rust adapter)
    /// or `"cyclomatic"`.
    pub metric: Option<String>,
    /// Source root(s) the analyzer walks. Accepts a single string
    /// (`src = "crates"`) or an array (`src = ["crate-a", "crate-b"]`).
    /// A single root stays src-relative; multiple roots are keyed
    /// git-toplevel-relative and require a git work tree.
    pub src: Option<SrcSpec>,
    /// Glob patterns matched against project-relative file paths;
    /// matching files are excluded from analysis.
    pub exclude: Option<Vec<String>>,
    /// Per-path threshold overrides — an array of `[[overrides]]`
    /// blocks, each pairing a glob `pattern` with its own `threshold`.
    #[serde(default)]
    pub overrides: Vec<OverrideSchema>,
    /// Saved view presets keyed by name (`[views.<name>]`), each a
    /// reusable bundle of report-shaping flags selectable via `--view`.
    #[serde(default)]
    pub views: HashMap<String, ViewPresetSchema>,
    /// Per-language override sections keyed by language name
    /// (`[language.rust]`, `[language.typescript]`, …). Each adapter
    /// reads only its own section and overlays it over the shared
    /// top-level defaults.
    #[serde(default)]
    pub language: HashMap<String, LangSchema>,
    /// Output-shaping settings (`[output]`): annotation cap, scorecard
    /// title, and subtitle.
    #[serde(default)]
    pub output: OutputSchema,
}

/// Source-root specification — a single path or a list of paths.
///
/// Accepts either form in TOML so the long-standing `src = "string"`
/// form keeps working while multi-root configs use `src = [...]`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SrcSpec {
    /// A single source root: `src = "crates"`.
    One(String),
    /// Multiple source roots: `src = ["crate-a", "crate-b"]`.
    Many(Vec<String>),
}

impl SrcSpec {
    /// Flatten to the parsed `Vec<PathBuf>` the analyzer consumes.
    fn into_paths(self) -> Vec<PathBuf> {
        match self {
            SrcSpec::One(s) => vec![PathBuf::from(s)],
            SrcSpec::Many(v) => v.into_iter().map(PathBuf::from).collect(),
        }
    }
}

/// Output-shaping settings (the `[output]` table) on the wire type.
#[derive(Debug, Default, Deserialize, JsonSchema, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct OutputSchema {
    /// Cap on the number of warning annotations emitted by the GitHub
    /// annotations reporter per run. Must be in `1..=100`. A CLI
    /// `--annotation-limit` flag wins over this value.
    pub annotation_limit: Option<u32>,
    /// Scorecard title — a single header label for the whole report.
    pub title: Option<String>,
    /// Scorecard subtitle, rendered beneath the title.
    pub subtitle: Option<String>,
}

/// A single per-path threshold override (a `[[overrides]]` block).
#[derive(Debug, Deserialize, JsonSchema, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct OverrideSchema {
    /// Glob pattern matched against project-relative file paths.
    pub pattern: String,
    /// CRAP cutoff applied to functions in files matching `pattern`.
    pub threshold: f64,
}

/// A saved view preset (a `[views.<name>]` block) on the wire type.
#[derive(Debug, Default, Deserialize, JsonSchema, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct ViewPresetSchema {
    /// Limit the report to the N highest-CRAP functions.
    pub top: Option<u32>,
    /// Keep only functions with coverage at or above this percent.
    pub min_coverage: Option<f64>,
    /// Keep only functions with coverage at or below this percent.
    pub max_coverage: Option<f64>,
    /// Sort key: `"crap"`, `"coverage"`, `"complexity"`, or `"path"`.
    pub sort: Option<String>,
    /// Show only functions that exceed their threshold.
    pub only_failing: Option<bool>,
    /// Report without failing the process on threshold breaches.
    pub no_fail: Option<bool>,
    /// Group rows by `"file"`.
    pub group_by: Option<String>,
    /// Render the compact minimal view.
    pub minimal_view: Option<bool>,
}

/// A per-language override section (a `[language.<name>]` block) on the
/// wire type. May assert any subset of the shared per-language knobs.
#[derive(Debug, Default, Deserialize, JsonSchema, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct LangSchema {
    /// Per-language numeric CRAP cutoff. Mutually exclusive with the
    /// section's `preset`.
    pub threshold: Option<f64>,
    /// Per-language threshold preset. Mutually exclusive with the
    /// section's `threshold`.
    pub preset: Option<String>,
    /// Per-language complexity metric.
    pub metric: Option<String>,
    /// Per-language exclude globs.
    pub exclude: Option<Vec<String>>,
}

// ── Typed loader error ─────────────────────────────────────────────

/// Errors the config loader can produce.
///
/// Co-located with the loader (matching the `CrapError` /
/// `CoverageRangeError` precedent) and `#[non_exhaustive]` so adding a
/// variant is not a breaking change. The loader's public surface
/// (`discover_config`, `load_config`) and its private parse/validate
/// helpers all return `Result<_, ConfigError>`; the CLI boundary
/// (`cli::load_file_config` and the adapter binaries) lifts it into
/// `anyhow::Error` via `?`, where `render_error`'s `{:#}` walks the
/// `#[source]` chain to print the path-bearing wrapper plus its cause.
///
/// The error is **two-layer** by design: `Toml` carries only the raw
/// `toml::de::Error` (path-less — `parse_config` has no path) and
/// interpolates it directly so an in-crate `parse_config(..).to_string()`
/// surfaces the underlying deserialize message; `Parse` is the
/// path-bearing wrapper `load_config` adds, whose Display names the file
/// and the word "parse" without flattening the source (thiserror's
/// Display does not walk `#[source]`).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// A non-`NotFound` I/O error while probing a discovery candidate
    /// (e.g. `PermissionDenied` on a higher-priority file). Surfaces
    /// rather than being masked by a legacy-config fall-through.
    #[error("cannot access config file {path}: {source}\n  hint: check file permissions", path = path.display())]
    Access {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Failed to read the config file's contents.
    #[error("failed to read config file: {path}", path = path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The path-bearing wrapper `load_config` adds around a parse
    /// failure, so the user sees which file failed. Its Display names
    /// the file and "parse"; the underlying `ConfigError` (a `Toml` or
    /// a validation variant) is the `#[source]`.
    #[error("failed to parse config file: {path}", path = path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<ConfigError>,
    },
    /// Raw TOML deserialize failure (path-less). Interpolates its source
    /// so `parse_config(..).to_string()` shows the underlying message
    /// (e.g. an `unknown field` from `deny_unknown_fields`).
    #[error("{0}")]
    Toml(#[from] toml::de::Error),
    /// `preset` and `threshold` both set. `section` names the
    /// `[language.<name>]` block when the conflict is per-language,
    /// `None` at the top level.
    #[error("{}preset and threshold are mutually exclusive in config", section_prefix(.section))]
    MutuallyExclusive { section: Option<String> },
    /// A `threshold` (top-level or per-language) that is not finite and
    /// positive.
    #[error("{}threshold must be a finite positive number, got: {value}", section_prefix(.section))]
    InvalidThreshold { value: f64, section: Option<String> },
    /// An `[[overrides]]` `threshold` that is not finite and positive.
    #[error(
        "override threshold must be a finite positive number, got: {value} (pattern: {pattern})"
    )]
    InvalidOverrideThreshold { value: f64, pattern: String },
    /// An `[output] annotation_limit` outside `1..=100`.
    #[error(
        "output.annotation_limit must be in 1..=100, got: {value}\n  hint: matches the CLI `--annotation-limit` range; 0 disables emission, > 100 floods the GH Actions per-step cap"
    )]
    InvalidAnnotationLimit { value: u32 },
    /// An unrecognized `preset` string.
    #[error("unknown preset: {value}\n  valid values: strict, default, lenient")]
    UnknownPreset { value: String },
    /// An unrecognized `metric` string.
    #[error("unknown metric: {value}\n  valid values: cognitive, cyclomatic")]
    UnknownMetric { value: String },
    /// An unrecognized view-preset `sort` string. `preset` names the
    /// `[views.<name>]` block.
    #[error(
        "preset `{preset}`: unknown sort: {value}\n  valid values: crap, coverage, complexity, path"
    )]
    UnknownSortKey { preset: String, value: String },
    /// An unrecognized view-preset `group_by` string.
    #[error("preset `{preset}`: unknown group_by: {value}\n  valid values: file")]
    UnknownGroupKey { preset: String, value: String },
    /// A view-preset coverage bound outside `[0, 100]`.
    #[error("preset `{preset}`: coverage value out of range: {value}\n  valid range: [0, 100]")]
    CoverageOutOfRange { preset: String, value: f64 },
    /// A view-preset `min_coverage` greater than its `max_coverage`.
    #[error("preset `{preset}`: min_coverage ({min}) must not exceed max_coverage ({max})")]
    CoverageMinExceedsMax { preset: String, min: f64, max: f64 },
}

/// Render a `[language.<name>]: ` prefix for the section-scoped error
/// variants, or empty string at the top level. Keeps the per-section
/// variants' Display byte-identical to the previous `bail!` messages
/// (so `"language.rust"` stays asserted) without duplicating two
/// `#[error]` strings per variant.
fn section_prefix(section: &Option<String>) -> String {
    match section {
        Some(name) => format!("[language.{name}]: "),
        None => String::new(),
    }
}

// ── Public API ─────────────────────────────────────────────────────

/// Outcome of an ordered config-file discovery scan.
///
/// Returned by [`discover_config`] when at least one candidate exists on
/// disk. `path` is the winning file — the highest-priority candidate that
/// exists. `used_index` is its position in the candidate list: index 0 is
/// the canonical name the adapter writes and names in hints (`crap.toml`),
/// and any higher index is a legacy per-adapter fallback the CLI surfaces
/// as a deprecation nudge. `shadowed` lists lower-priority candidates that
/// also exist on disk and were therefore ignored, so the CLI can tell the
/// operator they are safe to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDiscovery {
    /// The discovered config file (highest-priority existing candidate).
    pub path: PathBuf,
    /// Index of `path` within the candidate list (0 = canonical name).
    pub used_index: usize,
    /// Lower-priority candidates that also exist and were shadowed by
    /// `path`. Only confirmed regular files appear here.
    pub shadowed: Vec<PathBuf>,
}

/// Discover the adapter's config file by walking upward from `start`
/// (and every ancestor directory) and, within each directory, resolving
/// `file_names` in priority order. **The first ancestor directory that
/// yields any candidate wins** — the walk stops there.
///
/// `start` is the directory the search anchors on — in production the
/// first `--src` root, or the working directory when `--src` is empty.
/// It is **absolutized** via [`std::path::absolute`] before walking
/// (NOT canonicalized): `Path::ancestors()` is purely *lexical*, so
/// `"crates/foo".ancestors()` would yield `["crates/foo", "crates", ""]`
/// and never climb to the real repo root. `absolute` makes the path
/// absolute by prepending the process CWD when it is relative, so the
/// ancestors are the genuine on-disk parent chain — without touching the
/// filesystem, resolving symlinks, or erroring on a non-existent path
/// (which `canonicalize` would). Because it consults the process CWD only
/// (never *mutates* it), the function stays parallel-safe under nextest;
/// a relative `start` resolves against whatever CWD the caller runs in.
///
/// Within each directory the resolution mirrors the single-directory
/// contract: index 0 in `file_names` is the canonical name (`crap.toml`);
/// later entries are legacy per-adapter fallbacks. Discovery is by
/// *existence*, not parseability — a present-but-malformed canonical file
/// still wins and surfaces its parse error downstream, never silently
/// falling through to a stale legacy file that happens to parse. Only
/// `io::ErrorKind::NotFound` advances to the next name; any other I/O
/// error (e.g. `PermissionDenied`) short-circuits with `Err` so a
/// permission problem on the canonical config surfaces rather than being
/// masked by a legacy-config deprecation warning.
///
/// Shadow detection stays **same-directory only**: lower-priority names
/// that also exist *in the winning directory* are reported as `shadowed`
/// (safe to remove). A file in a *parent* directory is never reported as
/// shadowed — the operator is never told a file outside the chosen
/// config's directory is redundant.
///
/// The walk climbs to the filesystem root with no `.git` / workspace
/// boundary stop. One consequence to be aware of: a stray `crap.toml` in
/// `$HOME` (or any ancestor above the project) will be discovered when no
/// nearer config exists. Pass an explicit `--config` to bypass discovery
/// entirely.
///
/// Returns `Ok(Some(ConfigDiscovery))` when a candidate exists in some
/// ancestor directory, `Ok(None)` when none do anywhere up to the root.
pub fn discover_config(
    start: &Path,
    file_names: &[&str],
) -> Result<Option<ConfigDiscovery>, ConfigError> {
    // Absolutize so `.ancestors()` climbs the real parent chain rather
    // than the lexical components of a relative `start` (C-1). On the
    // rare error path (e.g. an empty path) fall back to the raw `start`
    // so a single-dir lookup still works.
    let anchored = std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf());

    for dir in anchored.ancestors() {
        if let Some(found) = discover_in_dir(dir, file_names)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// Run the ordered-name resolution within a single directory. Returns
/// `Ok(Some(_))` for the highest-priority existing file (with any
/// lower-priority same-dir files recorded as `shadowed`), `Ok(None)`
/// when no candidate exists here (so the caller advances to the parent),
/// or `Err` on a non-`NotFound` I/O error probing a candidate.
fn discover_in_dir(
    dir: &Path,
    file_names: &[&str],
) -> Result<Option<ConfigDiscovery>, ConfigError> {
    for (index, name) in file_names.iter().enumerate() {
        let candidate = dir.join(name);
        match std::fs::metadata(&candidate) {
            Ok(m) if m.is_file() => {
                // Winner found. Report any lower-priority names that also
                // exist *in this same directory* as `shadowed` so the
                // operator can delete them. Existence-confirmed via
                // `is_file()` (not `exists()`): a directory at a
                // lower-priority name is never reported as shadowed.
                let shadowed = file_names[index + 1..]
                    .iter()
                    .map(|n| dir.join(n))
                    .filter(|p| p.is_file())
                    .collect();
                return Ok(Some(ConfigDiscovery {
                    path: candidate,
                    used_index: index,
                    shadowed,
                }));
            }
            // Exists but not a regular file (directory, etc.) — not a
            // config we can load; advance to the next name.
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            // Construct the variant manually (not `#[from]`) so the
            // load-bearing `path` survives onto the error (I-4).
            Err(source) => {
                return Err(ConfigError::Access {
                    path: candidate,
                    source,
                });
            }
        }
    }
    Ok(None)
}

/// Load and parse a config file from the given path.
pub fn load_config(path: &Path) -> Result<FileConfig, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_config(&content).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

/// Parse TOML content into a `FileConfig`.
fn parse_config(content: &str) -> Result<FileConfig, ConfigError> {
    let raw: ConfigSchema = toml::from_str(content)?;
    validate_raw_config(&raw)?;

    let metric = raw.metric.as_deref().map(parse_metric).transpose()?;
    let preset = raw.preset.as_deref().map(parse_preset).transpose()?;

    let overrides = parse_overrides(raw.overrides);
    let views = parse_views(raw.views)?;
    let language = parse_languages(raw.language)?;

    Ok(FileConfig {
        threshold: raw.threshold,
        preset,
        metric,
        src: raw.src.map(SrcSpec::into_paths).unwrap_or_default(),
        exclude: raw.exclude,
        overrides,
        views,
        language,
        output: OutputConfig {
            annotation_limit: raw.output.annotation_limit,
            title: raw.output.title,
            subtitle: raw.output.subtitle,
        },
    })
}

/// Project the wire `[[overrides]]` blocks into the domain's per-path
/// override list. Infallible — the override thresholds are range-checked
/// up front by `validate_raw_config`, so this is a pure field rename.
fn parse_overrides(raw: Vec<OverrideSchema>) -> Vec<ThresholdOverride> {
    raw.into_iter()
        .map(|o| ThresholdOverride {
            pattern: o.pattern,
            threshold: o.threshold,
        })
        .collect()
}

/// Project the wire `[views.<name>]` map into the domain's saved view
/// presets, parsing each section's sort/group keys (the fallible step,
/// hence the `Result`-bearing collect).
fn parse_views(
    raw: HashMap<String, ViewPresetSchema>,
) -> Result<HashMap<String, ViewPreset>, ConfigError> {
    raw.into_iter()
        .map(|(name, raw_preset)| {
            let preset = parse_view_preset(&name, raw_preset)?;
            Ok((name, preset))
        })
        .collect()
}

/// Project the wire `[language.<name>]` map into the domain's per-language
/// config, validating each section's preset/threshold exclusivity (the
/// fallible step, hence the `Result`-bearing collect).
fn parse_languages(
    raw: HashMap<String, LangSchema>,
) -> Result<HashMap<String, LangConfig>, ConfigError> {
    raw.into_iter()
        .map(|(name, raw_lang)| {
            let lang = parse_lang_config(&name, raw_lang)?;
            Ok((name, lang))
        })
        .collect()
}

/// Project a per-language wire section into its parsed [`LangConfig`].
/// Carries the same `preset`/`threshold` mutual-exclusion and metric
/// validation the top level enforces, naming the offending section.
fn parse_lang_config(name: &str, raw: LangSchema) -> Result<LangConfig, ConfigError> {
    if raw.preset.is_some() && raw.threshold.is_some() {
        return Err(ConfigError::MutuallyExclusive {
            section: Some(name.to_string()),
        });
    }
    if let Some(t) = raw.threshold
        && !is_valid_threshold(t)
    {
        return Err(ConfigError::InvalidThreshold {
            value: t,
            section: Some(name.to_string()),
        });
    }
    let metric = raw.metric.as_deref().map(parse_metric).transpose()?;
    let preset = raw.preset.as_deref().map(parse_preset).transpose()?;
    Ok(LangConfig {
        threshold: raw.threshold,
        preset,
        metric,
        exclude: raw.exclude,
    })
}

fn validate_raw_config(raw: &ConfigSchema) -> Result<(), ConfigError> {
    check_preset_threshold_exclusive(raw)?;
    check_top_level_threshold_range(raw)?;
    check_override_thresholds(raw)?;
    check_annotation_limit(raw)?;
    Ok(())
}

/// `preset` and `threshold` are mutually exclusive at the top level (set
/// one, not both). The per-section variant of this carve-out lives in
/// `parse_lang_config`; this one names no section.
fn check_preset_threshold_exclusive(raw: &ConfigSchema) -> Result<(), ConfigError> {
    if raw.preset.is_some() && raw.threshold.is_some() {
        return Err(ConfigError::MutuallyExclusive { section: None });
    }
    Ok(())
}

/// The top-level `threshold`, when set, must be a positive finite value
/// (`is_valid_threshold`). Section-less so the error points at the root.
fn check_top_level_threshold_range(raw: &ConfigSchema) -> Result<(), ConfigError> {
    if let Some(t) = raw.threshold
        && !is_valid_threshold(t)
    {
        return Err(ConfigError::InvalidThreshold {
            value: t,
            section: None,
        });
    }
    Ok(())
}

/// Every `[[overrides]]` threshold must be in the same valid range as the
/// top-level threshold; the error names the offending glob `pattern`.
fn check_override_thresholds(raw: &ConfigSchema) -> Result<(), ConfigError> {
    for o in &raw.overrides {
        if !is_valid_threshold(o.threshold) {
            return Err(ConfigError::InvalidOverrideThreshold {
                value: o.threshold,
                pattern: o.pattern.clone(),
            });
        }
    }
    Ok(())
}

/// `[output] annotation_limit`, when set, must be in `1..=100`.
///
/// Mirror the CLI's `clap::value_parser!(u32).range(1..=100)` on
/// `--annotation-limit` so config and CLI agree on the legal range.
/// Without this check a TOML `[output] annotation_limit = 0` would
/// silently disable annotation emission (only the truncation notice
/// fires); `= 999` would silently flood the per-step UI cap. Both
/// are rejected by clap at the CLI boundary — config must match.
fn check_annotation_limit(raw: &ConfigSchema) -> Result<(), ConfigError> {
    if let Some(limit) = raw.output.annotation_limit
        && !(1..=100).contains(&limit)
    {
        return Err(ConfigError::InvalidAnnotationLimit { value: limit });
    }
    Ok(())
}

fn parse_view_preset(name: &str, raw: ViewPresetSchema) -> Result<ViewPreset, ConfigError> {
    let sort = raw
        .sort
        .as_deref()
        .map(|s| parse_sort_key(name, s))
        .transpose()?;
    let group_by = raw
        .group_by
        .as_deref()
        .map(|s| parse_group_key(name, s))
        .transpose()?;
    validate_preset_coverage_range(name, raw.min_coverage, raw.max_coverage)?;
    Ok(ViewPreset {
        top: raw.top,
        min_coverage: raw.min_coverage,
        max_coverage: raw.max_coverage,
        sort,
        only_failing: raw.only_failing,
        no_fail: raw.no_fail,
        group_by,
        minimal_view: raw.minimal_view,
    })
}

fn parse_sort_key(preset_name: &str, s: &str) -> Result<SortKey, ConfigError> {
    match s {
        "crap" => Ok(SortKey::Crap),
        "coverage" => Ok(SortKey::Coverage),
        "complexity" => Ok(SortKey::Complexity),
        "path" => Ok(SortKey::Path),
        other => Err(ConfigError::UnknownSortKey {
            preset: preset_name.to_string(),
            value: other.to_string(),
        }),
    }
}

fn parse_group_key(preset_name: &str, s: &str) -> Result<GroupKey, ConfigError> {
    match s {
        "file" => Ok(GroupKey::File),
        other => Err(ConfigError::UnknownGroupKey {
            preset: preset_name.to_string(),
            value: other.to_string(),
        }),
    }
}

/// Validate the preset's coverage bounds in isolation (fail-fast at config
/// load). Either-side-only is allowed and the absent side is
/// defaulted to `0` / `100` for the relational check, mirroring CLI
/// `validate_view_args` so a preset that would resolve to an invalid range
/// is rejected at TOML parse time rather than at `--view` resolution.
fn validate_preset_coverage_range(
    preset_name: &str,
    min: Option<f64>,
    max: Option<f64>,
) -> Result<(), ConfigError> {
    if min.is_none() && max.is_none() {
        return Ok(());
    }
    let lo = min.unwrap_or(0.0);
    let hi = max.unwrap_or(100.0);
    // `CoverageRangeError` has `#[non_exhaustive]` paused per ADR D10
    // (restored at v1.0). Now that this adapter lives in crap-core
    // alongside the enum, the match is in-crate and exhaustive — no
    // wildcard arm needed. v1.0 new variants will require an
    // explicit arm here.
    match CoverageRange::new(lo, hi) {
        Ok(_) => Ok(()),
        Err(CoverageRangeError::OutOfRange { value }) => Err(ConfigError::CoverageOutOfRange {
            preset: preset_name.to_string(),
            value,
        }),
        Err(CoverageRangeError::MinExceedsMax { min, max }) => {
            Err(ConfigError::CoverageMinExceedsMax {
                preset: preset_name.to_string(),
                min,
                max,
            })
        }
    }
}

fn parse_preset(s: &str) -> Result<ThresholdPreset, ConfigError> {
    match s {
        "strict" => Ok(ThresholdPreset::Strict),
        "default" => Ok(ThresholdPreset::Default),
        "lenient" => Ok(ThresholdPreset::Lenient),
        other => Err(ConfigError::UnknownPreset {
            value: other.to_string(),
        }),
    }
}

fn parse_metric(s: &str) -> Result<ComplexityMetric, ConfigError> {
    match s {
        "cognitive" => Ok(ComplexityMetric::Cognitive),
        "cyclomatic" => Ok(ComplexityMetric::Cyclomatic),
        other => Err(ConfigError::UnknownMetric {
            value: other.to_string(),
        }),
    }
}

// ── JSON Schema artifact ───────────────────────────────────────────

/// Render the committed JSON Schema for [`ConfigSchema`] as pretty JSON.
///
/// This is the editor `$schema` target for `crap.toml`. The committed
/// `crap.schema.json` at the repo root **is** this function's output;
/// a sync test asserts they stay byte-identical. Each field's `///` doc
/// surfaces as the schema property's `description`, so the schema and
/// the docs.rs hovers share one prose source — there is no parallel
/// hand-authored schema to drift.
pub fn config_json_schema() -> String {
    let schema = schemars::schema_for!(ConfigSchema);
    serde_json::to_string_pretty(&schema).expect("ConfigSchema JSON Schema serializes")
}

// ── Annotated example artifact ─────────────────────────────────────

/// Turn a `documented` field doc (the raw `///` text, possibly multiple
/// lines) into a leading TOML comment block (`# line\n…`), with a
/// trailing newline so it sits directly above the key.
fn doc_comment(doc: &str) -> String {
    let mut out = String::new();
    for line in doc.trim_end().lines() {
        out.push_str("# ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Look up a schema struct field's `///` doc by field name. Panics if the
/// name is absent — that can only happen if a field was renamed without
/// updating the emitter, which the no-`..` destructure makes a compile
/// error first, so this is an internal invariant, not a runtime path.
fn field_doc<T: DocumentedFields>(field: &str) -> &'static str {
    T::get_field_docs(field).unwrap_or_else(|_| {
        panic!(
            "schema field `{field}` has no documented `///` doc — every field must be documented"
        )
    })
}

/// Attach `doc` as the leading comment above `key` in `table`
/// (key-decor prefix; see the `toml_edit` decor model). `blank_before`
/// inserts a separating blank line above the comment block.
fn comment_key(table: &mut toml_edit::Table, key: &str, doc: &str, blank_before: bool) {
    if let Some(mut k) = table.key_mut(key) {
        let prefix = if blank_before {
            format!("\n{}", doc_comment(doc))
        } else {
            doc_comment(doc)
        };
        k.leaf_decor_mut().set_prefix(prefix);
    }
}

/// Render the exhaustive annotated `crap.example.toml`.
///
/// This is the single generator behind the committed `crap.example.toml`
/// reference AND `crap4rs init` / `crap4ts init` — `init` writes this
/// output verbatim, and the committed example **is** this function's
/// output (a byte-identical sync test guards it). NOT loaded by the tool;
/// purely the canonical option reference.
///
/// The example exercises **every** field. `threshold` is live and
/// `preset` is shown as a commented alternative because the two are
/// mutually exclusive — the loader rejects a config that sets both.
/// The same carve-out applies recursively to each `[language.<name>]`
/// section. Maps (`views`, `language`) are emitted by sorted key so the
/// output is deterministic (the sync test is byte-identical).
///
/// The compile-time completeness guard is the exhaustive `let
/// ConfigSchema { .. } = ..` destructure below with **no `..`**: adding a
/// field to `ConfigSchema` fails to compile until it is wired into this
/// emitter, so a new option can never silently ship undocumented.
pub fn render_example_config(meta: &AdapterMeta) -> String {
    use toml_edit::DocumentMut;

    // Exhaustive destructure — NO `..`. The compile guard: a new
    // ConfigSchema field breaks this line until it is emitted below.
    let example = exhaustive_example(meta);
    let ConfigSchema {
        threshold,
        preset,
        metric,
        src,
        exclude,
        overrides,
        views,
        language,
        output,
    } = example;

    // `preset` is the commented mutually-exclusive alternative to the live
    // `threshold` (Gate E), so the value never carries it — it is rendered
    // as a `#`-comment below. Binding it (rather than `..`) keeps the
    // exhaustive-destructure compile guard intact; asserting None documents
    // the invariant and consumes the binding.
    debug_assert!(
        preset.is_none(),
        "the exhaustive example must leave `preset` unset (threshold is live; preset is the commented alternative)"
    );

    let mut doc = DocumentMut::new();
    let root = doc.as_table_mut();
    root.set_implicit(false);

    emit_header_banner(root, meta);
    emit_top_scalars(root, threshold, metric, src, exclude);
    emit_overrides_block(root, overrides);
    emit_views_block(root, views);
    emit_language_block(root, language);
    emit_output_block(root, output);

    doc.to_string()
}

/// Emit the leading `#`-comment banner on the root table — the three-line
/// "generated by `<tool> init`" header naming the canonical config file.
fn emit_header_banner(root: &mut toml_edit::Table, meta: &AdapterMeta) {
    root.decor_mut().set_prefix(format!(
        "# {name} — exhaustive annotated config reference (every supported option).\n\
         # Generated by `{tool} init`; regenerate with `{tool} init --force`.\n\
         # This file documents every field — your real {name} is a trimmed subset.\n\n",
        name = meta.canonical_config_file_name(),
        tool = meta.tool_name,
    ));
}

/// Emit the top-level scalar keys in order: live `threshold` (with the
/// commented `preset` alternative beneath it), `metric`, `src` array, and
/// `exclude` array. `preset` is None in the value so the example
/// round-trips to `threshold`; it renders as a `#`-comment carrying its
/// own doc so the field stays documented.
fn emit_top_scalars(
    root: &mut toml_edit::Table,
    threshold: Option<f64>,
    metric: Option<String>,
    src: Option<SrcSpec>,
    exclude: Option<Vec<String>>,
) {
    use toml_edit::{Array, value};

    // threshold is live; preset is the commented mutually-exclusive
    // alternative (emitted as a comment so the example round-trips).
    let threshold_val = threshold.expect("exhaustive example sets threshold");
    root.insert("threshold", value(threshold_val));
    comment_key(
        root,
        "threshold",
        field_doc::<ConfigSchema>("threshold"),
        false,
    );
    emit_preset_alternative_comment(root);

    root.insert("metric", value(metric.expect("metric set")));
    comment_key(root, "metric", field_doc::<ConfigSchema>("metric"), true);

    // src: emit the multi-root array form (the exhaustive shape).
    let mut src_arr = Array::new();
    for p in src_to_strings(&src.expect("exhaustive example sets src")) {
        src_arr.push(p);
    }
    root.insert("src", value(src_arr));
    comment_key(root, "src", field_doc::<ConfigSchema>("src"), true);

    // exclude: non-empty array.
    let mut excl_arr = Array::new();
    for p in exclude.expect("exclude set") {
        excl_arr.push(p);
    }
    root.insert("exclude", value(excl_arr));
    comment_key(root, "exclude", field_doc::<ConfigSchema>("exclude"), true);
}

/// Attach the commented `# preset = "..."` alternative as a suffix on the
/// live `threshold` value, so it renders directly below the `threshold =
/// N` line. The block carries the `preset` field's own doc.
fn emit_preset_alternative_comment(root: &mut toml_edit::Table) {
    let preset_block = format!(
        "{doc}# preset = \"{val}\"\n",
        doc = doc_comment(field_doc::<ConfigSchema>("preset")),
        val = preset_alternative(),
    );
    if let Some(v) = root.get_mut("threshold").and_then(|i| i.as_value_mut()) {
        v.decor_mut().set_suffix(format!("\n\n{preset_block}"));
    }
}

/// Emit the `[[overrides]]` array-of-tables — one table per override with
/// its `pattern`/`threshold` keys, and the `overrides` field doc as a
/// header above the first table.
fn emit_overrides_block(root: &mut toml_edit::Table, overrides: Vec<OverrideSchema>) {
    use toml_edit::{ArrayOfTables, Item, Table, value};

    let mut overrides_aot = ArrayOfTables::new();
    for o in overrides {
        // Exhaustive destructure (no `..`) — extends the compile guard to
        // OverrideSchema: a new field breaks this until it is emitted.
        let OverrideSchema { pattern, threshold } = o;
        let mut t = Table::new();
        t.insert("pattern", value(pattern));
        comment_key(
            &mut t,
            "pattern",
            field_doc::<OverrideSchema>("pattern"),
            false,
        );
        t.insert("threshold", value(threshold));
        comment_key(
            &mut t,
            "threshold",
            field_doc::<OverrideSchema>("threshold"),
            false,
        );
        overrides_aot.push(t);
    }
    // Header doc for the overrides array.
    if let Some(first) = overrides_aot.get_mut(0) {
        first.decor_mut().set_prefix(format!(
            "\n{}",
            doc_comment(field_doc::<ConfigSchema>("overrides"))
        ));
    }
    root.insert("overrides", Item::ArrayOfTables(overrides_aot));
}

/// Emit the `[views.<name>]` tables, sorted by name for deterministic
/// output, with the `views` field doc above the first sub-table.
fn emit_views_block(root: &mut toml_edit::Table, views: HashMap<String, ViewPresetSchema>) {
    use toml_edit::{Item, Table};

    let mut views_sorted: Vec<_> = views.into_iter().collect();
    views_sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut views_tbl = Table::new();
    views_tbl.set_implicit(true);
    let mut first = true;
    for (name, vp) in views_sorted {
        let mut t = Table::new();
        insert_view_preset(&mut t, vp);
        set_section_doc_on_first(&mut t, &mut first, field_doc::<ConfigSchema>("views"));
        views_tbl.insert(&name, Item::Table(t));
    }
    root.insert("views", Item::Table(views_tbl));
}

/// Emit the `[language.<name>]` tables, sorted by name for deterministic
/// output, with the `language` field doc above the first sub-table.
fn emit_language_block(root: &mut toml_edit::Table, language: HashMap<String, LangSchema>) {
    use toml_edit::{Item, Table};

    let mut lang_sorted: Vec<_> = language.into_iter().collect();
    lang_sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut lang_tbl = Table::new();
    lang_tbl.set_implicit(true);
    let mut first = true;
    for (name, ls) in lang_sorted {
        let mut t = Table::new();
        insert_lang_section(&mut t, ls);
        set_section_doc_on_first(&mut t, &mut first, field_doc::<ConfigSchema>("language"));
        lang_tbl.insert(&name, Item::Table(t));
    }
    root.insert("language", Item::Table(lang_tbl));
}

/// Set `doc` as the section-header comment on `table` only when `first` is
/// still true, flipping it to false. Shared by the views/language map
/// emitters so the field doc renders above the first sub-table only.
fn set_section_doc_on_first(table: &mut toml_edit::Table, first: &mut bool, doc: &str) {
    if *first {
        table
            .decor_mut()
            .set_prefix(format!("\n{}", doc_comment(doc)));
        *first = false;
    }
}

/// Emit the `[output]` table — `annotation_limit`, `title`, `subtitle`,
/// each with its `///` doc, and the `output` field doc as the table
/// header.
fn emit_output_block(root: &mut toml_edit::Table, output: OutputSchema) {
    use toml_edit::{Item, Table, value};

    // Exhaustive destructure (no `..`) — extends the compile guard to
    // OutputSchema: a new field breaks this until it is emitted.
    let OutputSchema {
        annotation_limit,
        title,
        subtitle,
    } = output;
    let mut t = Table::new();
    t.insert(
        "annotation_limit",
        value(i64::from(annotation_limit.expect("annotation_limit"))),
    );
    comment_key(
        &mut t,
        "annotation_limit",
        field_doc::<OutputSchema>("annotation_limit"),
        false,
    );
    t.insert("title", value(title.expect("title")));
    comment_key(&mut t, "title", field_doc::<OutputSchema>("title"), false);
    t.insert("subtitle", value(subtitle.expect("subtitle")));
    comment_key(
        &mut t,
        "subtitle",
        field_doc::<OutputSchema>("subtitle"),
        false,
    );
    t.decor_mut().set_prefix(format!(
        "\n{}",
        doc_comment(field_doc::<ConfigSchema>("output"))
    ));
    root.insert("output", Item::Table(t));
}

/// Insert every field of a [`ViewPresetSchema`] into `table`, each with
/// its `///` doc as a leading comment. Exhaustive (no `..`) so a new
/// view-preset field fails to compile until it is emitted here.
fn insert_view_preset(table: &mut toml_edit::Table, vp: ViewPresetSchema) {
    use toml_edit::value;
    let ViewPresetSchema {
        top,
        min_coverage,
        max_coverage,
        sort,
        only_failing,
        no_fail,
        group_by,
        minimal_view,
    } = vp;
    table.insert("top", value(i64::from(top.expect("top"))));
    comment_key(table, "top", field_doc::<ViewPresetSchema>("top"), false);
    table.insert("min_coverage", value(min_coverage.expect("min_coverage")));
    comment_key(
        table,
        "min_coverage",
        field_doc::<ViewPresetSchema>("min_coverage"),
        false,
    );
    table.insert("max_coverage", value(max_coverage.expect("max_coverage")));
    comment_key(
        table,
        "max_coverage",
        field_doc::<ViewPresetSchema>("max_coverage"),
        false,
    );
    table.insert("sort", value(sort.expect("sort")));
    comment_key(table, "sort", field_doc::<ViewPresetSchema>("sort"), false);
    table.insert("only_failing", value(only_failing.expect("only_failing")));
    comment_key(
        table,
        "only_failing",
        field_doc::<ViewPresetSchema>("only_failing"),
        false,
    );
    table.insert("no_fail", value(no_fail.expect("no_fail")));
    comment_key(
        table,
        "no_fail",
        field_doc::<ViewPresetSchema>("no_fail"),
        false,
    );
    table.insert("group_by", value(group_by.expect("group_by")));
    comment_key(
        table,
        "group_by",
        field_doc::<ViewPresetSchema>("group_by"),
        false,
    );
    table.insert("minimal_view", value(minimal_view.expect("minimal_view")));
    comment_key(
        table,
        "minimal_view",
        field_doc::<ViewPresetSchema>("minimal_view"),
        false,
    );
}

/// Insert every field of a [`LangSchema`] into `table`. `threshold` is
/// live; `preset` is the commented mutually-exclusive alternative (same
/// carve-out as the top level). Exhaustive (no `..`).
fn insert_lang_section(table: &mut toml_edit::Table, ls: LangSchema) {
    use toml_edit::value;
    let LangSchema {
        threshold,
        preset,
        metric,
        exclude,
    } = ls;
    // threshold live; preset is the commented mutually-exclusive
    // alternative (rendered as a `#`-comment below). Asserting None
    // documents the per-section carve-out and consumes the binding while
    // keeping the exhaustive-destructure compile guard intact.
    debug_assert!(
        preset.is_none(),
        "each [language.*] section must leave `preset` unset (threshold is live; preset is the commented alternative)"
    );
    table.insert("threshold", value(threshold.expect("language threshold")));
    comment_key(
        table,
        "threshold",
        field_doc::<LangSchema>("threshold"),
        false,
    );
    if let Some(v) = table.get_mut("threshold").and_then(|i| i.as_value_mut()) {
        v.decor_mut().set_suffix(format!(
            "\n{doc}# preset = \"{val}\"\n",
            doc = doc_comment(field_doc::<LangSchema>("preset")),
            val = preset_alternative(),
        ));
    }
    table.insert("metric", value(metric.expect("language metric")));
    comment_key(table, "metric", field_doc::<LangSchema>("metric"), false);
    let mut excl = toml_edit::Array::new();
    for p in exclude.expect("language exclude") {
        excl.push(p);
    }
    table.insert("exclude", value(excl));
    comment_key(table, "exclude", field_doc::<LangSchema>("exclude"), false);
}

/// The preset string shown as the commented alternative to `threshold`.
fn preset_alternative() -> &'static str {
    "default"
}

/// Project the example's `SrcSpec` into the string roots it renders as.
fn src_to_strings(src: &SrcSpec) -> Vec<String> {
    match src {
        SrcSpec::One(s) => vec![s.clone()],
        SrcSpec::Many(v) => v.clone(),
    }
}

/// Build the exhaustive `ConfigSchema` value the example renders from.
///
/// Every `Option` is `Some` **except `preset`** (the commented
/// alternative to the live `threshold` — they are mutually exclusive),
/// and every collection is non-empty. Per-language sections set
/// `threshold` (live) and leave `preset` `None` for the same reason.
/// The value passes `validate_raw_config` + the per-view / per-language
/// validators (the round-trip test asserts this).
fn exhaustive_example(meta: &AdapterMeta) -> ConfigSchema {
    let mut views = HashMap::new();
    views.insert(
        "ci".to_string(),
        ViewPresetSchema {
            top: Some(20),
            min_coverage: Some(0.0),
            max_coverage: Some(90.0),
            sort: Some("coverage".to_string()),
            only_failing: Some(true),
            no_fail: Some(false),
            group_by: Some("file".to_string()),
            minimal_view: Some(true),
        },
    );

    // Enumerate BOTH known language sections so the example is a complete
    // reference (not just the running adapter's). The section-name strings
    // are example data here, never crap-core code literals.
    let mut language = HashMap::new();
    for key in ["rust", "typescript"] {
        language.insert(
            key.to_string(),
            LangSchema {
                threshold: Some(8.0),
                preset: None,
                metric: Some("cyclomatic".to_string()),
                exclude: Some(vec!["generated/**".to_string()]),
            },
        );
    }

    ConfigSchema {
        threshold: Some(15.0),
        preset: None,
        metric: Some("cognitive".to_string()),
        src: Some(SrcSpec::Many(vec![
            "crates/core/src".to_string(),
            "crates/cli/src".to_string(),
        ])),
        exclude: Some(
            meta.default_excludes
                .iter()
                .map(|s| (*s).to_string())
                .chain(std::iter::once("generated/**".to_string()))
                .collect(),
        ),
        overrides: vec![OverrideSchema {
            pattern: "src/domain/**".to_string(),
            threshold: 8.0,
        }],
        views,
        language,
        output: OutputSchema {
            annotation_limit: Some(10),
            title: Some("My Project CRAP Report".to_string()),
            subtitle: Some("coverage + complexity gate".to_string()),
        },
    }
}

/// Every documented config-schema field, as `(label, doc)` pairs across
/// all schema structs (`ConfigSchema`, `OutputSchema`, `OverrideSchema`,
/// `ViewPresetSchema`, `LangSchema`). The doc-completeness test asserts
/// each `doc` reaches `render_example_config`'s output, so a field can
/// never render without its annotation. `label` is `"<Struct>.<field>"`
/// for diagnostics.
pub fn all_schema_field_docs() -> Vec<(String, &'static str)> {
    fn docs<T: DocumentedFields>(struct_name: &'static str) -> Vec<(String, &'static str)> {
        T::FIELD_NAMES
            .iter()
            .zip(T::FIELD_DOCS.iter())
            .map(|(name, doc)| (format!("{struct_name}.{name}"), *doc))
            .collect()
    }
    let mut out = Vec::new();
    out.extend(docs::<ConfigSchema>("ConfigSchema"));
    out.extend(docs::<OutputSchema>("OutputSchema"));
    out.extend(docs::<OverrideSchema>("OverrideSchema"));
    out.extend(docs::<ViewPresetSchema>("ViewPresetSchema"));
    out.extend(docs::<LangSchema>("LangSchema"));
    out
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Typed `ConfigError` contract (#340) ──────────────────────────
    //
    // The substring assertions scattered through this module pin the
    // user-facing Display text; these pin the *typed* shape — that
    // `parse_config` / `load_config` return concrete `ConfigError`
    // variants the caller can match on, not stringly-typed errors. The
    // two-layer design (I-3) is asserted explicitly: a path-less `Toml`
    // from `parse_config` wraps into a path-bearing `Parse` at
    // `load_config`, and the `Parse` Display names the file + "parse"
    // without flattening the source (thiserror Display does not walk
    // `#[source]`).

    #[test]
    fn parse_config_returns_typed_mutually_exclusive() {
        let err = parse_config("preset = \"strict\"\nthreshold = 10.0\n").unwrap_err();
        assert!(
            matches!(err, ConfigError::MutuallyExclusive { section: None }),
            "expected top-level MutuallyExclusive, got: {err:?}"
        );
    }

    #[test]
    fn parse_config_returns_typed_unknown_metric() {
        let err = parse_config("metric = \"halstead\"\n").unwrap_err();
        match err {
            ConfigError::UnknownMetric { value } => assert_eq!(value, "halstead"),
            other => panic!("expected UnknownMetric, got: {other:?}"),
        }
    }

    #[test]
    fn parse_config_returns_typed_toml_on_malformed() {
        let err = parse_config("this is not toml [[[").unwrap_err();
        assert!(
            matches!(err, ConfigError::Toml(_)),
            "expected Toml variant, got: {err:?}"
        );
    }

    #[test]
    fn load_config_wraps_parse_in_path_bearing_parse_variant() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crap.toml");
        std::fs::write(&path, "threshold = not_a_number\n").unwrap();

        let err = load_config(&path).unwrap_err();
        // Outer layer: path-bearing Parse whose Display names the file
        // and "parse" (the through-binary anyhow `{:#}` anchor).
        match &err {
            ConfigError::Parse { path: p, source } => {
                assert_eq!(p, &path);
                // Inner layer: the path-less Toml deserialize error.
                assert!(
                    matches!(**source, ConfigError::Toml(_)),
                    "Parse source should be Toml, got: {source:?}"
                );
            }
            other => panic!("expected Parse, got: {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("parse"), "Display must name 'parse': {msg}");
        assert!(
            msg.contains("crap.toml"),
            "Display must name the file: {msg}"
        );
    }

    #[test]
    fn load_config_missing_file_is_typed_read_error() {
        let err = load_config(Path::new("definitely-nonexistent-config.toml")).unwrap_err();
        assert!(
            matches!(err, ConfigError::Read { .. }),
            "expected Read variant, got: {err:?}"
        );
        assert!(err.to_string().contains("failed to read config file"));
    }

    // ── Tooling spike (walking-skeleton step 1) ──────────────────────
    //
    // Proves the schemars 1.x API the config-schema design relies on
    // works on the workspace MSRV: `#[derive(JsonSchema)]` emits a `///`
    // doc comment as the property `description`, and `schema_for!`
    // serializes to JSON. Kept as a permanent regression guard against a
    // schemars upgrade silently dropping the doc-comment → description
    // behavior the committed `crap.schema.json` depends on.
    #[test]
    fn schemars_renders_doc_comments_as_descriptions() {
        use schemars::{JsonSchema, schema_for};

        #[derive(JsonSchema)]
        #[allow(dead_code)]
        struct SpikeProbe {
            /// The probe's documented field.
            documented_field: u32,
        }

        let schema = schema_for!(SpikeProbe);
        let json = serde_json::to_string(&schema).expect("schema serializes to JSON");
        assert!(
            json.contains("The probe's documented field."),
            "schemars must render the `///` doc as the property description; got: {json}"
        );
    }

    // ── Tooling spike: documented 0.9.2 field-doc access ─────────────
    //
    // Proves the `documented` API `render_example_config` relies on works
    // on MSRV: `#[derive(DocumentedFields)]` exposes each field's `///`
    // text via `get_field_docs(name)`. Permanent regression guard against
    // a `documented` upgrade silently changing the accessor the example
    // emitter depends on.
    #[test]
    fn documented_exposes_field_docs_by_name() {
        use documented::DocumentedFields;

        #[derive(DocumentedFields)]
        #[allow(dead_code)]
        struct SpikeDoc {
            /// The documented spike field.
            spike_field: u32,
        }

        let doc = SpikeDoc::get_field_docs("spike_field")
            .expect("documented exposes the field's `///` doc by name");
        assert_eq!(doc, "The documented spike field.");
        assert!(
            SpikeDoc::get_field_docs("missing").is_err(),
            "unknown field name must error, not panic"
        );
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
threshold = 10.0
metric = "cyclomatic"
src = "crates"
exclude = ["tests/**", "benches/**"]

[[overrides]]
pattern = "domain/**"
threshold = 5.0

[[overrides]]
pattern = "adapters/**"
threshold = 15.0
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.threshold, Some(10.0));
        assert_eq!(config.metric, Some(ComplexityMetric::Cyclomatic));
        // Bare-string `src` form stays back-compatible: deserializes
        // through `SrcSpec::One` into a single-element list.
        assert_eq!(config.src, vec![PathBuf::from("crates")]);
        assert_eq!(
            config.exclude,
            Some(vec!["tests/**".to_string(), "benches/**".to_string()])
        );
        assert_eq!(config.overrides.len(), 2);
        assert_eq!(config.overrides[0].pattern, "domain/**");
        assert_eq!(config.overrides[0].threshold, 5.0);
        assert_eq!(config.overrides[1].pattern, "adapters/**");
        assert_eq!(config.overrides[1].threshold, 15.0);
    }

    #[test]
    fn parse_minimal_config() {
        let toml = "";
        let config = parse_config(toml).unwrap();
        assert_eq!(config.threshold, None);
        assert_eq!(config.metric, None);
        assert!(config.src.is_empty());
        assert_eq!(config.exclude, None);
        assert!(config.overrides.is_empty());
        assert!(config.language.is_empty());
    }

    // ── src string-or-array back-compat (#348, Gate C) ────────────

    #[test]
    fn parse_src_bare_string_stays_single_root() {
        // Back-compat: the long-standing `src = "string"` form must keep
        // deserializing — into a single-element root list.
        let config = parse_config(r#"src = "crates""#).unwrap();
        assert_eq!(config.src, vec![PathBuf::from("crates")]);
    }

    #[test]
    fn parse_src_array_collects_multi_roots() {
        let toml = r#"src = ["crates/a/src", "crates/b/src"]"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(
            config.src,
            vec![PathBuf::from("crates/a/src"), PathBuf::from("crates/b/src")]
        );
    }

    #[test]
    fn parse_src_absent_yields_empty_list() {
        let config = parse_config("threshold = 10.0\n").unwrap();
        assert!(config.src.is_empty());
    }

    // ── [output] title / subtitle (#348 AC4, Q3) ──────────────────

    #[test]
    fn parse_output_title_and_subtitle() {
        let toml = r#"
[output]
title = "Acme Coverage Report"
subtitle = "nightly build"
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.output.title.as_deref(), Some("Acme Coverage Report"));
        assert_eq!(config.output.subtitle.as_deref(), Some("nightly build"));
    }

    #[test]
    fn parse_output_title_subtitle_absent_by_default() {
        let config = parse_config("threshold = 10.0\n").unwrap();
        assert_eq!(config.output.title, None);
        assert_eq!(config.output.subtitle, None);
    }

    #[test]
    fn parse_output_title_alongside_annotation_limit() {
        let toml = r#"
[output]
annotation_limit = 7
title = "My Report"
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.output.annotation_limit, Some(7));
        assert_eq!(config.output.title.as_deref(), Some("My Report"));
    }

    // ── [language.<name>] map (#348 AC1/AC2) ──────────────────────

    #[test]
    fn parse_no_language_table_yields_empty_map() {
        let config = parse_config("threshold = 10.0\n").unwrap();
        assert!(config.language.is_empty());
    }

    #[test]
    fn parse_language_sections_keyed_by_name() {
        let toml = r#"
threshold = 20.0

[language.rust]
threshold = 8.0

[language.typescript]
threshold = 25.0
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.threshold, Some(20.0));
        assert_eq!(config.language.len(), 2);
        assert_eq!(config.language["rust"].threshold, Some(8.0));
        assert_eq!(config.language["typescript"].threshold, Some(25.0));
    }

    #[test]
    fn parse_language_section_full_field_set() {
        let toml = r#"
[language.rust]
preset = "strict"
metric = "cyclomatic"
exclude = ["generated/**"]
"#;
        let config = parse_config(toml).unwrap();
        let rust = &config.language["rust"];
        assert_eq!(rust.preset, Some(ThresholdPreset::Strict));
        assert_eq!(rust.metric, Some(ComplexityMetric::Cyclomatic));
        assert_eq!(
            rust.exclude.as_deref(),
            Some(&["generated/**".to_string()][..])
        );
        assert_eq!(rust.threshold, None);
    }

    #[test]
    fn parse_language_section_preset_and_threshold_mutually_exclusive() {
        let toml = r#"
[language.rust]
preset = "strict"
threshold = 8.0
"#;
        let err = parse_config(toml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("mutually exclusive"), "got: {msg}");
        assert!(
            msg.contains("language.rust"),
            "error must name the section, got: {msg}"
        );
    }

    #[test]
    fn parse_language_section_bad_metric_rejected() {
        let toml = r#"
[language.rust]
metric = "halstead"
"#;
        let err = parse_config(toml).unwrap_err();
        assert!(err.to_string().contains("unknown metric"), "got: {err}");
    }

    #[test]
    fn parse_language_section_unknown_field_rejected() {
        let toml = r#"
[language.rust]
threshold = 8.0
nonsense = true
"#;
        let err = parse_config(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown") || msg.contains("nonsense"),
            "expected deny_unknown_fields error, got: {msg}"
        );
    }

    // ── JSON Schema artifact (#348) ────────────────────────────────

    #[test]
    fn config_json_schema_emits_field_descriptions() {
        let schema = config_json_schema();
        // The `///` docs on ConfigSchema fields must surface as the
        // property descriptions schemars renders — this is what an
        // editor honoring `$schema` shows on hover/autocomplete.
        assert!(
            schema.contains("Custom numeric CRAP cutoff."),
            "threshold description missing from schema: {schema}"
        );
        assert!(
            schema.contains("Per-language override sections"),
            "language description missing from schema"
        );
        assert!(
            schema.contains("Scorecard title"),
            "output.title description missing from schema"
        );
    }

    #[test]
    fn config_json_schema_src_accepts_string_or_array() {
        // The untagged `SrcSpec` enum must surface as an `anyOf` of a
        // string and an array, so an editor accepts both `src = "x"` and
        // `src = ["x", "y"]`.
        let schema = config_json_schema();
        assert!(
            schema.contains("anyOf"),
            "schema missing anyOf for src: {schema}"
        );
        assert!(
            schema.contains("\"type\": \"array\""),
            "schema missing array type"
        );
    }

    #[test]
    fn config_json_schema_is_deterministic() {
        // The committed artifact + sync test rely on stable output.
        assert_eq!(config_json_schema(), config_json_schema());
    }

    #[test]
    fn parse_threshold_only() {
        let toml = "threshold = 12.5\n";
        let config = parse_config(toml).unwrap();
        assert_eq!(config.threshold, Some(12.5));
        assert_eq!(config.metric, None);
    }

    #[test]
    fn parse_overrides_only() {
        let toml = r#"
[[overrides]]
pattern = "core/**"
threshold = 3.0
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.threshold, None);
        assert_eq!(config.overrides.len(), 1);
    }

    #[test]
    fn parse_metric_cognitive() {
        let toml = r#"metric = "cognitive""#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.metric, Some(ComplexityMetric::Cognitive));
    }

    #[test]
    fn parse_metric_cyclomatic() {
        let toml = r#"metric = "cyclomatic""#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.metric, Some(ComplexityMetric::Cyclomatic));
    }

    #[test]
    fn invalid_metric_rejected() {
        let toml = r#"metric = "halstead""#;
        let err = parse_config(toml).unwrap_err();
        assert!(err.to_string().contains("unknown metric"));
    }

    #[test]
    fn negative_threshold_rejected() {
        let toml = "threshold = -5.0\n";
        let err = parse_config(toml).unwrap_err();
        assert!(err.to_string().contains("finite positive"));
    }

    #[test]
    fn zero_threshold_rejected() {
        let toml = "threshold = 0.0\n";
        let err = parse_config(toml).unwrap_err();
        assert!(err.to_string().contains("finite positive"));
    }

    #[test]
    fn inf_threshold_rejected() {
        let toml = "threshold = inf\n";
        let err = parse_config(toml).unwrap_err();
        assert!(err.to_string().contains("finite positive"));
    }

    #[test]
    fn negative_override_threshold_rejected() {
        let toml = r#"
[[overrides]]
pattern = "src/**"
threshold = -1.0
"#;
        let err = parse_config(toml).unwrap_err();
        assert!(err.to_string().contains("finite positive"));
    }

    #[test]
    fn unknown_field_rejected() {
        let toml = "unknown_key = true\n";
        let err = parse_config(toml).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn malformed_toml_rejected() {
        let toml = "this is not toml [[[";
        assert!(parse_config(toml).is_err());
    }

    #[test]
    fn zero_override_threshold_rejected() {
        let toml = r#"
[[overrides]]
pattern = "src/**"
threshold = 0.0
"#;
        let err = parse_config(toml).unwrap_err();
        assert!(err.to_string().contains("finite positive"));
    }

    #[test]
    fn parse_preset_strict() {
        let config = parse_config(r#"preset = "strict""#).unwrap();
        assert_eq!(config.preset, Some(ThresholdPreset::Strict));
        assert_eq!(config.threshold, None);
    }

    #[test]
    fn parse_preset_default() {
        let config = parse_config(r#"preset = "default""#).unwrap();
        assert_eq!(config.preset, Some(ThresholdPreset::Default));
    }

    #[test]
    fn parse_preset_lenient() {
        let config = parse_config(r#"preset = "lenient""#).unwrap();
        assert_eq!(config.preset, Some(ThresholdPreset::Lenient));
    }

    #[test]
    fn preset_and_threshold_mutually_exclusive() {
        let toml = "preset = \"strict\"\nthreshold = 10.0\n";
        let err = parse_config(toml).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn unknown_preset_rejected() {
        let err = parse_config(r#"preset = "extreme""#).unwrap_err();
        assert!(err.to_string().contains("unknown preset"));
    }

    #[test]
    fn load_config_missing_file() {
        let err = load_config(Path::new("nonexistent.toml")).unwrap_err();
        assert!(err.to_string().contains("failed to read config file"));
    }

    #[test]
    fn load_config_valid_file() {
        // `load_config` is name-agnostic — it loads whatever path it is
        // handed. The unified canonical name keeps this in step with the
        // config-ast-purity gate (crap-rs#342).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crap.toml");
        std::fs::write(&path, "threshold = 10.0\n").unwrap();

        let config = load_config(&path).unwrap();
        assert_eq!(config.threshold, Some(10.0));
    }

    #[test]
    fn load_config_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crap.toml");
        std::fs::write(&path, "not valid toml [[[").unwrap();

        let err = load_config(&path).unwrap_err();
        assert!(err.to_string().contains("failed to parse config file"));
    }

    // ── ViewPreset tests ───────────────────────────────────

    #[test]
    fn parse_no_views_table_yields_empty_map() {
        // Back-compat: existing TOML with no `[views]` blocks must continue
        // to parse with `views == HashMap::new()`.
        let config = parse_config("threshold = 10.0\n").unwrap();
        assert_eq!(config.threshold, Some(10.0));
        assert!(config.views.is_empty());
    }

    #[test]
    fn parse_empty_view_block_yields_default_preset() {
        let toml = "[views.ci]\n";
        let config = parse_config(toml).unwrap();
        assert_eq!(config.views.len(), 1);
        let ci = config.views.get("ci").expect("preset `ci` parsed");
        assert_eq!(*ci, ViewPreset::default());
    }

    #[test]
    fn parse_full_view_block_parses_every_field() {
        let toml = r#"
[views.ci]
top = 20
min_coverage = 0
max_coverage = 90
sort = "coverage"
only_failing = true
no_fail = false
group_by = "file"
minimal_view = true
"#;
        let config = parse_config(toml).unwrap();
        let ci = config.views.get("ci").expect("preset `ci` parsed");
        assert_eq!(ci.top, Some(20));
        assert_eq!(ci.min_coverage, Some(0.0));
        assert_eq!(ci.max_coverage, Some(90.0));
        assert_eq!(ci.sort, Some(SortKey::Coverage));
        assert_eq!(ci.only_failing, Some(true));
        assert_eq!(ci.no_fail, Some(false));
        assert_eq!(ci.group_by, Some(GroupKey::File));
        assert_eq!(ci.minimal_view, Some(true));
    }

    #[test]
    fn parse_unknown_view_field_rejected() {
        let toml = r#"
[views.ci]
top = 5
diff_ref = "main"
"#;
        let err = parse_config(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown") || msg.contains("diff_ref"),
            "expected deny_unknown_fields error, got: {msg}"
        );
    }

    #[test]
    fn parse_bad_sort_string_rejected() {
        let toml = r#"
[views.ci]
sort = "nonsense"
"#;
        let err = parse_config(toml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown sort"), "got: {msg}");
        assert!(msg.contains("ci"), "error must name preset, got: {msg}");
    }

    #[test]
    fn parse_bad_group_by_string_rejected() {
        let toml = r#"
[views.ci]
group_by = "module"
"#;
        let err = parse_config(toml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown group_by"), "got: {msg}");
        assert!(msg.contains("ci"), "error must name preset, got: {msg}");
    }

    #[test]
    fn parse_multiple_view_presets_independent() {
        let toml = r#"
[views.ci]
top = 20
sort = "coverage"

[views.investigate]
top = 10
sort = "complexity"
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.views.len(), 2);
        let ci = config.views.get("ci").unwrap();
        assert_eq!(ci.top, Some(20));
        assert_eq!(ci.sort, Some(SortKey::Coverage));
        let inv = config.views.get("investigate").unwrap();
        assert_eq!(inv.top, Some(10));
        assert_eq!(inv.sort, Some(SortKey::Complexity));
    }

    #[test]
    fn parse_view_preset_top_zero_accepted() {
        // `top = 0` is canonicalised to `None` by `build_view_spec` (per
        // existing CLI semantic — see `cli/view_args.rs::build_view_spec`).
        // Config-load must accept the value rather than reject it.
        let toml = r#"
[views.ci]
top = 0
"#;
        let config = parse_config(toml).unwrap();
        let ci = config.views.get("ci").unwrap();
        assert_eq!(ci.top, Some(0));
    }

    #[test]
    fn parse_view_preset_min_coverage_out_of_range_rejected() {
        let toml = r#"
[views.ci]
min_coverage = -1
"#;
        let err = parse_config(toml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("out of range"), "got: {msg}");
        assert!(msg.contains("ci"), "error must name preset, got: {msg}");
    }

    #[test]
    fn parse_view_preset_max_coverage_out_of_range_rejected() {
        let toml = r#"
[views.ci]
max_coverage = 105
"#;
        let err = parse_config(toml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("out of range"), "got: {msg}");
    }

    #[test]
    fn parse_view_preset_min_exceeds_max_rejected() {
        let toml = r#"
[views.ci]
min_coverage = 90
max_coverage = 30
"#;
        let err = parse_config(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must not exceed") || msg.contains("exceeds"),
            "got: {msg}"
        );
        assert!(msg.contains("ci"), "error must name preset, got: {msg}");
    }

    #[test]
    fn parse_view_preset_min_only_resolves_to_full_upper_bound() {
        // `min_coverage = 50` alone (no `max_coverage`) is valid because
        // the absent side defaults to 100 — mirrors CLI semantics in
        // `cli::view_args::resolve_coverage_bounds`.
        let toml = r#"
[views.ci]
min_coverage = 50
"#;
        let config = parse_config(toml).unwrap();
        let ci = config.views.get("ci").unwrap();
        assert_eq!(ci.min_coverage, Some(50.0));
        assert_eq!(ci.max_coverage, None);
    }

    #[test]
    fn parse_view_preset_alongside_threshold() {
        // Existing top-level fields and view presets coexist.
        let toml = r#"
threshold = 12.0

[views.ci]
top = 20
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.threshold, Some(12.0));
        assert_eq!(config.views.len(), 1);
        assert_eq!(config.views["ci"].top, Some(20));
    }

    #[test]
    fn parse_view_preset_all_sort_variants() {
        let toml = r#"
[views.crap_sort]
sort = "crap"

[views.coverage_sort]
sort = "coverage"

[views.complexity_sort]
sort = "complexity"

[views.path_sort]
sort = "path"
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.views["crap_sort"].sort, Some(SortKey::Crap));
        assert_eq!(config.views["coverage_sort"].sort, Some(SortKey::Coverage));
        assert_eq!(
            config.views["complexity_sort"].sort,
            Some(SortKey::Complexity)
        );
        assert_eq!(config.views["path_sort"].sort, Some(SortKey::Path));
    }

    // ── OutputConfig tests ───────────────────────────────────────

    #[test]
    fn parse_no_output_table_yields_default_output_config() {
        // Back-compat: every existing crap4rs.toml lacks `[output]` and
        // must continue to parse with the new field defaulted.
        let config = parse_config("threshold = 10.0\n").unwrap();
        assert_eq!(config.output, OutputConfig::default());
        assert_eq!(config.output.annotation_limit, None);
    }

    #[test]
    fn parse_output_annotation_limit() {
        let toml = "[output]\nannotation_limit = 25\n";
        let config = parse_config(toml).unwrap();
        assert_eq!(config.output.annotation_limit, Some(25));
    }

    #[test]
    fn parse_output_alongside_threshold() {
        let toml = r#"
threshold = 12.0

[output]
annotation_limit = 7
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.threshold, Some(12.0));
        assert_eq!(config.output.annotation_limit, Some(7));
    }

    #[test]
    fn parse_output_annotation_limit_zero_rejected() {
        // 0 would silently disable annotation emission (the reporter
        // takes 0 of the eligible set + only emits the truncation
        // notice). Clap rejects 0 at the CLI boundary via
        // `value_parser!(u32).range(1..=100)`; config must match.
        let toml = "[output]\nannotation_limit = 0\n";
        let err = parse_config(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("annotation_limit") && msg.contains("1..=100"),
            "expected range error, got: {msg}"
        );
    }

    #[test]
    fn parse_output_annotation_limit_above_max_rejected() {
        // 101+ would silently flood the GH Actions per-step UI cap
        // (10 warning per step; anything past 10 is dropped by the
        // runner). Clap rejects at the CLI; config must match.
        let toml = "[output]\nannotation_limit = 101\n";
        let err = parse_config(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("annotation_limit") && msg.contains("1..=100"),
            "expected range error, got: {msg}"
        );
    }

    #[test]
    fn parse_output_annotation_limit_boundary_values_accepted() {
        for v in [1u32, 10, 50, 100] {
            let toml = format!("[output]\nannotation_limit = {v}\n");
            let config = parse_config(&toml).expect("boundary value should parse");
            assert_eq!(config.output.annotation_limit, Some(v));
        }
    }

    #[test]
    fn parse_unknown_output_field_rejected() {
        // deny_unknown_fields guards forward-compat: a TOML pinned at
        // an old crap4rs version must still surface unrecognised output
        // settings as load-time errors rather than silently dropping
        // them.
        let toml = r#"
[output]
annotation_limit = 5
nonsense_field = "x"
"#;
        let err = parse_config(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown") || msg.contains("nonsense_field"),
            "expected deny_unknown_fields error, got: {msg}"
        );
    }

    // ── discover_config walk-upward + within-dir ordered discovery ────
    //
    // `discover_config(start, file_names)` absolutizes `start`, walks its
    // ancestor directories, and within each directory resolves the
    // ordered `file_names` (index 0 canonical, the rest legacy
    // fallbacks). The first ancestor directory yielding any candidate
    // wins. These cases pin the WITHIN-DIR contract (the same-dir
    // back-compat the single-directory loader had): a canonical file
    // wins; a lone legacy file is discovered at `used_index > 0`; a
    // co-present legacy file is reported as `shadowed`; an empty tree
    // discovers nothing; a directory at a name is skipped; a non-NotFound
    // I/O error bails. The CROSS-DIR contract (walk-upward, nearest-dir
    // wins) is pinned by the dedicated ancestor / nearest-wins cases
    // below. The synthetic adapter names keep the analyzer's own
    // per-adapter literals (`crap4rs.toml` / `crap4ts.toml`) out of this
    // module per the config-ast-purity gate (crap-rs#342) — discovery is
    // name-agnostic, so synthetic names exercise the identical code path.
    //
    // `start` is the tempdir itself in the same-dir cases, so the winning
    // directory is the first ancestor and no climb occurs. Tempdir-rooted
    // (absolute) starts keep this safe under nextest's parallel execution
    // model (no process-wide CWD dependence).

    /// Canonical adapter config name used in the discovery unit tests —
    /// synthetic so the per-adapter analyzer names stay out of this
    /// module (crap-rs#342). The unified `crap.toml` and the synthetic
    /// legacy below exercise the same name-agnostic discovery path.
    const TEST_CANONICAL: &str = "crap.toml";
    /// Synthetic legacy fallback name (index 1) for the discovery tests.
    const TEST_LEGACY: &str = "test-adapter-legacy.toml";
    const TEST_NAMES: &[&str] = &[TEST_CANONICAL, TEST_LEGACY];

    #[test]
    fn discover_config_canonical_only_wins_at_index_zero() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().join(TEST_CANONICAL);
        std::fs::write(&canonical, "threshold = 22.0\n").unwrap();

        let disc = discover_config(dir.path(), TEST_NAMES).unwrap().unwrap();

        assert_eq!(disc.path, canonical);
        assert_eq!(disc.used_index, 0, "canonical name is index 0");
        assert!(
            disc.shadowed.is_empty(),
            "no legacy file present, nothing shadowed"
        );
    }

    #[test]
    fn discover_config_legacy_only_falls_back_at_index_one() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(TEST_LEGACY);
        std::fs::write(&legacy, "threshold = 9.0\n").unwrap();

        // Canonical (index 0) is absent → discovery advances to the legacy
        // fallback at index 1 within the same directory.
        let disc = discover_config(dir.path(), TEST_NAMES).unwrap().unwrap();

        assert_eq!(disc.path, legacy);
        assert_eq!(disc.used_index, 1, "legacy fallback is index 1");
        assert!(
            disc.shadowed.is_empty(),
            "the winner cannot shadow itself; nothing lower-priority exists"
        );
    }

    #[test]
    fn discover_config_both_present_canonical_wins_legacy_shadowed() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().join(TEST_CANONICAL);
        let legacy = dir.path().join(TEST_LEGACY);
        std::fs::write(&canonical, "threshold = 22.0\n").unwrap();
        std::fs::write(&legacy, "threshold = 9.0\n").unwrap();

        let disc = discover_config(dir.path(), TEST_NAMES).unwrap().unwrap();

        assert_eq!(disc.path, canonical, "canonical wins by name order");
        assert_eq!(disc.used_index, 0);
        assert_eq!(
            disc.shadowed,
            vec![legacy],
            "the co-present legacy file is reported as shadowed"
        );
    }

    #[test]
    fn discover_config_neither_present_returns_none() {
        // An empty tempdir tree (its ancestors are real but carry no
        // candidate up to the root). Discovery returns None rather than
        // climbing into an unrelated config — the tempdir lives under the
        // system temp root, which has no config.
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(
            discover_config(dir.path(), TEST_NAMES).unwrap(),
            None,
            "no candidate in the tree → no config discovered"
        );
    }

    #[test]
    fn discover_config_non_file_at_canonical_advances_to_legacy() {
        // A non-regular file at the canonical position (a directory at the
        // canonical name) is not a config we can load, so discovery
        // advances to the legacy fallback within the same directory rather
        // than treating the directory as the winner or erroring. Pins the
        // `Ok(_) => continue` arm.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(TEST_CANONICAL)).unwrap();
        let legacy = dir.path().join(TEST_LEGACY);
        std::fs::write(&legacy, "threshold = 9.0\n").unwrap();

        let disc = discover_config(dir.path(), TEST_NAMES).unwrap().unwrap();

        assert_eq!(disc.path, legacy, "directory at index 0 is skipped");
        assert_eq!(disc.used_index, 1);
        assert!(disc.shadowed.is_empty());
    }

    #[test]
    fn discover_config_non_file_shadow_candidate_is_not_recorded() {
        // A lower-priority candidate that exists but is not a regular file
        // (a directory at the legacy name) must NOT appear in `shadowed` —
        // the filter is `is_file()`, not `exists()`, so the operator is
        // never told to "remove" a directory. Pins the shadow filter.
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().join(TEST_CANONICAL);
        std::fs::write(&canonical, "threshold = 22.0\n").unwrap();
        std::fs::create_dir(dir.path().join(TEST_LEGACY)).unwrap();

        let disc = discover_config(dir.path(), TEST_NAMES).unwrap().unwrap();

        assert_eq!(disc.path, canonical);
        assert_eq!(disc.used_index, 0);
        assert!(
            disc.shadowed.is_empty(),
            "a directory at a lower-priority candidate must not be reported as shadowed"
        );
    }

    #[test]
    #[cfg(unix)]
    fn discover_config_permission_error_short_circuits_no_legacy_fallthrough() {
        // The load-bearing contract: a non-`NotFound` I/O error (here
        // `PermissionDenied`) probing a candidate must bail, never
        // silently fall through to a co-present legacy file. Without this,
        // an unreadable canonical config would be masked by a stale legacy
        // file and the operator would get a confusing legacy deprecation
        // warning instead of a permissions error.
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().join(TEST_CANONICAL);
        std::fs::write(&canonical, "threshold = 22.0\n").unwrap();
        let legacy = dir.path().join(TEST_LEGACY);
        std::fs::write(&legacy, "threshold = 9.0\n").unwrap();

        // Block traversal of the dir so `metadata(canonical)` yields
        // PermissionDenied (chmod 000 on the file itself still permits
        // `metadata`, which reads the inode, not contents).
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o000)).unwrap();

        // If perms didn't actually take effect (e.g. running as root, which
        // bypasses the check), this branch is untestable here — restore and
        // skip rather than emit a false failure.
        let still_readable = std::fs::metadata(&canonical).is_ok();
        let result = discover_config(dir.path(), TEST_NAMES);
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        if still_readable {
            return;
        }
        let err = result.expect_err("a permission error must surface, not fall through");
        assert!(
            err.to_string().contains("cannot access config file"),
            "got: {err}"
        );
    }

    // ── walk-upward cross-dir contract (crap-rs#339) ──────────────────

    #[test]
    fn discover_config_finds_config_in_an_ancestor_directory() {
        // The walk-upward core: a config in a *parent* of `start` is
        // found when `start` itself has none. `start` is an absolute
        // nested tempdir path, so `.ancestors()` climbs to the parent
        // holding the config. (The relative-`start`-from-a-nested-CWD
        // case — which is what truly exercises the `std::path::absolute`
        // C-1 fix — lives in the subprocess regression test in
        // crap4rs/tests/config_discovery_integration.rs, because a unit
        // test cannot control the process CWD parallel-safely.)
        let root = tempfile::tempdir().unwrap();
        let canonical = root.path().join(TEST_CANONICAL);
        std::fs::write(&canonical, "threshold = 22.0\n").unwrap();
        let nested = root.path().join("crates").join("sub");
        std::fs::create_dir_all(&nested).unwrap();

        let disc = discover_config(&nested, TEST_NAMES).unwrap().unwrap();

        assert_eq!(
            disc.path, canonical,
            "discovery must climb to the ancestor config"
        );
        assert_eq!(disc.used_index, 0);
    }

    #[test]
    fn discover_config_nearest_dir_wins_nearer_legacy_beats_farther_canonical() {
        // I-2: "first ancestor dir with any candidate wins, ordered names
        // within that dir." A nearer legacy file beats a farther canonical
        // — the nearest directory holding ANY candidate stops the walk, so
        // its (legacy) file wins even though a canonical exists higher up.
        // This is NEW behavior vs the within-dir canonical-over-legacy
        // guarantee, and is a conscious nearest-wins decision (recorded in
        // the closeout ADR). Shadow detection stays same-dir: the farther
        // canonical is NOT reported as shadowed (no cross-dir "safe to
        // remove" notice).
        let root = tempfile::tempdir().unwrap();
        let far_canonical = root.path().join(TEST_CANONICAL);
        std::fs::write(&far_canonical, "threshold = 22.0\n").unwrap();
        let child = root.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let near_legacy = child.join(TEST_LEGACY);
        std::fs::write(&near_legacy, "threshold = 9.0\n").unwrap();

        let disc = discover_config(&child, TEST_NAMES).unwrap().unwrap();

        assert_eq!(
            disc.path, near_legacy,
            "the nearer legacy file wins over the farther canonical"
        );
        assert_eq!(disc.used_index, 1, "the winner is the legacy name");
        assert!(
            disc.shadowed.is_empty(),
            "the farther canonical is in a different dir — never reported as shadowed"
        );
    }
}
