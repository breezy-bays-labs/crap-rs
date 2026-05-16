use super::types::ComplexityMetric;

// ── Threshold calibration table ──────────────────────────────────────
//
// CRAP thresholds are NOT metric-invariant. For the *same* function,
// its cyclomatic count (decision points) and its cognitive count
// (nesting-weighted structural complexity) differ in magnitude — a
// function that is "moderately risky" scores ~16 cyclomatic but ~25
// cognitive. So one scalar cutoff cannot serve both metrics: a cutoff
// tuned for cognitive scores, applied to cyclomatic scores, lets
// genuinely-risky functions pass (and vice versa).
//
// Each calibration *tier* (strict / default / lenient) therefore has a
// distinct cutoff per metric:
//
//                strict  default  lenient
//   cyclomatic      8       16       30
//   cognitive      15       25       40
//
// The bare-named constants are the cognitive column; the
// `_CYCLOMATIC` siblings are the cyclomatic column.
// `ThresholdPreset::threshold(metric)` is the single lookup that keys
// a tier to the right column — every preset / `--strict` / `--lenient`
// / no-flag-default resolution routes through it so no path applies a
// metric's cutoff to the other metric's scores.

/// Strict CRAP cutoff for the **cognitive** metric — high-quality or
/// safety-critical code. Matches SonarSource S3776's cognitive
/// complexity limit and eliminates false positives on well-tested
/// idiomatic Rust (SeaORM-style large match arms). Cyclomatic
/// equivalent: [`STRICT_THRESHOLD_CYCLOMATIC`].
pub const STRICT_THRESHOLD: f64 = 15.0;

/// Default CRAP cutoff for the **cognitive** metric — balanced for
/// typical codebases. Cyclomatic equivalent: [`DEFAULT_THRESHOLD_CYCLOMATIC`].
pub const DEFAULT_THRESHOLD: f64 = 25.0;

/// Lenient CRAP cutoff for the **cognitive** metric — legacy or
/// transitional code. Cyclomatic equivalent: [`LENIENT_THRESHOLD_CYCLOMATIC`].
pub const LENIENT_THRESHOLD: f64 = 40.0;

/// Strict CRAP cutoff for the **cyclomatic** metric. ~half the
/// cognitive strict value because cyclomatic scores run lower in
/// magnitude for the same code.
pub const STRICT_THRESHOLD_CYCLOMATIC: f64 = 8.0;

/// Default CRAP cutoff for the **cyclomatic** metric — the balanced
/// tier for cyclomatic-scored code.
pub const DEFAULT_THRESHOLD_CYCLOMATIC: f64 = 16.0;

/// Lenient CRAP cutoff for the **cyclomatic** metric.
pub const LENIENT_THRESHOLD_CYCLOMATIC: f64 = 30.0;

/// Named threshold preset — a calibration *tier*, independent of
/// metric. The concrete f64 cutoff is resolved per metric via
/// [`ThresholdPreset::threshold`] (the same tier maps to a different
/// number for cyclomatic vs cognitive scores).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdPreset {
    /// High-quality libraries, safety-critical code. Cognitive 15,
    /// cyclomatic 8.
    Strict,
    /// Typical projects (balanced) — the tier used when no preset or
    /// explicit threshold is given. Cognitive 25, cyclomatic 16.
    Default,
    /// Legacy or transitional code. Cognitive 40, cyclomatic 30.
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
        // D5 calibration table (locked decision #5).
        assert_eq!(STRICT_THRESHOLD, 15.0);
        assert_eq!(DEFAULT_THRESHOLD, 25.0);
        assert_eq!(LENIENT_THRESHOLD, 40.0);
        assert_eq!(STRICT_THRESHOLD_CYCLOMATIC, 8.0);
        assert_eq!(DEFAULT_THRESHOLD_CYCLOMATIC, 16.0);
        assert_eq!(LENIENT_THRESHOLD_CYCLOMATIC, 30.0);
    }

    #[test]
    fn preset_to_threshold_is_metric_keyed() {
        use ComplexityMetric::{Cognitive, Cyclomatic};
        // Cognitive column (crap4rs).
        assert_eq!(ThresholdPreset::Strict.threshold(Cognitive), 15.0);
        assert_eq!(ThresholdPreset::Default.threshold(Cognitive), 25.0);
        assert_eq!(ThresholdPreset::Lenient.threshold(Cognitive), 40.0);
        // Cyclomatic column (crap4ts / crap4rs --metric cyclomatic) —
        // the #218 fix: a tier resolves to the metric-correct cutoff,
        // not the cognitive value applied blindly.
        assert_eq!(ThresholdPreset::Strict.threshold(Cyclomatic), 8.0);
        assert_eq!(ThresholdPreset::Default.threshold(Cyclomatic), 16.0);
        assert_eq!(ThresholdPreset::Lenient.threshold(Cyclomatic), 30.0);
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
