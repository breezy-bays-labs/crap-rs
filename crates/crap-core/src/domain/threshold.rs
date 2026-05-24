use super::types::ComplexityMetric;

// ── Threshold calibration table ──────────────────────────────────────
//
// CRAP thresholds are aligned with the intrinsic risk classification
// in `crap.rs::classify_risk`: every preset fires at the next
// risk-tier boundary up.
//
//                strict  default  lenient
//   cyclomatic      8       15       25
//   cognitive       8       15       25
//
// Both metric columns currently hold the same values. The dual-column
// infrastructure is preserved because cognitive and cyclomatic scores
// can diverge in magnitude for the same code (cognitive is
// nesting-weighted; cyclomatic counts decision points), and a future
// recalibration may want to widen one column without touching the
// other. Today the columns are flat-equal — the simplest defensible
// position until corpus evidence motivates a split.
//
// `ThresholdPreset::threshold(metric)` is the single lookup that keys
// a tier to a column. Every preset / `--strict` / `--lenient` /
// no-flag-default resolution routes through it, so a future per-metric
// divergence is a one-line constant change with no API churn.

/// Strict CRAP cutoff for the **cognitive** metric — gates at the
/// Low → Acceptable risk boundary; for high-quality or safety-critical
/// code. Cyclomatic equivalent: [`STRICT_THRESHOLD_CYCLOMATIC`].
pub const STRICT_THRESHOLD: f64 = 8.0;

/// Default CRAP cutoff for the **cognitive** metric — gates at the
/// Acceptable → Moderate risk boundary; the balanced tier for typical
/// codebases. Cyclomatic equivalent: [`DEFAULT_THRESHOLD_CYCLOMATIC`].
pub const DEFAULT_THRESHOLD: f64 = 15.0;

/// Lenient CRAP cutoff for the **cognitive** metric — gates at the
/// Moderate → High risk boundary; for legacy or transitional code.
/// Cyclomatic equivalent: [`LENIENT_THRESHOLD_CYCLOMATIC`].
pub const LENIENT_THRESHOLD: f64 = 25.0;

/// Strict CRAP cutoff for the **cyclomatic** metric. Currently flat-
/// equal to the cognitive strict value; the constant is retained so a
/// future per-metric recalibration is a one-line change.
pub const STRICT_THRESHOLD_CYCLOMATIC: f64 = 8.0;

/// Default CRAP cutoff for the **cyclomatic** metric. Currently flat-
/// equal to the cognitive default; see column note above.
pub const DEFAULT_THRESHOLD_CYCLOMATIC: f64 = 15.0;

/// Lenient CRAP cutoff for the **cyclomatic** metric. Currently flat-
/// equal to the cognitive lenient; see column note above.
pub const LENIENT_THRESHOLD_CYCLOMATIC: f64 = 25.0;

/// Named threshold preset — a calibration *tier*, independent of
/// metric. The concrete f64 cutoff is resolved per metric via
/// [`ThresholdPreset::threshold`] (the same tier maps to a different
/// number for cyclomatic vs cognitive scores).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdPreset {
    /// High-quality libraries, safety-critical code. Gates at the
    /// Low → Acceptable risk boundary (cognitive 8, cyclomatic 8).
    Strict,
    /// Typical projects (balanced) — the tier used when no preset or
    /// explicit threshold is given. Gates at the Acceptable → Moderate
    /// risk boundary (cognitive 15, cyclomatic 15).
    Default,
    /// Legacy or transitional code. Gates at the Moderate → High risk
    /// boundary (cognitive 25, cyclomatic 25).
    Lenient,
}

impl ThresholdPreset {
    /// Resolve this tier to its concrete f64 CRAP cutoff for `metric`.
    /// This is the single place tier→number is keyed on the metric:
    /// `--strict` / `--lenient` / config `preset` / the no-flag
    /// default all route through it, so a cutoff calibrated for one
    /// metric is never silently applied to the other metric's
    /// (different-magnitude) scores.
    pub fn threshold(self, metric: ComplexityMetric) -> f64 {
        match (metric, self) {
            (ComplexityMetric::Cognitive, Self::Strict) => STRICT_THRESHOLD,
            (ComplexityMetric::Cognitive, Self::Default) => DEFAULT_THRESHOLD,
            (ComplexityMetric::Cognitive, Self::Lenient) => LENIENT_THRESHOLD,
            (ComplexityMetric::Cyclomatic, Self::Strict) => STRICT_THRESHOLD_CYCLOMATIC,
            (ComplexityMetric::Cyclomatic, Self::Default) => DEFAULT_THRESHOLD_CYCLOMATIC,
            (ComplexityMetric::Cyclomatic, Self::Lenient) => LENIENT_THRESHOLD_CYCLOMATIC,
        }
    }
}

/// Returns true if the value is a valid CRAP threshold (finite and positive).
pub fn is_valid_threshold(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

/// A glob-based threshold override for a specific path pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct ThresholdOverride {
    /// Glob pattern matched against project-relative file paths (e.g. `domain/**`).
    pub pattern: String,
    /// CRAP threshold for functions in files matching this pattern.
    pub threshold: f64,
}

/// Threshold configuration with optional per-path overrides.
///
/// When overrides are present, each function's file path is tested against
/// the override patterns in declaration order. The last matching override
/// wins. If no override matches, the global threshold applies.
#[derive(Debug, Clone, PartialEq)]
pub struct ThresholdConfig {
    /// Global CRAP threshold (used when no override matches).
    pub global: f64,
    /// Per-path overrides, evaluated in order (last match wins).
    pub overrides: Vec<ThresholdOverride>,
}

/// Returns the cognitive-metric default cutoff. `Default` cannot take
/// a metric argument, so it cannot be metric-correct on its own — it
/// is the cognitive baseline only.
///
/// The CLI never relies on this: the analysis path resolves the
/// metric-correct `global` through `merge_threshold` (CLI > config >
/// adapter default, keyed on the effective metric) and constructs
/// `ThresholdConfig` directly, bypassing `Default`. The remaining
/// `Default` use is a struct-field initializer that is overwritten
/// before any analysis runs.
///
/// A library embedder that needs a metric-correct config should build
/// it explicitly rather than via `Default`:
///
/// ```
/// # use crap_core::domain::threshold::{ThresholdConfig, ThresholdPreset};
/// # use crap_core::domain::types::ComplexityMetric;
/// let metric = ComplexityMetric::Cyclomatic;
/// let config = ThresholdConfig {
///     global: ThresholdPreset::Default.threshold(metric),
///     overrides: Vec::new(),
/// };
/// ```
impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            global: DEFAULT_THRESHOLD,
            overrides: Vec::new(),
        }
    }
}

impl ThresholdConfig {
    /// Returns true if any per-path overrides are configured.
    pub fn has_overrides(&self) -> bool {
        !self.overrides.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_constants() {
        // Aligned with classify_risk boundaries (Low ≤ 8, Acceptable ≤ 15,
        // Moderate ≤ 25). Both metric columns flat-equal today; see the
        // calibration-table comment at the top of this module for why
        // the dual-column infrastructure is preserved.
        assert_eq!(STRICT_THRESHOLD, 8.0);
        assert_eq!(DEFAULT_THRESHOLD, 15.0);
        assert_eq!(LENIENT_THRESHOLD, 25.0);
        assert_eq!(STRICT_THRESHOLD_CYCLOMATIC, 8.0);
        assert_eq!(DEFAULT_THRESHOLD_CYCLOMATIC, 15.0);
        assert_eq!(LENIENT_THRESHOLD_CYCLOMATIC, 25.0);
    }

    #[test]
    fn preset_to_threshold_is_metric_keyed() {
        use ComplexityMetric::{Cognitive, Cyclomatic};
        // Cognitive column (crap4rs default).
        assert_eq!(ThresholdPreset::Strict.threshold(Cognitive), 8.0);
        assert_eq!(ThresholdPreset::Default.threshold(Cognitive), 15.0);
        assert_eq!(ThresholdPreset::Lenient.threshold(Cognitive), 25.0);
        // Cyclomatic column (crap4ts / crap4rs --metric cyclomatic).
        // Routing stays metric-keyed even with flat-equal values so a
        // future per-metric recalibration is a one-line change.
        assert_eq!(ThresholdPreset::Strict.threshold(Cyclomatic), 8.0);
        assert_eq!(ThresholdPreset::Default.threshold(Cyclomatic), 15.0);
        assert_eq!(ThresholdPreset::Lenient.threshold(Cyclomatic), 25.0);
    }

    #[test]
    fn default_config_uses_default_threshold() {
        let config = ThresholdConfig::default();
        assert_eq!(config.global, DEFAULT_THRESHOLD);
        assert!(config.overrides.is_empty());
    }

    #[test]
    fn has_overrides_false_when_empty() {
        let config = ThresholdConfig::default();
        assert!(!config.has_overrides());
    }

    #[test]
    fn has_overrides_true_when_present() {
        let config = ThresholdConfig {
            global: DEFAULT_THRESHOLD,
            overrides: vec![ThresholdOverride {
                pattern: "domain/**".to_string(),
                threshold: 5.0,
            }],
        };
        assert!(config.has_overrides());
    }

    #[test]
    fn is_valid_threshold_accepts_positive_finite() {
        assert!(is_valid_threshold(1.0));
        assert!(is_valid_threshold(0.001));
        assert!(is_valid_threshold(DEFAULT_THRESHOLD));
        assert!(is_valid_threshold(100.0));
    }

    #[test]
    fn is_valid_threshold_rejects_invalid() {
        assert!(!is_valid_threshold(0.0));
        assert!(!is_valid_threshold(-1.0));
        assert!(!is_valid_threshold(f64::NAN));
        assert!(!is_valid_threshold(f64::INFINITY));
        assert!(!is_valid_threshold(f64::NEG_INFINITY));
    }

    #[test]
    fn threshold_override_equality() {
        let a = ThresholdOverride {
            pattern: "src/**".to_string(),
            threshold: 10.0,
        };
        let b = ThresholdOverride {
            pattern: "src/**".to_string(),
            threshold: 10.0,
        };
        assert_eq!(a, b);
    }
}
