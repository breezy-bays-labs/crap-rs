//! Config file adapter — loads `crap4rs.toml` and converts to domain types.
//!
//! Handles TOML parsing and config file discovery. All CLI-representable
//! options are supported. Per-path threshold overrides use glob patterns.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::domain::threshold::{ThresholdOverride, ThresholdPreset, is_valid_threshold};
use crate::domain::types::ComplexityMetric;

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
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOverride {
    pattern: String,
    threshold: f64,
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

    Ok(FileConfig {
        threshold: raw.threshold,
        preset,
        metric,
        src: raw.src.map(PathBuf::from),
        exclude: raw.exclude,
        overrides,
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
}
