/// Default CRAP score threshold.
pub const DEFAULT_THRESHOLD: f64 = 8.0;

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
            global: 8.0,
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
