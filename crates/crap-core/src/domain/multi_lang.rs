//! Multi-language composition types — N-adapter agnostic.
//!
//! These types model "the unified report" composed from per-adapter
//! analysis envelopes. The composition lives in
//! [`crate::core::compose::compose_multi_lang`]; the rendering lives
//! in [`crate::adapters::reporters::format_html_multi`]. This module
//! is data-only — no I/O, no rendering, no language-specific logic.
//!
//! ## N-adapter invariant
//!
//! [`LanguageBlock`] carries adapter identity (`tool_name`,
//! `display_name`, `language`) as owned strings rather than borrowing
//! `&'static str` from an in-process `AdapterMeta`. This is deliberate:
//! consumers like the multi-language renderer load envelopes from
//! disk where adapter identity is owned `String` after deserialization,
//! and any future adapter (e.g. a hypothetical Python or Go adapter)
//! can supply its own identity without modifying this module.
//!
//! The composition function is generic over `Vec<LanguageBlock>`; there
//! is no hardcoded list of supported languages. Adding a new adapter
//! amounts to producing a `LanguageBlock` from its envelope — no
//! changes here.

use crate::domain::delta::DeltaView;
use crate::domain::types::{ComplexityMetric, FunctionIdentity, RiskDistribution, RiskLevel};
use crate::domain::view::AnalysisView;

/// Composed multi-language report context.
///
/// Carries every per-language block plus the cross-adapter combined
/// summary. The renderer reads this directly — no further composition
/// happens at render time.
///
/// `'a` borrows lifetime is the lifetime of the underlying
/// `AnalysisView` / `DeltaView` data each block wraps. Combined view
/// data (the `combined` field) is owned because it aggregates across
/// blocks and produces ranked functions that may outlive any
/// single block borrow.
#[derive(Debug)]
pub struct MultiLangContext<'a> {
    /// Per-language blocks in caller-supplied order. The renderer
    /// emits Language nav buttons in this order; callers control
    /// whether Rust appears before TypeScript (default in the
    /// composer: alphabetical by `tool_name`).
    pub languages: Vec<LanguageBlock<'a>>,
    /// Cross-adapter summary + ranked-CRAP list. See
    /// [`CombinedSummary`] for the locked aggregation rules.
    pub combined: CombinedSummary,
}

/// One adapter's per-language data: identity, view, optional delta.
///
/// Adapter identity (`tool_name`, `display_name`, `language`,
/// `tool_version`) is owned `String` to compose cleanly with
/// envelope deserialization. The view + delta references borrow
/// from analyses constructed upstream.
#[derive(Debug)]
pub struct LanguageBlock<'a> {
    /// Stable wire identifier (e.g. `"crap4rs"`, `"crap4ts"`).
    /// Drives the dedup check in
    /// [`crate::core::compose::compose_multi_lang`] and the
    /// `data-tool` markers in the rendered Language nav.
    pub tool_name: String,
    /// Human-readable adapter label (e.g. `"Rust"`, `"TypeScript"`).
    /// Surfaced in the Adapters footer + the per-row badges in the
    /// Combined view's ranked table.
    pub display_name: String,
    /// Stable wire language tag (e.g. `"rust"`, `"typescript"`).
    /// Drives the `data-lang` attribute on the segmented Language
    /// nav + URL hash routing (`#rust:current`, `#typescript:delta`,
    /// `#combined:current`).
    pub language: String,
    /// Adapter version string (e.g. `"0.6.0"`). Surfaced in the
    /// page title and the per-adapter footer; carried separately
    /// from `tool_name` so envelope-loaded data can populate both
    /// without the renderer reconstructing version strings.
    pub tool_version: String,
    /// Per-adapter complexity metric used to score this analysis.
    /// Surfaced in the Adapters footer row ("Rust · cognitive
    /// complexity · …") so reviewers can see the dimensional
    /// difference between adapters at a glance.
    pub metric: ComplexityMetric,
    /// Workspace-level CRAP threshold for this adapter. Per-function
    /// thresholds may override (see
    /// `FunctionVerdict.threshold`); this is the dominant value the
    /// adapter's KPI tiles cite.
    pub threshold: f64,
    /// The per-adapter analysis view (shaped rows, summary, etc.).
    pub view: AnalysisView<'a>,
    /// Optional delta vs baseline. `None` when no baseline was
    /// provided to this adapter; the Combined → Delta tab handles
    /// the asymmetric case (per shaping doc EDGE locks).
    pub delta: Option<DeltaView<'a>>,
}

/// Cross-adapter aggregation produced by composition.
///
/// Sums (functions, exceeding, files) are additive because each
/// adapter scans disjoint source trees. The ranked function list
/// applies the D2d sort rule (risk level desc, then CRAP/threshold
/// ratio desc within band) — this avoids treating raw CRAP scores as
/// dimensionally equivalent across adapters with different complexity
/// metric scales.
#[derive(Debug, Default)]
pub struct CombinedSummary {
    /// Sum of `total_functions` across all blocks. Each adapter scans
    /// a disjoint source tree, so the sum is non-overlapping.
    pub total_functions: usize,
    /// Sum of `exceeding_threshold` across all blocks. "Exceeds" is
    /// computed per-adapter against that adapter's own calibrated
    /// threshold, so the sum is dimensionally honest at the count
    /// level (binary per-function, then count).
    pub total_exceeding: usize,
    /// Sum of `total_files` across all blocks.
    pub total_files: usize,
    /// Worst single function across all adapters by CRAP/threshold
    /// ratio. Ratio is dimensionally consistent across adapters
    /// (each fraction is "how far over its own adapter's
    /// threshold"); raw CRAP is NOT comparable cross-adapter and
    /// would not produce an honest "worst" reading.
    pub worst_ratio: Option<WorstRatio>,
    /// Aggregate risk distribution: sum per tier across all
    /// adapters. Each adapter classifies functions per its own
    /// calibrated thresholds (see ADR on metric-keyed threshold
    /// calibration), so a High-risk Rust function and a High-risk
    /// TypeScript function are both "above their respective High
    /// thresholds" — the per-tier sum is dimensionally honest.
    pub distribution: RiskDistribution,
    /// Workspace-wide ranked function list. Sort key: risk level
    /// descending (High → Low), then CRAP/threshold ratio descending
    /// within each band. See `RankedFunction` for the per-row data
    /// shape.
    pub ordered_functions: Vec<RankedFunction>,
}

/// One row of the Combined-view ranked-CRAP table.
///
/// Each row identifies both the function and its source adapter so
/// renderers can paint the per-row adapter badge.
#[derive(Debug, Clone)]
pub struct RankedFunction {
    /// Wire language tag (e.g. `"rust"`). Drives the `data-lang`
    /// marker on the rendered row.
    pub language: String,
    /// Display label (e.g. `"Rust"`) for the badge text.
    pub adapter_display: String,
    /// Function identity (qualified name + file + span).
    pub identity: FunctionIdentity,
    /// Raw CRAP score — surfaced alongside ratio so users see the
    /// actual number. NOT the sort key.
    pub crap: f64,
    /// The adapter threshold this function was scored against.
    pub threshold: f64,
    /// CRAP / threshold (the secondary sort key). Dimensionally
    /// consistent within an adapter's risk band.
    pub ratio: f64,
    /// Risk level classification per the adapter's calibrated
    /// thresholds. Drives the primary sort + the risk-pill in the
    /// rendered row.
    pub risk_level: RiskLevel,
    /// Line coverage percentage (0..=100). Surfaced inline in the
    /// ranked row.
    pub coverage_percent: f64,
    /// Complexity number (cognitive or cyclomatic per the adapter's
    /// `metric` field — see `LanguageBlock.metric`).
    pub complexity: u32,
}

/// Worst CRAP/threshold ratio observed across all adapters.
///
/// Ratio (not raw CRAP) is the dimensionally honest comparand.
#[derive(Debug, Clone)]
pub struct WorstRatio {
    /// The ratio itself (e.g. 5.72 for a function 5.72× over its
    /// adapter's threshold).
    pub ratio: f64,
    /// Wire language tag of the adapter that scored this function.
    pub language: String,
    /// Display label of the adapter (e.g. `"Rust"`).
    pub adapter_display: String,
    /// Qualified function name (e.g. `"view::analyze_view"`).
    pub function_name: String,
}

/// Numeric ordering for risk levels: High > Moderate > Acceptable > Low.
///
/// Used by the D2d sort to group ranked functions by risk band.
/// Pure: returns a `u8` so consumers can `.cmp` against tuples.
pub fn risk_level_rank(level: RiskLevel) -> u8 {
    match level {
        RiskLevel::High => 4,
        RiskLevel::Moderate => 3,
        RiskLevel::Acceptable => 2,
        RiskLevel::Low => 1,
    }
}

/// Compute the CRAP / threshold ratio with a safe-divide guard.
///
/// Adapter envelopes with a zero or negative threshold are
/// configuration errors (treated upstream); this function returns
/// `f64::INFINITY` for that edge case so the ranked list still
/// places such functions at the top of their risk band rather than
/// crashing.
pub fn safe_ratio(crap: f64, threshold: f64) -> f64 {
    if threshold > 0.0 {
        crap / threshold
    } else {
        f64::INFINITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::RiskLevel;

    #[test]
    fn risk_level_rank_orders_high_above_low() {
        assert!(risk_level_rank(RiskLevel::High) > risk_level_rank(RiskLevel::Moderate));
        assert!(risk_level_rank(RiskLevel::Moderate) > risk_level_rank(RiskLevel::Acceptable));
        assert!(risk_level_rank(RiskLevel::Acceptable) > risk_level_rank(RiskLevel::Low));
    }

    #[test]
    fn safe_ratio_divides_normally() {
        assert!((safe_ratio(45.0, 8.0) - 5.625).abs() < 1e-9);
    }

    #[test]
    fn safe_ratio_returns_infinity_on_zero_threshold() {
        assert!(safe_ratio(10.0, 0.0).is_infinite());
    }

    #[test]
    fn safe_ratio_returns_infinity_on_negative_threshold() {
        assert!(safe_ratio(10.0, -1.0).is_infinite());
    }
}
