//! Domain-level View abstraction — the canonical pure-domain shaping
//! primitive over `AnalysisResult`.
//!
//! ```text
//! ViewSpec → view::apply(&result, spec) → AnalysisView<'_>
//! ```
//!
//! The View filters, sorts, and truncates findings to produce a shaped
//! report. The keystone invariant is the **gate is unshapeable; only the
//! display is shapeable**: `view.full` always borrows the original,
//! unfiltered `AnalysisResult`, and exit-code logic must derive from
//! `view.full.passed`, never from the post-shape `view.shown`.
//!
//! Pure domain code — no I/O, no external crates beyond `serde` and
//! `thiserror` (mirrors `domain::types`). Future `crap-core` extraction
//! takes this module whole.

use crate::domain::summary::compute_summary;
use crate::domain::types::{AnalysisResult, AnalysisSummary, FunctionVerdict};
use serde::Serialize;

// ── Spec types ───────────────────────────────────────────────────────

#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize)]
pub struct ViewSpec {
    pub filters: Filters,
    pub sort: SortKey,
    pub limit: Option<usize>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize)]
pub struct Filters {
    pub only_failing: bool,
    pub coverage_range: Option<CoverageRange>,
}

/// Inclusive coverage range filter.
///
/// Both endpoints are validated to be in `[0.0, 100.0]` and `min <= max`
/// at construction time; downstream consumers can rely on these
/// invariants without re-checking.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CoverageRange {
    pub min: f64,
    pub max: f64,
}

impl CoverageRange {
    pub fn new(min: f64, max: f64) -> Result<Self, CoverageRangeError> {
        if !is_in_unit_percent(min) {
            return Err(CoverageRangeError::OutOfRange { value: min });
        }
        if !is_in_unit_percent(max) {
            return Err(CoverageRangeError::OutOfRange { value: max });
        }
        if min > max {
            return Err(CoverageRangeError::MinExceedsMax { min, max });
        }
        Ok(Self { min, max })
    }
}

fn is_in_unit_percent(v: f64) -> bool {
    v.is_finite() && (0.0..=100.0).contains(&v)
}

/// Tag-only error type — variants carry numeric context but no prose.
/// The CLI translates these to user-facing messages so the domain stays
/// language-agnostic for `crap-core` extraction.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq)]
pub enum CoverageRangeError {
    #[error("coverage value out of range: {value}")]
    OutOfRange { value: f64 },
    #[error("min ({min}) exceeds max ({max})")]
    MinExceedsMax { min: f64, max: f64 },
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    /// CRAP score descending (matches the legacy table reporter's order).
    #[default]
    Crap,
    /// Coverage ascending (low-coverage shown first — investigator's interest).
    Coverage,
    /// Complexity descending.
    Complexity,
    /// Alphabetical by `file_path`, then CRAP descending within file.
    Path,
}

// ── View output ──────────────────────────────────────────────────────

/// The shaped result of applying a `ViewSpec` to an `AnalysisResult`.
///
/// `full` is borrow-only and elided from JSON output (the envelope's
/// `result` field already carries the same data). All shaping happens
/// over `shown`; `eligible_count` is the post-filter, pre-truncate
/// count; `truncated` records whether `limit` reduced the row set.
#[non_exhaustive]
#[derive(Debug, Serialize)]
pub struct AnalysisView<'a> {
    /// Borrows the original analysis. `#[serde(skip)]` because the
    /// envelope's `result` already serializes the full analysis.
    #[serde(skip)]
    pub full: &'a AnalysisResult,
    pub spec: ViewSpec,
    pub eligible_count: usize,
    pub truncated: bool,
    pub shown: Vec<&'a FunctionVerdict>,
    pub shown_summary: AnalysisSummary,
}

// ── apply: filter → sort → truncate ──────────────────────────────────

pub fn apply<'a>(result: &'a AnalysisResult, spec: ViewSpec) -> AnalysisView<'a> {
    let eligible: Vec<&'a FunctionVerdict> = apply_filters(&result.functions, &spec.filters);
    let eligible_count = eligible.len();

    let mut shown = eligible;
    sort_in_place(&mut shown, spec.sort);
    let truncated = truncate_to(&mut shown, spec.limit);

    let shown_owned: Vec<FunctionVerdict> = shown.iter().map(|&v| v.clone()).collect();
    let shown_summary = compute_summary(&shown_owned);

    AnalysisView {
        full: result,
        spec,
        eligible_count,
        truncated,
        shown,
        shown_summary,
    }
}

/// Filter pass — returns a vector of references that match every active filter.
///
/// AND-composes filters: a verdict is eligible iff every active filter
/// admits it. Coverage-range branch uses `is_finite()` so NaN coverage
/// is excluded explicitly (BDD: view.feature:231).
fn apply_filters<'a>(
    verdicts: &'a [FunctionVerdict],
    filters: &Filters,
) -> Vec<&'a FunctionVerdict> {
    verdicts
        .iter()
        .filter(|v| !filters.only_failing || v.exceeds)
        .filter(|v| match &filters.coverage_range {
            Some(range) => matches_coverage_range(v.scored.coverage_percent, range),
            None => true,
        })
        .collect()
}

fn matches_coverage_range(cov: f64, range: &CoverageRange) -> bool {
    cov.is_finite() && cov >= range.min && cov <= range.max
}

// ── Sort dispatch + comparators ──────────────────────────────────────

fn sort_in_place(shown: &mut [&FunctionVerdict], key: SortKey) {
    // sort_by — stable. NOT sort_unstable_by: callers rely on input-order
    // preservation for tied keys (BDD: view.feature:122-128).
    match key {
        SortKey::Crap => shown.sort_by(cmp_by_crap),
        SortKey::Coverage => shown.sort_by(cmp_by_coverage),
        SortKey::Complexity => shown.sort_by(cmp_by_complexity),
        SortKey::Path => shown.sort_by(cmp_by_path),
    }
}

/// CRAP descending. f64-bearing — NaN sorts last under the descending
/// order (i.e., comparator returns `Less` for non-NaN vs NaN-second so
/// non-NaN wins the descending position; symmetrically for NaN-first).
fn cmp_by_crap(a: &&FunctionVerdict, b: &&FunctionVerdict) -> std::cmp::Ordering {
    let (ax, bx) = (a.scored.crap.value, b.scored.crap.value);
    match (ax.is_nan(), bx.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => bx.partial_cmp(&ax).expect("non-NaN partial_cmp infallible"),
    }
}

/// Coverage ascending. NaN sorts last (BDD: view.feature:237).
fn cmp_by_coverage(a: &&FunctionVerdict, b: &&FunctionVerdict) -> std::cmp::Ordering {
    let (ax, bx) = (a.scored.coverage_percent, b.scored.coverage_percent);
    match (ax.is_nan(), bx.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => ax.partial_cmp(&bx).expect("non-NaN partial_cmp infallible"),
    }
}

/// Complexity descending. `u32` is `Ord`; no NaN concerns.
fn cmp_by_complexity(a: &&FunctionVerdict, b: &&FunctionVerdict) -> std::cmp::Ordering {
    b.scored.complexity.cmp(&a.scored.complexity)
}

/// Path alphabetical ascending; ties broken by CRAP descending.
fn cmp_by_path(a: &&FunctionVerdict, b: &&FunctionVerdict) -> std::cmp::Ordering {
    match a
        .scored
        .identity
        .file_path
        .cmp(&b.scored.identity.file_path)
    {
        std::cmp::Ordering::Equal => cmp_by_crap(a, b),
        ord => ord,
    }
}

// ── Truncate ─────────────────────────────────────────────────────────

/// Truncate `shown` to `limit` entries. Returns whether any rows were
/// dropped. `Some(0)` and `None` are treated identically as "no limit"
/// (BDD: view.feature:213 — `--top 0` semantics).
fn truncate_to(shown: &mut Vec<&FunctionVerdict>, limit: Option<usize>) -> bool {
    match limit {
        Some(n) if n > 0 && shown.len() > n => {
            shown.truncate(n);
            true
        }
        _ => false,
    }
}

// ── Display predicate ────────────────────────────────────────────────

/// True iff the shaped view materially differs from the underlying
/// analysis (rows filtered out OR rows truncated). Reporters use this
/// to decide whether to emit a "View:" subtitle line.
///
/// Default `ViewSpec` over a non-empty result returns `false` — the
/// walking-skeleton invariant.
pub fn should_render_view_line(view: &AnalysisView<'_>) -> bool {
    view.eligible_count < view.full.functions.len() || view.truncated
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        AnalysisSummary, ComplexityMetric, CrapScore, FunctionIdentity, FunctionVerdict,
        RiskDistribution, ScoredFunction, SourceSpan,
    };

    // ── Fixture helpers ────────────────────────────────────────────

    fn mk_verdict(
        name: &str,
        file: &str,
        complexity: u32,
        coverage: f64,
        crap_value: f64,
        threshold: f64,
    ) -> FunctionVerdict {
        let risk_level = crate::domain::crap::classify_risk(crap_value);
        FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: file.to_string(),
                    qualified_name: name.to_string(),
                    span: SourceSpan {
                        start_line: 1,
                        end_line: 10,
                    },
                },
                complexity,
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: coverage,
                crap: CrapScore {
                    value: crap_value,
                    risk_level,
                },
                contributors: vec![],
            },
            threshold,
            exceeds: crap_value > threshold,
        }
    }

    /// Background fixture from view.feature ll. 9-17. Threshold 25.0.
    fn background_fixture() -> AnalysisResult {
        let verdicts = vec![
            mk_verdict("parse_lcov", "src/adapters/lcov.rs", 12, 100.0, 12.00, 25.0),
            mk_verdict("walk_ast", "src/adapters/syn.rs", 18, 75.0, 23.06, 25.0),
            mk_verdict(
                "render_table",
                "src/adapters/table.rs",
                9,
                60.0,
                14.18,
                25.0,
            ),
            mk_verdict(
                "apply_threshold",
                "src/domain/threshold.rs",
                4,
                100.0,
                4.00,
                25.0,
            ),
            mk_verdict(
                "sort_verdicts",
                "src/adapters/table.rs",
                6,
                0.0,
                42.00,
                25.0,
            ),
            mk_verdict("parse_args", "src/cli/mod.rs", 22, 50.0, 63.50, 25.0),
        ];
        let summary = compute_summary(&verdicts);
        let passed = verdicts.iter().all(|v| !v.exceeds);
        AnalysisResult {
            functions: verdicts,
            summary,
            passed,
        }
    }

    fn empty_result() -> AnalysisResult {
        AnalysisResult {
            functions: vec![],
            summary: AnalysisSummary {
                total_functions: 0,
                total_files: 0,
                exceeding_threshold: 0,
                average_crap: 0.0,
                median_crap: 0.0,
                max_crap: None,
                worst_function: None,
                distribution: RiskDistribution {
                    low: 0,
                    acceptable: 0,
                    moderate: 0,
                    high: 0,
                },
            },
            passed: true,
        }
    }

    // ── Default-spec invariants (Order, Identity, Summary, immutability) ───

    #[test]
    fn default_spec_is_noop_on_fixture() {
        // view.feature ll. 25-31: default spec produces a no-op view in
        // CRAP-descending order. Equivalent: shown contains every function;
        // eligible_count equals total; truncated false; CRAP desc.
        let r = background_fixture();
        let view = apply(&r, ViewSpec::default());
        assert_eq!(view.shown.len(), r.functions.len());
        assert_eq!(view.eligible_count, r.functions.len());
        assert!(!view.truncated);
        // Pointer equality on `view.full` (no PartialEq derive needed)
        assert!(std::ptr::eq(view.full, &r));
        // Order: CRAP descending
        for w in view.shown.windows(2) {
            assert!(
                w[0].scored.crap.value >= w[1].scored.crap.value,
                "expected CRAP descending; got {} then {}",
                w[0].scored.crap.value,
                w[1].scored.crap.value
            );
        }
    }

    #[test]
    fn default_spec_empty_input_is_empty_view() {
        // view.feature l. 197.
        let r = empty_result();
        let view = apply(&r, ViewSpec::default());
        assert!(view.shown.is_empty());
        assert_eq!(view.eligible_count, 0);
        assert!(!view.truncated);
        assert!(view.full.passed);
    }

    #[test]
    fn view_full_immutability_after_apply() {
        // view.feature l. 221.
        let r = background_fixture();
        let crap_before: Vec<f64> = r.functions.iter().map(|v| v.scored.crap.value).collect();
        let view = apply(&r, ViewSpec::default());
        // view.full points at r
        assert!(std::ptr::eq(view.full, &r));
        // r itself is unchanged
        let crap_after: Vec<f64> = r.functions.iter().map(|v| v.scored.crap.value).collect();
        assert_eq!(crap_before, crap_after);
    }

    #[test]
    fn default_spec_preserves_identity_set() {
        // view.feature ll. 33-35.
        let r = background_fixture();
        let view = apply(&r, ViewSpec::default());
        let shown_names: std::collections::HashSet<&String> = view
            .shown
            .iter()
            .map(|v| &v.scored.identity.qualified_name)
            .collect();
        let original_names: std::collections::HashSet<&String> = r
            .functions
            .iter()
            .map(|v| &v.scored.identity.qualified_name)
            .collect();
        assert_eq!(shown_names, original_names);
    }

    // ── CoverageRange constructor: 7-row table ─────────────────────

    #[test]
    fn coverage_range_new_validation_table() {
        // view.feature ll. 79-91.
        type Case = (f64, f64, Result<(f64, f64), ()>);
        let cases: &[Case] = &[
            (0.0, 100.0, Ok((0.0, 100.0))),
            (50.0, 50.0, Ok((50.0, 50.0))),
            (1.0, 90.0, Ok((1.0, 90.0))),
            (-0.1, 50.0, Err(())),
            (50.0, 100.1, Err(())),
            (90.0, 50.0, Err(())),
            (100.0, 0.0, Err(())),
        ];
        for (min, max, expect) in cases {
            let got = CoverageRange::new(*min, *max);
            match (got, expect) {
                (Ok(r), Ok((emin, emax))) => {
                    assert!(
                        (r.min - emin).abs() < 1e-9 && (r.max - emax).abs() < 1e-9,
                        "min={min}, max={max}: got {r:?}, expected ({emin}, {emax})"
                    );
                }
                (Err(_), Err(())) => {} // good
                (got, expect) => panic!("min={min}, max={max}: got {got:?}, expected {expect:?}"),
            }
        }
    }

    #[test]
    fn coverage_range_error_variants() {
        // out-of-range vs min-exceeds-max are distinct, tag-only variants.
        let oor = CoverageRange::new(-1.0, 50.0).unwrap_err();
        assert!(matches!(oor, CoverageRangeError::OutOfRange { .. }));
        let mxm = CoverageRange::new(80.0, 20.0).unwrap_err();
        assert!(matches!(mxm, CoverageRangeError::MinExceedsMax { .. }));
    }

    // ── Sort stability — the surgical mutation killer ───────────

    #[test]
    fn sort_stability_on_tied_crap() {
        // view.feature ll. 122-128. Catches `sort_by → sort_unstable_by`.
        // Hand-built deterministic [foo, bar] both at CRAP=12.0.
        let foo = mk_verdict("foo", "src/a.rs", 5, 80.0, 12.0, 25.0);
        let bar = mk_verdict("bar", "src/a.rs", 5, 80.0, 12.0, 25.0);
        let r = AnalysisResult {
            functions: vec![foo, bar],
            summary: empty_result().summary, // unused
            passed: true,
        };
        let view = apply(&r, ViewSpec::default());
        // Input order [foo, bar] preserved on tied keys.
        assert_eq!(
            view.shown[0].scored.identity.qualified_name,
            "foo",
            "stable sort must preserve input order on ties; got {:?}",
            view.shown
                .iter()
                .map(|v| &v.scored.identity.qualified_name)
                .collect::<Vec<_>>()
        );
        assert_eq!(view.shown[1].scored.identity.qualified_name, "bar");
    }

    // ── Filters ────────────────────────────────────────────────────

    #[test]
    fn only_failing_filter_retains_only_exceeds_true() {
        // view.feature l. 44.
        let r = background_fixture();
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.iter().all(|v| v.exceeds));
        // And every shown CRAP exceeds threshold
        for v in &view.shown {
            assert!(v.scored.crap.value > v.threshold);
        }
    }

    #[test]
    fn coverage_range_filter_inclusive() {
        // view.feature l. 50.
        let r = background_fixture();
        let range = CoverageRange::new(50.0, 90.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.iter().all(|v| {
            let cov = v.scored.coverage_percent;
            cov.is_finite() && (50.0..=90.0).contains(&cov)
        }));
        let manual_count = r
            .functions
            .iter()
            .filter(|v| v.scored.coverage_percent.is_finite())
            .filter(|v| (50.0..=90.0).contains(&v.scored.coverage_percent))
            .count();
        assert_eq!(view.eligible_count, manual_count);
    }

    #[test]
    fn coverage_range_boundary_inclusive_50_low() {
        // view.feature ll. 56-68 row 1: cov=50.0 in 50..=90 → appears.
        let v = mk_verdict("at50", "src/a.rs", 1, 50.0, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(50.0, 90.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 1);
    }

    #[test]
    fn coverage_range_boundary_inclusive_90_high() {
        // row 2: cov=90.0 in 50..=90 → appears.
        let v = mk_verdict("at90", "src/a.rs", 1, 90.0, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(50.0, 90.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 1);
    }

    #[test]
    fn coverage_range_boundary_inclusive_below_low() {
        // row 3: cov=49.9 in 50..=90 → absent.
        let v = mk_verdict("just_under", "src/a.rs", 1, 49.9, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(50.0, 90.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.is_empty());
    }

    #[test]
    fn coverage_range_boundary_inclusive_above_high() {
        // row 4: cov=90.1 in 50..=90 → absent.
        let v = mk_verdict("just_over", "src/a.rs", 1, 90.1, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(50.0, 90.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.is_empty());
    }

    #[test]
    fn coverage_range_boundary_inclusive_zero_singleton() {
        // row 5: cov=0.0 in 0..=0 → appears.
        let v = mk_verdict("zero", "src/a.rs", 1, 0.0, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(0.0, 0.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 1);
    }

    #[test]
    fn coverage_range_boundary_inclusive_hundred_singleton() {
        // row 6: cov=100.0 in 100..=100 → appears.
        let v = mk_verdict("full", "src/a.rs", 1, 100.0, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(100.0, 100.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 1);
    }

    #[test]
    fn filters_and_compose() {
        // view.feature l. 70: filters AND-compose.
        // only_failing AND coverage_range [50, 100] → both must hold.
        let r = background_fixture();
        let range = CoverageRange::new(50.0, 100.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                coverage_range: Some(range),
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        for v in &view.shown {
            assert!(v.exceeds);
            let cov = v.scored.coverage_percent;
            assert!((50.0..=100.0).contains(&cov));
        }
    }

    #[test]
    fn nan_coverage_excluded_from_range_filter() {
        // view.feature l. 231: NaN coverage excluded from range filter.
        let v = mk_verdict("zero_lines", "src/a.rs", 1, f64::NAN, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(0.0, 100.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.is_empty());
    }

    // ── Sort ───────────────────────────────────────────────────────

    #[test]
    fn sort_by_crap_descending() {
        // view.feature ll. 95-98.
        let r = background_fixture();
        let spec = ViewSpec {
            sort: SortKey::Crap,
            ..Default::default()
        };
        let view = apply(&r, spec);
        for w in view.shown.windows(2) {
            assert!(w[0].scored.crap.value >= w[1].scored.crap.value);
        }
    }

    #[test]
    fn sort_by_coverage_ascending() {
        // view.feature ll. 100-103.
        let r = background_fixture();
        let spec = ViewSpec {
            sort: SortKey::Coverage,
            ..Default::default()
        };
        let view = apply(&r, spec);
        for w in view.shown.windows(2) {
            assert!(w[0].scored.coverage_percent <= w[1].scored.coverage_percent);
        }
    }

    #[test]
    fn sort_by_complexity_descending() {
        // view.feature ll. 105-108.
        let r = background_fixture();
        let spec = ViewSpec {
            sort: SortKey::Complexity,
            ..Default::default()
        };
        let view = apply(&r, spec);
        for w in view.shown.windows(2) {
            assert!(w[0].scored.complexity >= w[1].scored.complexity);
        }
    }

    #[test]
    fn sort_by_path_alphabetical_then_crap() {
        // view.feature ll. 110-114.
        let r = background_fixture();
        let spec = ViewSpec {
            sort: SortKey::Path,
            ..Default::default()
        };
        let view = apply(&r, spec);
        // Primary: file_path ascending.
        for w in view.shown.windows(2) {
            let (a, b) = (
                &w[0].scored.identity.file_path,
                &w[1].scored.identity.file_path,
            );
            assert!(a <= b, "files not in ascending order: {a} then {b}");
        }
        // Within each file: CRAP descending.
        for w in view.shown.windows(2) {
            if w[0].scored.identity.file_path == w[1].scored.identity.file_path {
                assert!(
                    w[0].scored.crap.value >= w[1].scored.crap.value,
                    "within file {}: CRAP not descending: {} then {}",
                    w[0].scored.identity.file_path,
                    w[0].scored.crap.value,
                    w[1].scored.crap.value
                );
            }
        }
    }

    #[test]
    fn sort_by_path_secondary_multi_file() {
        // view.feature ll. 116-120: 3 files, 5 verdicts.
        // src/a.rs (5, 30), src/b.rs (10), src/c.rs (1, 50)
        // Expected: a.rs::30, a.rs::5, b.rs::10, c.rs::50, c.rs::1
        let verdicts = vec![
            mk_verdict("a_low", "src/a.rs", 1, 50.0, 5.0, 25.0),
            mk_verdict("a_high", "src/a.rs", 1, 50.0, 30.0, 25.0),
            mk_verdict("b_only", "src/b.rs", 1, 50.0, 10.0, 25.0),
            mk_verdict("c_low", "src/c.rs", 1, 50.0, 1.0, 25.0),
            mk_verdict("c_high", "src/c.rs", 1, 50.0, 50.0, 25.0),
        ];
        let r = AnalysisResult {
            functions: verdicts,
            summary: empty_result().summary,
            passed: true,
        };
        let spec = ViewSpec {
            sort: SortKey::Path,
            ..Default::default()
        };
        let view = apply(&r, spec);
        let names: Vec<&str> = view
            .shown
            .iter()
            .map(|v| v.scored.identity.qualified_name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["a_high", "a_low", "b_only", "c_high", "c_low"],
            "path sort with secondary CRAP-desc order wrong"
        );
    }

    #[test]
    fn nan_coverage_sorts_last_under_coverage_ascending() {
        // view.feature ll. 237-242. Coverages: [10.0, NaN, 50.0, NaN, 90.0].
        let verdicts = vec![
            mk_verdict("c10", "src/a.rs", 1, 10.0, 1.0, 25.0),
            mk_verdict("nan1", "src/a.rs", 1, f64::NAN, 1.0, 25.0),
            mk_verdict("c50", "src/a.rs", 1, 50.0, 1.0, 25.0),
            mk_verdict("nan2", "src/a.rs", 1, f64::NAN, 1.0, 25.0),
            mk_verdict("c90", "src/a.rs", 1, 90.0, 1.0, 25.0),
        ];
        let r = AnalysisResult {
            functions: verdicts,
            summary: empty_result().summary,
            passed: true,
        };
        let spec = ViewSpec {
            sort: SortKey::Coverage,
            ..Default::default()
        };
        let view = apply(&r, spec);
        // First 3 are non-NaN ascending; last 2 are NaN.
        let coverages: Vec<f64> = view
            .shown
            .iter()
            .map(|v| v.scored.coverage_percent)
            .collect();
        assert_eq!(coverages[0], 10.0);
        assert_eq!(coverages[1], 50.0);
        assert_eq!(coverages[2], 90.0);
        assert!(coverages[3].is_nan());
        assert!(coverages[4].is_nan());
    }

    // ── Truncate ───────────────────────────────────────────────────

    #[test]
    fn limit_truncates() {
        // view.feature ll. 132-137. Background has 6 functions; limit=3.
        let r = background_fixture();
        let spec = ViewSpec {
            limit: Some(3),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 3);
        assert_eq!(view.eligible_count, 6);
        assert!(view.truncated);
    }

    #[test]
    fn limit_greater_than_eligible() {
        // view.feature ll. 139-144.
        let r = background_fixture();
        let spec = ViewSpec {
            limit: Some(100),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 6);
        assert_eq!(view.eligible_count, 6);
        assert!(!view.truncated);
    }

    #[test]
    fn limit_none() {
        // view.feature ll. 146-150.
        let r = background_fixture();
        let spec = ViewSpec {
            limit: None,
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), view.eligible_count);
        assert!(!view.truncated);
    }

    #[test]
    fn limit_zero_treated_as_no_limit() {
        // view.feature l. 213. --top 0 ⇒ limit = None semantics.
        // Construct directly with Some(0); the code treats it as no-limit.
        let r = background_fixture();
        let spec = ViewSpec {
            limit: Some(0),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), view.eligible_count);
        assert!(!view.truncated);
    }

    #[test]
    fn limit_equal_to_eligible_does_not_mark_truncated() {
        // Boundary: shown.len() == limit. Mutation-killer for `>` vs `>=`
        // in `truncate_to`. The data is unchanged either way, but
        // `truncated` MUST stay false when nothing was actually dropped.
        let r = background_fixture();
        assert_eq!(r.functions.len(), 6, "background fixture sanity");
        let spec = ViewSpec {
            limit: Some(6),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 6);
        assert_eq!(view.eligible_count, 6);
        assert!(!view.truncated, "limit == eligible must NOT mark truncated");
    }

    // ── Order of operations ────────────────────────────────────────

    #[test]
    fn order_filter_then_sort_then_truncate() {
        // view.feature ll. 154-160.
        // only_failing AND sort=Coverage AND limit=2
        let r = background_fixture();
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            sort: SortKey::Coverage,
            limit: Some(2),
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 2);
        for v in &view.shown {
            assert!(v.exceeds);
        }
        // Coverage ascending
        assert!(view.shown[0].scored.coverage_percent <= view.shown[1].scored.coverage_percent);
        // eligible_count = total failing functions before truncation
        let total_failing = r.functions.iter().filter(|v| v.exceeds).count();
        assert_eq!(view.eligible_count, total_failing);
    }

    #[test]
    fn truncation_does_not_change_gate() {
        // view.feature ll. 162-168 — Given an analysis with 3 functions
        // exceeding threshold. Construct that fixture explicitly.
        let verdicts = vec![
            mk_verdict("ok", "src/a.rs", 1, 100.0, 1.0, 25.0),
            mk_verdict("fail1", "src/a.rs", 1, 0.0, 50.0, 25.0),
            mk_verdict("fail2", "src/a.rs", 1, 0.0, 60.0, 25.0),
            mk_verdict("fail3", "src/a.rs", 1, 0.0, 70.0, 25.0),
        ];
        let summary = compute_summary(&verdicts);
        let passed = verdicts.iter().all(|v| !v.exceeds);
        let r = AnalysisResult {
            functions: verdicts,
            summary,
            passed,
        };
        let spec = ViewSpec {
            limit: Some(1),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 1);
        // gate = view.full.passed (false) and exceeding count unchanged.
        assert!(!view.full.passed);
        assert_eq!(view.full.summary.exceeding_threshold, 3);
    }

    #[test]
    fn filtering_does_not_change_gate() {
        // view.feature ll. 170-175 — analysis with 3 exceeding, filter
        // excludes all of them.
        let verdicts = vec![
            mk_verdict("ok", "src/a.rs", 1, 100.0, 1.0, 25.0),
            mk_verdict("fail1", "src/a.rs", 1, 0.0, 50.0, 25.0),
            mk_verdict("fail2", "src/a.rs", 1, 0.0, 60.0, 25.0),
            mk_verdict("fail3", "src/a.rs", 1, 0.0, 70.0, 25.0),
        ];
        let summary = compute_summary(&verdicts);
        let r = AnalysisResult {
            functions: verdicts,
            summary,
            passed: false,
        };
        // Range [99, 100] keeps "ok" only; excludes the 3 failing ones.
        let range = CoverageRange::new(99.0, 100.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.iter().all(|v| !v.exceeds));
        assert!(!view.full.passed);
        assert_eq!(view.full.summary.exceeding_threshold, 3);
    }

    // ── shown_summary ──────────────────────────────────────────────

    #[test]
    fn shown_summary_over_shown_subset() {
        // view.feature ll. 179-186.
        let r = background_fixture();
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown_summary.total_functions, view.shown.len());
        assert_eq!(
            view.shown_summary.exceeding_threshold,
            view.shown.len(),
            "every shown row exceeds, so shown_summary should report all"
        );
        // Manual check: avg
        let manual_avg: f64 =
            view.shown.iter().map(|v| v.scored.crap.value).sum::<f64>() / view.shown.len() as f64;
        assert!((view.shown_summary.average_crap - manual_avg).abs() < 1e-9);
    }

    #[test]
    fn shown_summary_differs_from_full() {
        // view.feature ll. 188-193 — analysis with 6 functions, 3 exceeding.
        let verdicts = vec![
            mk_verdict("ok1", "src/a.rs", 1, 100.0, 1.0, 25.0),
            mk_verdict("ok2", "src/a.rs", 1, 100.0, 2.0, 25.0),
            mk_verdict("ok3", "src/a.rs", 1, 100.0, 3.0, 25.0),
            mk_verdict("fail1", "src/a.rs", 1, 0.0, 50.0, 25.0),
            mk_verdict("fail2", "src/a.rs", 1, 0.0, 60.0, 25.0),
            mk_verdict("fail3", "src/a.rs", 1, 0.0, 70.0, 25.0),
        ];
        let summary = compute_summary(&verdicts);
        let r = AnalysisResult {
            functions: verdicts,
            summary,
            passed: false,
        };
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.full.summary.total_functions, 6);
        assert_eq!(view.shown_summary.total_functions, 3);
    }

    // ── Edge cases ─────────────────────────────────────────────────

    #[test]
    fn all_filtered_out_produces_empty_shown() {
        // view.feature ll. 205-211.
        let v = mk_verdict("low_cov", "src/a.rs", 1, 50.0, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(95.0, 100.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.is_empty());
        assert_eq!(view.eligible_count, 0);
        assert!(!view.truncated);
    }

    // ── should_render_view_line — display predicate ────────────────

    #[test]
    fn display_predicate_default_spec_is_false() {
        // Default invocation: no filtering, no truncation.
        let r = background_fixture();
        let view = apply(&r, ViewSpec::default());
        assert!(!should_render_view_line(&view));
    }

    #[test]
    fn display_predicate_sort_only_is_false() {
        // Sort-only invocation: still false (no rows reduced).
        let r = background_fixture();
        let spec = ViewSpec {
            sort: SortKey::Coverage,
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(!should_render_view_line(&view));
    }

    #[test]
    fn display_predicate_top_truncating_is_true() {
        // --top truncating: true.
        let r = background_fixture();
        let spec = ViewSpec {
            limit: Some(2),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(should_render_view_line(&view));
    }

    #[test]
    fn display_predicate_coverage_filter_excluding_is_true() {
        // Coverage filter that excludes: true.
        let r = background_fixture();
        let range = CoverageRange::new(99.0, 100.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(should_render_view_line(&view));
    }

    #[test]
    fn display_predicate_only_failing_reducing_is_true() {
        // --only-failing with some passing rows: true.
        let r = background_fixture();
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(should_render_view_line(&view));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::test_strategies::{arb_analysis_result, arb_verdict_with_nan_coverage};
    use proptest::prelude::*;

    /// Mirror the legacy `format_table` sort: CRAP descending with stable
    /// fallback to input order on ties.
    fn legacy_sort_order(result: &AnalysisResult) -> Vec<&FunctionVerdict> {
        let mut sorted: Vec<&FunctionVerdict> = result.functions.iter().collect();
        sorted.sort_by(|a, b| {
            b.scored
                .crap
                .value
                .partial_cmp(&a.scored.crap.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Invariant 1 (Order): `apply(r, ViewSpec::default()).shown` matches
        /// the legacy sort: CRAP descending, stable on ties.
        #[test]
        fn prop_default_spec_order_matches_legacy_sort(result in arb_analysis_result()) {
            let view = apply(&result, ViewSpec::default());
            let legacy = legacy_sort_order(&result);
            let view_names: Vec<&String> =
                view.shown.iter().map(|v| &v.scored.identity.qualified_name).collect();
            let legacy_names: Vec<&String> =
                legacy.iter().map(|v| &v.scored.identity.qualified_name).collect();
            // Compare by (name, file) to disambiguate duplicates in the strategy.
            // proptest generates unique enough fixtures that names typically suffice.
            prop_assert_eq!(view_names.len(), legacy_names.len());
            for (vname, lname) in view_names.iter().zip(legacy_names.iter()) {
                prop_assert_eq!(vname, lname);
            }
        }

        /// Invariant 2 (Identity): the set of identities is preserved.
        #[test]
        fn prop_default_spec_preserves_identity(result in arb_analysis_result()) {
            let view = apply(&result, ViewSpec::default());
            let shown_identities: std::collections::HashSet<&crate::domain::types::FunctionIdentity> =
                view.shown.iter().map(|v| &v.scored.identity).collect();
            let original_identities: std::collections::HashSet<&crate::domain::types::FunctionIdentity> =
                result.functions.iter().map(|v| &v.scored.identity).collect();
            prop_assert_eq!(shown_identities, original_identities);
        }

        /// Invariant 3 (Summary): `view.full` borrows the original result.
        /// Stronger than equals — pointer equality.
        #[test]
        fn prop_default_spec_preserves_summary(result in arb_analysis_result()) {
            let view = apply(&result, ViewSpec::default());
            prop_assert!(std::ptr::eq(view.full, &result));
            // and shape: total_functions agrees
            prop_assert_eq!(view.full.summary.total_functions, result.summary.total_functions);
        }

        /// Invariant 4 (Display): biconditional of the predicate.
        #[test]
        fn prop_display_predicate_biconditional(result in arb_analysis_result()) {
            // Default spec: predicate must be false (no rows reduced).
            let view = apply(&result, ViewSpec::default());
            let computed = should_render_view_line(&view);
            let expected = view.eligible_count < view.full.functions.len() || view.truncated;
            prop_assert_eq!(computed, expected);
            // Default invocation reduces nothing → both sides false.
            prop_assert!(!computed);
        }

        /// `apply` never panics on NaN-coverage inputs.
        #[test]
        fn prop_apply_never_panics_with_nan_coverage(
            verdicts in prop::collection::vec(arb_verdict_with_nan_coverage(), 0..50)
        ) {
            let result = AnalysisResult {
                functions: verdicts.clone(),
                summary: crate::domain::types::AnalysisSummary {
                    total_functions: verdicts.len(),
                    total_files: verdicts.len(),
                    exceeding_threshold: 0,
                    average_crap: 0.0,
                    median_crap: 0.0,
                    max_crap: None,
                    worst_function: None,
                    distribution: crate::domain::types::RiskDistribution {
                        low: 0, acceptable: 0, moderate: 0, high: 0,
                    },
                },
                passed: true,
            };
            // Try every SortKey + a coverage-range filter.
            for sort in [SortKey::Crap, SortKey::Coverage, SortKey::Complexity, SortKey::Path] {
                let spec = ViewSpec {
                    filters: Filters {
                        coverage_range: Some(CoverageRange::new(0.0, 100.0).unwrap()),
                        ..Default::default()
                    },
                    sort,
                    limit: Some(10),
                };
                let _ = apply(&result, spec);
            }
        }
    }
}
