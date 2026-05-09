/// Strict CRAP threshold — for high-quality or safety-critical code.
/// Matches SonarSource S3776 cognitive complexity limit and eliminates false
/// positives on well-tested idiomatic Rust (SeaORM-style large match arms).
pub const STRICT_THRESHOLD: f64 = 15.0;

/// Default CRAP threshold — balanced for typical Rust codebases.
pub const DEFAULT_THRESHOLD: f64 = 25.0;

/// Lenient CRAP threshold — for legacy or transitional code.
pub const LENIENT_THRESHOLD: f64 = 40.0;

/// Named threshold preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdPreset {
    /// `STRICT_THRESHOLD` (15) — high-quality libraries, safety-critical code.
    Strict,
    /// `DEFAULT_THRESHOLD` (25) — typical Rust projects.
    Default,
    /// `LENIENT_THRESHOLD` (40) — legacy or transitional code.
    Lenient,
}

impl ThresholdPreset {
    /// Returns the f64 threshold value for this preset.
    pub fn threshold(self) -> f64 {
        match self {
            Self::Strict => STRICT_THRESHOLD,
            Self::Default => DEFAULT_THRESHOLD,
            Self::Lenient => LENIENT_THRESHOLD,
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
        assert_eq!(DEFAULT_THRESHOLD, 25.0);
        assert_eq!(STRICT_THRESHOLD, 15.0);
        assert_eq!(LENIENT_THRESHOLD, 40.0);
    }

    #[test]
    fn preset_to_threshold() {
        assert_eq!(ThresholdPreset::Strict.threshold(), STRICT_THRESHOLD);
        assert_eq!(ThresholdPreset::Default.threshold(), DEFAULT_THRESHOLD);
        assert_eq!(ThresholdPreset::Lenient.threshold(), LENIENT_THRESHOLD);
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
