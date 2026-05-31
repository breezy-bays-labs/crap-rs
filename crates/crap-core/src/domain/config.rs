//! Parsed configuration types — the **parsed projection** the analyzer
//! consumes after a config file is loaded and validated.
//!
//! These are pure domain POD types: their fields are domain types
//! (`ThresholdPreset`, `ComplexityMetric`, `PathBuf`, …), language- and
//! format-agnostic, built imperatively by the adapter's `parse_config`.
//! They are deliberately distinct from the adapter's wire/schema family
//! (the `Deserialize + JsonSchema` types that describe `crap.toml`
//! verbatim and stay in the adapters layer): the wire types hold values
//! *as the user types them* (`preset = "strict"` is a string), while
//! these hold the *parsed* values the rest of crap-core operates on.
//!
//! They live in `domain/` (not adapters) because they are the shape the
//! language-agnostic core merges and analyzes — every adapter, Rust or
//! TypeScript, produces the same `FileConfig`. The adapter layer
//! re-exports them so existing consumers compile unchanged.

use std::collections::HashMap;
use std::path::PathBuf;

use super::threshold::{ThresholdOverride, ThresholdPreset};
use super::types::ComplexityMetric;
use super::view::{GroupKey, SortKey};

/// Parsed configuration from a TOML file.
///
/// All fields are optional — missing fields mean "use CLI default."
/// The CLI layer merges this with command-line flags.
///
/// This is the **parsed projection**: its fields are domain types
/// (`ThresholdPreset`, `ComplexityMetric`, `PathBuf`, …), the shape the
/// rest of crap-core consumes. It is deliberately distinct from the
/// adapter's wire/schema type (the `Deserialize + JsonSchema` config
/// schema that lives in the adapters layer and describes `crap.toml`
/// verbatim): the wire type holds values as the user types them, this
/// holds the parsed values the analyzer operates on.
#[derive(Debug, Clone, Default)]
pub struct FileConfig {
    pub threshold: Option<f64>,
    pub preset: Option<ThresholdPreset>,
    pub metric: Option<ComplexityMetric>,
    /// Source roots the analyzer walks, in declaration order.
    ///
    /// Empty means "unset, defer to CLI / the `["src"]` default." A
    /// single root keeps src-relative identity (byte-identical
    /// back-compat); multiple roots key git-toplevel-relative (D18,
    /// enforced downstream by `cli::resolve_identity_base`). The TOML
    /// `src` key accepts either a bare string (`src = "crates"`) or an
    /// array (`src = ["a", "b"]`) — both deserialize into this list.
    pub src: Vec<PathBuf>,
    pub exclude: Option<Vec<String>>,
    pub overrides: Vec<ThresholdOverride>,
    /// Saved view presets keyed by preset name.
    ///
    /// Each `[views.<name>]` block in TOML deserializes into a
    /// [`ViewPreset`]; the CLI layer resolves `--view <name>` against
    /// this map and folds preset values into `Cli` before
    /// `build_view_spec`.
    pub views: HashMap<String, ViewPreset>,
    /// Per-language override sections keyed by language name.
    ///
    /// Each `[language.<name>]` block in TOML deserializes into a
    /// [`LangConfig`]; the CLI layer selects the running adapter's
    /// section via `AdapterMeta::config_lang_key` and overlays it over
    /// the shared top-level defaults (per-language wins). Languages the
    /// adapter doesn't recognize are simply never selected.
    pub language: HashMap<String, LangConfig>,
    /// Output-shaping settings under `[output]`.
    ///
    /// Reporter-specific knobs (`annotation_limit`, `title`,
    /// `subtitle`) live here so they share a single TOML namespace
    /// rather than polluting the top-level table. Missing `[output]`
    /// blocks deserialize to `OutputConfig::default()`.
    pub output: OutputConfig,
}

/// Reporter-level output settings (TOML `[output]` table).
///
/// All fields are optional — missing fields mean "use CLI default."
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutputConfig {
    /// Cap on the number of `::warning` annotations emitted by the
    /// `github-annotations` reporter per invocation. `None` defers to
    /// the CLI default (10); a CLI `--annotation-limit` flag always
    /// wins over this value when both are set.
    pub annotation_limit: Option<u32>,
    /// Scorecard title — a single header label for the whole report.
    /// `None` renders the default unlabeled header.
    pub title: Option<String>,
    /// Scorecard subtitle, rendered beneath the title. `None` emits no
    /// subtitle line.
    pub subtitle: Option<String>,
}

/// Per-language config overrides (TOML `[language.<name>]` table).
///
/// A language section may assert any subset of the shared top-level
/// knobs that are meaningful per-language; the CLI layer overlays a
/// `Some` here over the inherited shared value. `preset` and
/// `threshold` remain mutually exclusive — a section asserting both is
/// rejected at parse time, the same as the top level.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LangConfig {
    pub threshold: Option<f64>,
    pub preset: Option<ThresholdPreset>,
    pub metric: Option<ComplexityMetric>,
    pub exclude: Option<Vec<String>>,
}

/// Saved view preset.
///
/// All fields are optional — `None` means "preset does not assert this
/// field, defer to CLI / defaults." Booleans are `Option<bool>` so the
/// preset can distinguish "absent" from "explicitly false," which lets
/// the CLI layer treat a CLI bool of `false` as "user didn't say"
/// (OR-merge semantics — see `apply_preset_to_cli`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewPreset {
    pub top: Option<u32>,
    pub min_coverage: Option<f64>,
    pub max_coverage: Option<f64>,
    pub sort: Option<SortKey>,
    pub only_failing: Option<bool>,
    pub no_fail: Option<bool>,
    pub group_by: Option<GroupKey>,
    pub minimal_view: Option<bool>,
}
