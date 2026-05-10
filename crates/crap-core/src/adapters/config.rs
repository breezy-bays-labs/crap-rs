//! Config file adapter — loads `crap4rs.toml` and converts to domain types.
//!
//! Handles TOML parsing and config file discovery. All CLI-representable
//! options are supported. Per-path threshold overrides use glob patterns.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::domain::threshold::{ThresholdOverride, ThresholdPreset, is_valid_threshold};
use crate::domain::types::ComplexityMetric;
use crate::domain::view::{CoverageRange, CoverageRangeError, GroupKey, SortKey};

// ── Public config type (adapter output) ────────────────────────────

/// Parsed configuration from a TOML file.
///
/// All fields are optional — missing fields mean "use CLI default."
/// The CLI layer merges this with command-line flags.
#[derive(Debug, Clone, Default)]
pub struct FileConfig {
    pub threshold: Option<f64>,
    pub preset: Option<ThresholdPreset>,
    pub metric: Option<ComplexityMetric>,
    pub src: Option<PathBuf>,
    pub exclude: Option<Vec<String>>,
    pub overrides: Vec<ThresholdOverride>,
    /// Saved view presets keyed by preset name (issue #80).
    ///
    /// Each `[views.<name>]` block in TOML deserializes into a
    /// [`ViewPreset`]; the CLI layer resolves `--view <name>` against
    /// this map and folds preset values into `Cli` before
    /// `build_view_spec`.
    pub views: HashMap<String, ViewPreset>,
}

/// Saved view preset (issue #80).
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

// ── TOML serde types (private) ─────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    threshold: Option<f64>,
    preset: Option<String>,
    metric: Option<String>,
    src: Option<String>,
    exclude: Option<Vec<String>>,
    #[serde(default)]
    overrides: Vec<RawOverride>,
    #[serde(default)]
    views: HashMap<String, RawViewPreset>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOverride {
    pattern: String,
    threshold: f64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawViewPreset {
    top: Option<u32>,
    min_coverage: Option<f64>,
    max_coverage: Option<f64>,
    sort: Option<String>,
    only_failing: Option<bool>,
    no_fail: Option<bool>,
    group_by: Option<String>,
    minimal_view: Option<bool>,
}

// ── Public API ─────────────────────────────────────────────────────

/// Default config file name.
pub const CONFIG_FILE_NAME: &str = "crap4rs.toml";

/// Discover the config file in the current working directory.
///
/// Returns `Ok(Some(path))` if `crap4rs.toml` exists, `Ok(None)` if absent.
/// Returns `Err` on permission errors or other filesystem failures.
pub fn discover_config() -> Result<Option<PathBuf>> {
    let path = PathBuf::from(CONFIG_FILE_NAME);
    match std::fs::metadata(&path) {
        Ok(m) if m.is_file() => Ok(Some(path)),
        Ok(_) => Ok(None), // exists but not a file (directory, symlink to dir, etc.)
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => anyhow::bail!(
            "cannot access config file {}: {e}\n  hint: check file permissions",
            path.display()
        ),
    }
}

/// Load and parse a config file from the given path.
pub fn load_config(path: &Path) -> Result<FileConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;
    parse_config(&content)
        .with_context(|| format!("failed to parse config file: {}", path.display()))
}

/// Parse TOML content into a `FileConfig`.
fn parse_config(content: &str) -> Result<FileConfig> {
    let raw: RawConfig = toml::from_str(content)?;
    validate_raw_config(&raw)?;

    let metric = raw.metric.as_deref().map(parse_metric).transpose()?;
    let preset = raw.preset.as_deref().map(parse_preset).transpose()?;

    let overrides = raw
        .overrides
        .into_iter()
        .map(|o| ThresholdOverride {
            pattern: o.pattern,
            threshold: o.threshold,
        })
        .collect();

    let views = raw
        .views
        .into_iter()
        .map(|(name, raw_preset)| {
            let preset = parse_view_preset(&name, raw_preset)?;
            Ok::<_, anyhow::Error>((name, preset))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    Ok(FileConfig {
        threshold: raw.threshold,
        preset,
        metric,
        src: raw.src.map(PathBuf::from),
        exclude: raw.exclude,
        overrides,
        views,
    })
}

fn validate_raw_config(raw: &RawConfig) -> Result<()> {
    if raw.preset.is_some() && raw.threshold.is_some() {
        anyhow::bail!("preset and threshold are mutually exclusive in config");
    }
    if let Some(t) = raw.threshold
        && !is_valid_threshold(t)
    {
        anyhow::bail!("threshold must be a finite positive number, got: {t}");
    }
    for o in &raw.overrides {
        if !is_valid_threshold(o.threshold) {
            anyhow::bail!(
                "override threshold must be a finite positive number, got: {} (pattern: {})",
                o.threshold,
                o.pattern
            );
        }
    }
    Ok(())
}

fn parse_view_preset(name: &str, raw: RawViewPreset) -> Result<ViewPreset> {
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

fn parse_sort_key(preset_name: &str, s: &str) -> Result<SortKey> {
    match s {
        "crap" => Ok(SortKey::Crap),
        "coverage" => Ok(SortKey::Coverage),
        "complexity" => Ok(SortKey::Complexity),
        "path" => Ok(SortKey::Path),
        other => anyhow::bail!(
            "preset `{preset_name}`: unknown sort: {other}\n  valid values: crap, coverage, complexity, path"
        ),
    }
}

fn parse_group_key(preset_name: &str, s: &str) -> Result<GroupKey> {
    match s {
        "file" => Ok(GroupKey::File),
        other => {
            anyhow::bail!("preset `{preset_name}`: unknown group_by: {other}\n  valid values: file")
        }
    }
}

/// Validate the preset's coverage bounds in isolation (fail-fast at config
/// load per issue #80). Either-side-only is allowed and the absent side is
/// defaulted to `0` / `100` for the relational check, mirroring CLI
/// `validate_view_args` so a preset that would resolve to an invalid range
/// is rejected at TOML parse time rather than at `--view` resolution.
fn validate_preset_coverage_range(
    preset_name: &str,
    min: Option<f64>,
    max: Option<f64>,
) -> Result<()> {
    if min.is_none() && max.is_none() {
        return Ok(());
    }
    let lo = min.unwrap_or(0.0);
    let hi = max.unwrap_or(100.0);
    // `CoverageRangeError` is `#[non_exhaustive]` paused per D10
    // amendment (#147 restores at v1.0). Now that this adapter lives in
    // crap-core alongside the enum, the match is in-crate and
    // exhaustive — no wildcard arm needed. v1.0 new variants will
    // require an explicit arm here.
    match CoverageRange::new(lo, hi) {
        Ok(_) => Ok(()),
        Err(CoverageRangeError::OutOfRange { value }) => anyhow::bail!(
            "preset `{preset_name}`: coverage value out of range: {value}\n  valid range: [0, 100]"
        ),
        Err(CoverageRangeError::MinExceedsMax { min, max }) => anyhow::bail!(
            "preset `{preset_name}`: min_coverage ({min}) must not exceed max_coverage ({max})"
        ),
    }
}

fn parse_preset(s: &str) -> Result<ThresholdPreset> {
    match s {
        "strict" => Ok(ThresholdPreset::Strict),
        "default" => Ok(ThresholdPreset::Default),
        "lenient" => Ok(ThresholdPreset::Lenient),
        other => anyhow::bail!("unknown preset: {other}\n  valid values: strict, default, lenient"),
    }
}

fn parse_metric(s: &str) -> Result<ComplexityMetric> {
    match s {
        "cognitive" => Ok(ComplexityMetric::Cognitive),
        "cyclomatic" => Ok(ComplexityMetric::Cyclomatic),
        other => anyhow::bail!("unknown metric: {other}\n  valid values: cognitive, cyclomatic"),
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(config.src, Some(PathBuf::from("crates")));
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
        assert_eq!(config.src, None);
        assert_eq!(config.exclude, None);
        assert!(config.overrides.is_empty());
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
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crap4rs.toml");
        std::fs::write(&path, "threshold = 10.0\n").unwrap();

        let config = load_config(&path).unwrap();
        assert_eq!(config.threshold, Some(10.0));
    }

    #[test]
    fn load_config_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crap4rs.toml");
        std::fs::write(&path, "not valid toml [[[").unwrap();

        let err = load_config(&path).unwrap_err();
        assert!(err.to_string().contains("failed to parse config file"));
    }

    // ── ViewPreset tests (issue #80) ───────────────────────────────────

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
}
