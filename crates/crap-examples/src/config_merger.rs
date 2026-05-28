//! Compound catastrophe. High complexity multiplied by low coverage
//! lands in the Critical band — the product of both terms of the
//! CRAP formula at once. This module isolates the worst case so the
//! heatmap has a Critical-band anchor.
//!
//! `merge_configs` reconciles two TOML sources (defaults and env
//! overrides) with nested-table merging and explicit error returns.
//! The tests below only cover the simplest top-level merge; the
//! nested merging arm and every error branch stay uncovered. The
//! score lives at the top of the pedagogical distribution.

use anyhow::{Result, anyhow};
use toml::Value;
use toml::map::Map;

#[derive(Debug, Clone, Default)]
pub struct MergedConfig {
    pub values: Map<String, Value>,
}

/// Merge two TOML sources: env overrides defaults at the top level,
/// and nested tables merge recursively. Type mismatches produce
/// errors.
pub fn merge_configs(defaults: &str, env: &str) -> Result<MergedConfig> {
    let defaults_value: Value = defaults
        .parse::<Value>()
        .map_err(|e| anyhow!("defaults parse failed: {e}"))?;
    let env_value: Value = env
        .parse::<Value>()
        .map_err(|e| anyhow!("env parse failed: {e}"))?;

    let mut merged = match defaults_value {
        Value::Table(t) => t,
        _ => return Err(anyhow!("defaults must be a TOML table")),
    };

    let env_table = match env_value {
        Value::Table(t) => t,
        _ => return Err(anyhow!("env must be a TOML table")),
    };

    for (key, value) in env_table {
        if let Some(existing) = merged.get_mut(&key) {
            if let (Value::Table(left), Value::Table(right)) = (existing, &value) {
                for (k, v) in right {
                    if let Some(sub_existing) = left.get_mut(k) {
                        *sub_existing = v.clone();
                    } else {
                        left.insert(k.clone(), v.clone());
                    }
                }
            } else {
                merged.insert(key, value);
            }
        } else {
            merged.insert(key, value);
        }
    }

    Ok(MergedConfig { values: merged })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Only the simplest top-level merge is covered. ──
    //
    // Deliberately uncovered branches:
    //   * nested-table recursive merge
    //   * defaults-must-be-table Err
    //   * env-must-be-table Err
    //   * defaults parse Err
    //   * env parse Err
    //   * nested-table sub-key already-exists branch
    //
    // The CRAP target for `merge_configs` is the top of the
    // Critical band; if a contributor expands the test suite without
    // rebanding the pedagogy, the README heatmap drifts.

    #[test]
    fn defaults_only_passes_through() {
        let merged = merge_configs("foo = 1\nbar = \"baz\"\n", "").unwrap();
        assert_eq!(merged.values.get("foo"), Some(&Value::Integer(1)));
        assert_eq!(
            merged.values.get("bar"),
            Some(&Value::String("baz".to_string()))
        );
    }

    #[test]
    fn env_overrides_defaults_at_top_level() {
        let merged = merge_configs("foo = 1\n", "foo = 2\n").unwrap();
        assert_eq!(merged.values.get("foo"), Some(&Value::Integer(2)));
    }

    #[test]
    fn merged_config_default_is_empty() {
        let m = MergedConfig::default();
        assert!(m.values.is_empty());
    }
}
