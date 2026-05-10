use serde::Serialize;

use super::delta::{DeltaSummary, FunctionChange};
use super::types::{
    AnalysisResult, AnalysisSummary, CrapScore, FunctionIdentity, FunctionVerdict,
    RiskDistribution, RiskLevel,
};

/// Compute an `AnalysisSummary` from any iterable of `&FunctionVerdict`.
///
/// Accepting an `IntoIterator` instead of a slice lets callers pass either
/// `&[FunctionVerdict]` (the typical core path) or `&[&FunctionVerdict]`
/// (the `view::apply` path, where `shown` is already a vec of borrows).
/// This avoids the deep clone that would otherwise be required to
/// materialise an owned slice on every `view::apply` invocation.
pub fn compute_summary<'a, I>(verdicts: I) -> AnalysisSummary
where
    I: IntoIterator<Item = &'a FunctionVerdict>,
{
    let mut distribution = RiskDistribution {
        low: 0,
        acceptable: 0,
        moderate: 0,
        high: 0,
    };

    let mut scores: Vec<f64> = Vec::new();
    let mut complexities: Vec<u32> = Vec::new();
    let mut finite_coverages: Vec<f64> = Vec::new();
    let mut files: std::collections::HashSet<&'a String> = std::collections::HashSet::new();
    let mut exceeding: usize = 0;
    let mut max_crap = None;
    let mut worst_function = None;
    let mut max_complexity: u32 = 0;

    for v in verdicts {
        let score = v.scored.crap.value;
        scores.push(score);
        complexities.push(v.scored.complexity);
        let cov = v.scored.coverage_percent;
        if cov.is_finite() {
            finite_coverages.push(cov);
        }
        files.insert(&v.scored.identity.file_path);

        if v.exceeds {
            exceeding += 1;
        }

        match v.scored.crap.risk_level {
            RiskLevel::Low => distribution.low += 1,
            RiskLevel::Acceptable => distribution.acceptable += 1,
            RiskLevel::Moderate => distribution.moderate += 1,
            RiskLevel::High => distribution.high += 1,
        }

        if max_crap.is_none() || score > max_crap.unwrap_or(0.0) {
            max_crap = Some(score);
            worst_function = Some(v.scored.identity.clone());
        }
        if v.scored.complexity > max_complexity {
            max_complexity = v.scored.complexity;
        }
    }

    let total_functions = scores.len();
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    complexities.sort_unstable();
    finite_coverages.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let average_crap = mean_f64(&scores);
    let median_crap = median_f64_sorted(&scores);

    let average_complexity = mean_u32(&complexities);
    let median_complexity = median_u32_sorted(&complexities);

    let min_coverage = finite_coverages.first().copied().unwrap_or(0.0);
    let average_coverage = mean_f64(&finite_coverages);
    let median_coverage = median_f64_sorted(&finite_coverages);

    AnalysisSummary {
        total_functions,
        total_files: files.len(),
        exceeding_threshold: exceeding,
        average_crap,
        median_crap,
        max_crap: max_crap.map(super::crap::classify_risk).map(|risk_level| {
            super::types::CrapScore {
                value: max_crap.unwrap(),
                risk_level,
            }
        }),
        worst_function,
        distribution,
        max_complexity,
        average_complexity,
        median_complexity,
        min_coverage,
        average_coverage,
        median_coverage,
    }
}

fn mean_f64(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn mean_u32(values: &[u32]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().map(|v| *v as f64).sum::<f64>() / values.len() as f64
}

fn median_f64_sorted(sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    if n.is_multiple_of(2) {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    }
}

fn median_u32_sorted(sorted: &[u32]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    if n.is_multiple_of(2) {
        (sorted[n / 2 - 1] as f64 + sorted[n / 2] as f64) / 2.0
    } else {
        sorted[n / 2] as f64
    }
}

// ── Per-file aggregation (issue #64 — `--group-by file`) ────────────

/// Per-file aggregate over a `FunctionVerdict` partition.
///
/// The partition key is `FunctionIdentity::file_path`. Aggregates are
/// pure: no `syn`, no LCOV, no `PathBuf` — just integers, floats, and
/// the already-domain identity string. Ships unchanged into `crap-core`.
#[derive(Debug, Clone, Serialize)]
pub struct FileSummary {
    pub file_path: String,
    pub function_count: usize,
    pub exceeding_count: usize,
    pub average_crap: f64,
    pub median_crap: f64,
    pub max_crap: Option<CrapScore>,
    pub worst_function: Option<FunctionIdentity>,
    pub distribution: RiskDistribution,
    /// Mean of finite `coverage_percent` values across the file's
    /// functions. NaN inputs are excluded from both numerator and
    /// denominator; if no finite values exist, this is `0.0`.
    /// Drives file-level `--sort-by coverage`.
    pub average_coverage: f64,
    /// Highest `complexity` value across the file's functions.
    /// `0` for an empty bucket (unreachable in practice — `function_count`
    /// is always >= 1 for a returned summary). Drives file-level
    /// `--sort-by complexity`.
    pub max_complexity: u32,
}

/// Group `verdicts` by `file_path` and compute per-file aggregates.
///
/// Order of the returned vec is undefined — callers (e.g. the View)
/// must apply their own sort. Empty input returns an empty vec.
///
/// Mirrors `compute_summary`'s `IntoIterator<Item = &'a FunctionVerdict>`
/// signature so it composes with `view::apply` without forcing a clone.
pub fn compute_file_summaries<'a, I>(verdicts: I) -> Vec<FileSummary>
where
    I: IntoIterator<Item = &'a FunctionVerdict>,
{
    // Stable insertion order keeps the output deterministic for fixture
    // tests; callers that want a specific order sort downstream.
    let mut order: Vec<&'a String> = Vec::new();
    let mut buckets: std::collections::HashMap<&'a String, Vec<&'a FunctionVerdict>> =
        std::collections::HashMap::new();

    for v in verdicts {
        let path = &v.scored.identity.file_path;
        if !buckets.contains_key(path) {
            order.push(path);
        }
        buckets.entry(path).or_default().push(v);
    }

    order
        .into_iter()
        .map(|path| {
            let bucket = buckets
                .remove(path)
                .expect("bucket present for ordered key");
            file_summary_for(path.clone(), &bucket)
        })
        .collect()
}

/// Per-verdict accumulator for `file_summary_for`.
///
/// Holds running totals so the per-verdict update is a single fold step
/// without keeping the cognitive complexity of `file_summary_for` itself
/// above the project's strict-CRAP gate (CC ≤ 15).
struct FileAcc {
    sum_crap: f64,
    max_crap_value: Option<f64>,
    worst_function: Option<FunctionIdentity>,
    finite_scores: Vec<f64>,
    sum_finite_coverage: f64,
    finite_coverage_count: usize,
    max_complexity: u32,
    exceeding_count: usize,
    distribution: RiskDistribution,
}

impl FileAcc {
    fn with_capacity(n: usize) -> Self {
        Self {
            sum_crap: 0.0,
            max_crap_value: None,
            worst_function: None,
            finite_scores: Vec::with_capacity(n),
            sum_finite_coverage: 0.0,
            finite_coverage_count: 0,
            max_complexity: 0,
            exceeding_count: 0,
            distribution: RiskDistribution {
                low: 0,
                acceptable: 0,
                moderate: 0,
                high: 0,
            },
        }
    }

    fn fold(&mut self, v: &FunctionVerdict) {
        let score = v.scored.crap.value;
        // Strip NaN from the median set so per-file median is deterministic
        // (CodeRabbit observation on summary.rs:256 — `partial_cmp.unwrap_or(Equal)`
        // can otherwise leave NaN at the middle index). NaN scores still
        // contribute to `sum_crap` to mirror `compute_summary`'s aggregate
        // behavior; in practice the CRAP formula is always finite, so this
        // only matters as a defensive contract.
        self.sum_crap += score;
        if score.is_finite() {
            self.finite_scores.push(score);
        }
        if v.exceeds {
            self.exceeding_count += 1;
        }
        bump_distribution(&mut self.distribution, v.scored.crap.risk_level);
        if beats(self.max_crap_value, score) {
            self.max_crap_value = Some(score);
            self.worst_function = Some(v.scored.identity.clone());
        }
        let cov = v.scored.coverage_percent;
        if cov.is_finite() {
            self.sum_finite_coverage += cov;
            self.finite_coverage_count += 1;
        }
        // equivalent-mutant: `>` vs `>=` here is observationally identical —
        // re-assigning `max_complexity` to the same value is a no-op, so
        // cargo-mutants reports a survivor that does not represent a real bug.
        if v.scored.complexity > self.max_complexity {
            self.max_complexity = v.scored.complexity;
        }
    }
}

fn bump_distribution(d: &mut RiskDistribution, level: RiskLevel) {
    match level {
        RiskLevel::Low => d.low += 1,
        RiskLevel::Acceptable => d.acceptable += 1,
        RiskLevel::Moderate => d.moderate += 1,
        RiskLevel::High => d.high += 1,
    }
}

/// Strict-greater: first verdict at the max wins (matches `compute_summary`'s
/// "first wins" semantics on ties).
fn beats(curr: Option<f64>, score: f64) -> bool {
    match curr {
        None => true,
        Some(c) => score > c,
    }
}

fn safe_avg(sum: f64, count: usize) -> f64 {
    if count > 0 { sum / count as f64 } else { 0.0 }
}

fn file_summary_for(file_path: String, verdicts: &[&FunctionVerdict]) -> FileSummary {
    let function_count = verdicts.len();
    let mut acc = FileAcc::with_capacity(function_count);
    for v in verdicts {
        acc.fold(v);
    }
    FileSummary {
        file_path,
        function_count,
        exceeding_count: acc.exceeding_count,
        average_crap: safe_avg(acc.sum_crap, function_count),
        median_crap: median_of(&mut acc.finite_scores),
        max_crap: acc.max_crap_value.map(|value| CrapScore {
            value,
            risk_level: super::crap::classify_risk(value),
        }),
        worst_function: acc.worst_function,
        distribution: acc.distribution,
        average_coverage: safe_avg(acc.sum_finite_coverage, acc.finite_coverage_count),
        max_complexity: acc.max_complexity,
    }
}

/// Sort-stable median for a non-empty score vector. NaN handling mirrors
/// `compute_summary`: `partial_cmp` falls back to `Equal` so NaN does
/// not panic. Empty input returns `0.0` to match `compute_summary`.
fn median_of(scores: &mut [f64]) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = scores.len();
    if n.is_multiple_of(2) {
        (scores[n / 2 - 1] + scores[n / 2]) / 2.0
    } else {
        scores[n / 2]
    }
}

// ── CrapDeltaRowData (scorecard-row producer, issue #111) ───────────

/// Status mirrors mokumo's `Row::CrapDelta::status` (Green/Yellow/Red).
/// Producer-mints-status under Model P — see crap4rs gap doc at
/// `~/Github/ops/pipelines/crap4rs/crap4rs-20260503-scorecard-row-rollout.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum CrapDeltaStatus {
    Green,
    Yellow,
    Red,
}

impl CrapDeltaStatus {
    /// Canonical wire string — equal to the serde JSON representation
    /// (PascalCase, sans quotes). See
    /// `crate::domain::types::ContributorKind::as_wire_str` for the
    /// rationale; equality with serde is pinned in
    /// `tests::wire_str_matches_serde`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Green => "Green",
            Self::Yellow => "Yellow",
            Self::Red => "Red",
        }
    }
}

/// Language-agnostic record carrying the structured signal a producer
/// projects into a mokumo `Row::CrapDelta` JSON object. Lives in
/// `domain/summary.rs` (pure-domain — no `syn`, no LCOV) so it ships
/// into `crap-core` as a rename-only move during the future unification
/// (ops#231). The reporter at `adapters/reporters/scorecard_row.rs`
/// wires this record into the locked V3-symmetric wire shape.
#[derive(Debug, Clone, Serialize)]
pub struct CrapDeltaRowData {
    pub status: CrapDeltaStatus,
    pub threshold: u32,
    pub delta_count: i32,
    pub delta_text: String,
    pub failure_detail_md: Option<String>,
}

/// Project the scorecard-row signal from a current analysis +
/// optional baseline + delta. Status is producer-side (Model P):
///
/// - **Red** when `delta_summary.new_violations > 0`.
/// - **Yellow** when no new violations but `regressions > 0`.
/// - **Green** otherwise.
///
/// When `baseline` and `delta` are `None` (single-snapshot mode), status
/// is Green if no functions exceed the threshold, Red otherwise; the
/// row treats baseline_count = 0 so `delta_count == current_count`.
pub fn project_crap_delta_row(
    current: &AnalysisResult,
    baseline: Option<&AnalysisResult>,
    delta: Option<(&DeltaSummary, &[FunctionChange])>,
    threshold: u32,
) -> CrapDeltaRowData {
    let current_count = current.summary.exceeding_threshold;
    let baseline_count = baseline.map_or(0, |b| b.summary.exceeding_threshold);
    let delta_count = current_count as i32 - baseline_count as i32;

    let (status, delta_text, failure_detail_md) = match delta {
        Some((delta_summary, changes)) => resolve_with_delta(
            baseline_count,
            current_count,
            delta_count,
            delta_summary,
            changes,
            threshold,
        ),
        None => resolve_no_baseline(current, current_count, threshold),
    };

    CrapDeltaRowData {
        status,
        threshold,
        delta_count,
        delta_text,
        failure_detail_md,
    }
}

fn resolve_with_delta(
    baseline_count: usize,
    current_count: usize,
    delta_count: i32,
    delta_summary: &DeltaSummary,
    changes: &[FunctionChange],
    threshold: u32,
) -> (CrapDeltaStatus, String, Option<String>) {
    if delta_summary.new_violations > 0 {
        let detail = render_delta_failure_detail(changes, threshold);
        let text = format_delta_text_red(baseline_count, current_count, delta_count);
        (CrapDeltaStatus::Red, text, Some(detail))
    } else if delta_summary.regressions > 0 {
        let text =
            format!("{baseline_count} → {current_count} (regressions on existing functions)");
        (CrapDeltaStatus::Yellow, text, None)
    } else {
        let text = format_delta_text_green(baseline_count, current_count, delta_count);
        (CrapDeltaStatus::Green, text, None)
    }
}

fn resolve_no_baseline(
    current: &AnalysisResult,
    current_count: usize,
    threshold: u32,
) -> (CrapDeltaStatus, String, Option<String>) {
    if current_count == 0 {
        (
            CrapDeltaStatus::Green,
            "0 over threshold (no baseline)".to_string(),
            None,
        )
    } else {
        let detail = render_no_baseline_failure_detail(current, threshold);
        let text = format!("{current_count} over threshold (no baseline)");
        (CrapDeltaStatus::Red, text, Some(detail))
    }
}

fn format_delta_text_red(baseline: usize, current: usize, delta: i32) -> String {
    format!("{baseline} → {current} ({delta:+})")
}

fn format_delta_text_green(baseline: usize, current: usize, delta: i32) -> String {
    if delta == 0 {
        format!("{baseline} → {current}")
    } else {
        format!("{baseline} → {current} ({delta:+})")
    }
}

fn render_delta_failure_detail(changes: &[FunctionChange], threshold: u32) -> String {
    let mut violators: Vec<NewViolator> = changes
        .iter()
        .filter_map(NewViolator::from_change)
        .collect();
    violators.sort_by(|a, b| {
        b.current_crap
            .partial_cmp(&a.current_crap)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out = format!("**New CRAP threshold violations (>{threshold}):**\n");
    for v in &violators {
        let baseline_str = match v.baseline_crap {
            Some(b) => format!("was {b:.1}"),
            None => "newly added".to_string(),
        };
        out.push_str(&format!(
            "- `{name}` — `{file}:{line}` — CRAP {current:.1} ({baseline_str})\n",
            name = v.qualified_name,
            file = v.file_path,
            line = v.line,
            current = v.current_crap,
        ));
    }
    out
}

fn render_no_baseline_failure_detail(result: &AnalysisResult, threshold: u32) -> String {
    let mut violators: Vec<&FunctionVerdict> =
        result.functions.iter().filter(|v| v.exceeds).collect();
    violators.sort_by(|a, b| {
        b.scored
            .crap
            .value
            .partial_cmp(&a.scored.crap.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out = format!("**Functions over CRAP threshold (>{threshold}):**\n");
    for v in &violators {
        out.push_str(&format!(
            "- `{name}` — `{file}:{line}` — CRAP {crap:.1}\n",
            name = v.scored.identity.qualified_name,
            file = v.scored.identity.file_path,
            line = v.scored.identity.span.start_line,
            crap = v.scored.crap.value,
        ));
    }
    out
}

struct NewViolator {
    qualified_name: String,
    file_path: String,
    line: usize,
    current_crap: f64,
    baseline_crap: Option<f64>,
}

impl NewViolator {
    fn from_change(change: &FunctionChange) -> Option<Self> {
        match change {
            FunctionChange::Added { current } if current.exceeds => Some(Self {
                qualified_name: current.scored.identity.qualified_name.clone(),
                file_path: current.scored.identity.file_path.clone(),
                line: current.scored.identity.span.start_line,
                current_crap: current.scored.crap.value,
                baseline_crap: None,
            }),
            FunctionChange::Modified { baseline, current }
                if !baseline.exceeds && current.exceeds =>
            {
                Some(Self {
                    qualified_name: current.scored.identity.qualified_name.clone(),
                    file_path: current.scored.identity.file_path.clone(),
                    line: current.scored.identity.span.start_line,
                    current_crap: current.scored.crap.value,
                    baseline_crap: Some(baseline.scored.crap.value),
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        ComplexityMetric, CrapScore, FunctionIdentity, ScoredFunction, SourceSpan,
    };

    fn make_verdict(file: &str, name: &str, crap_value: f64, threshold: f64) -> FunctionVerdict {
        let risk_level = super::super::crap::classify_risk(crap_value);
        FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: file.to_string(),
                    qualified_name: name.to_string(),
                    span: SourceSpan {
                        start_line: 1,
                        end_line: 10,
                        start_column: 0,
                        end_column: 0,
                    },
                },
                complexity: 1,
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: 100.0,
                crap: CrapScore {
                    value: crap_value,
                    risk_level,
                },
                contributors: vec![],
            },
            threshold,
            exceeds: crap_value > threshold,
            diagnostic: None,
        }
    }

    #[test]
    fn empty_verdicts() {
        let summary = compute_summary(&[]);
        assert_eq!(summary.total_functions, 0);
        assert_eq!(summary.total_files, 0);
        assert_eq!(summary.exceeding_threshold, 0);
        assert_eq!(summary.average_crap, 0.0);
        assert_eq!(summary.median_crap, 0.0);
        assert!(summary.max_crap.is_none());
        assert!(summary.worst_function.is_none());
        assert_eq!(summary.distribution.low, 0);
        assert_eq!(summary.distribution.acceptable, 0);
        assert_eq!(summary.distribution.moderate, 0);
        assert_eq!(summary.distribution.high, 0);
    }

    #[test]
    fn single_verdict() {
        let v = make_verdict("a.rs", "foo", 3.0, 30.0);
        let summary = compute_summary(&[v]);
        assert_eq!(summary.total_functions, 1);
        assert_eq!(summary.total_files, 1);
        assert_eq!(summary.exceeding_threshold, 0);
        assert_eq!(summary.average_crap, 3.0);
        assert_eq!(summary.median_crap, 3.0);
        assert_eq!(summary.max_crap.unwrap().value, 3.0);
        assert_eq!(
            summary.worst_function.as_ref().unwrap().qualified_name,
            "foo"
        );
    }

    #[test]
    fn odd_count_median() {
        let verdicts = vec![
            make_verdict("a.rs", "a", 1.0, 30.0),
            make_verdict("a.rs", "b", 5.0, 30.0),
            make_verdict("a.rs", "c", 9.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.median_crap, 5.0);
    }

    #[test]
    fn even_count_median() {
        let verdicts = vec![
            make_verdict("a.rs", "a", 2.0, 30.0),
            make_verdict("a.rs", "b", 4.0, 30.0),
            make_verdict("a.rs", "c", 6.0, 30.0),
            make_verdict("a.rs", "d", 8.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        // Median of [2, 4, 6, 8] = (4 + 6) / 2 = 5.0
        assert_eq!(summary.median_crap, 5.0);
    }

    #[test]
    fn distribution_counting() {
        let verdicts = vec![
            make_verdict("a.rs", "low", 2.0, 30.0),        // Low (<=5)
            make_verdict("a.rs", "acceptable", 6.0, 30.0), // Acceptable (<=8)
            make_verdict("a.rs", "moderate", 15.0, 30.0),  // Moderate (<=30)
            make_verdict("a.rs", "high", 50.0, 30.0),      // High (>30)
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.distribution.low, 1);
        assert_eq!(summary.distribution.acceptable, 1);
        assert_eq!(summary.distribution.moderate, 1);
        assert_eq!(summary.distribution.high, 1);
    }

    #[test]
    fn max_crap_and_worst_function() {
        let verdicts = vec![
            make_verdict("a.rs", "small", 2.0, 30.0),
            make_verdict("b.rs", "big", 50.0, 30.0),
            make_verdict("c.rs", "medium", 10.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.max_crap.unwrap().value, 50.0);
        assert_eq!(
            summary.worst_function.as_ref().unwrap().qualified_name,
            "big"
        );
    }

    #[test]
    fn file_deduplication() {
        let verdicts = vec![
            make_verdict("a.rs", "foo", 2.0, 30.0),
            make_verdict("a.rs", "bar", 3.0, 30.0),
            make_verdict("b.rs", "baz", 4.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.total_functions, 3);
        assert_eq!(summary.total_files, 2);
    }

    #[test]
    fn exceeding_threshold_count() {
        let verdicts = vec![
            make_verdict("a.rs", "ok", 5.0, 10.0),     // below
            make_verdict("a.rs", "bad", 15.0, 10.0),   // exceeds
            make_verdict("a.rs", "worse", 50.0, 10.0), // exceeds
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.exceeding_threshold, 2);
    }

    #[test]
    fn average_calculation() {
        let verdicts = vec![
            make_verdict("a.rs", "a", 3.0, 30.0),
            make_verdict("a.rs", "b", 6.0, 30.0),
            make_verdict("a.rs", "c", 9.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.average_crap, 6.0);
    }

    #[test]
    fn tied_scores_first_wins_worst_function() {
        let verdicts = vec![
            make_verdict("a.rs", "first", 10.0, 30.0),
            make_verdict("a.rs", "second", 10.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(
            summary.worst_function.as_ref().unwrap().qualified_name,
            "first"
        );
    }
}

#[cfg(test)]
mod file_summary_tests {
    use super::*;
    use crate::domain::types::{
        ComplexityMetric, CrapScore, FunctionIdentity, ScoredFunction, SourceSpan,
    };

    fn vrd(file: &str, name: &str, crap_value: f64, threshold: f64) -> FunctionVerdict {
        let risk_level = super::super::crap::classify_risk(crap_value);
        FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: file.to_string(),
                    qualified_name: name.to_string(),
                    span: SourceSpan {
                        start_line: 1,
                        end_line: 10,
                        start_column: 0,
                        end_column: 0,
                    },
                },
                complexity: 1,
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: 100.0,
                crap: CrapScore {
                    value: crap_value,
                    risk_level,
                },
                contributors: vec![],
            },
            threshold,
            exceeds: crap_value > threshold,
            diagnostic: None,
        }
    }

    #[test]
    fn empty_input_returns_empty_vec() {
        // P8 — empty robustness.
        let result = compute_file_summaries(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn single_file_single_function() {
        let v = vrd("a.rs", "foo", 3.0, 25.0);
        let summaries = compute_file_summaries(&[v]);
        assert_eq!(summaries.len(), 1);
        let f = &summaries[0];
        assert_eq!(f.file_path, "a.rs");
        assert_eq!(f.function_count, 1);
        assert_eq!(f.exceeding_count, 0);
        assert_eq!(f.average_crap, 3.0);
        assert_eq!(f.median_crap, 3.0);
        assert_eq!(f.max_crap.unwrap().value, 3.0);
        assert_eq!(f.worst_function.as_ref().unwrap().qualified_name, "foo");
    }

    #[test]
    fn partition_completeness() {
        // P1: sum(file.function_count) == verdicts.len()
        let verdicts = vec![
            vrd("a.rs", "a1", 1.0, 25.0),
            vrd("a.rs", "a2", 2.0, 25.0),
            vrd("b.rs", "b1", 30.0, 25.0),
            vrd("c.rs", "c1", 4.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let total: usize = summaries.iter().map(|f| f.function_count).sum();
        assert_eq!(total, verdicts.len());
    }

    #[test]
    fn distinct_files_are_grouped() {
        // P3: len == distinct file paths
        let verdicts = vec![
            vrd("a.rs", "a1", 1.0, 25.0),
            vrd("a.rs", "a2", 2.0, 25.0),
            vrd("b.rs", "b1", 30.0, 25.0),
            vrd("c.rs", "c1", 4.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        assert_eq!(summaries.len(), 3);
        let mut paths: Vec<&str> = summaries.iter().map(|f| f.file_path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, vec!["a.rs", "b.rs", "c.rs"]);
    }

    #[test]
    fn exceeding_count_aggregates_per_file() {
        // P2 (per-file).
        let verdicts = vec![
            vrd("a.rs", "ok", 5.0, 10.0),
            vrd("a.rs", "bad1", 15.0, 10.0),
            vrd("a.rs", "bad2", 50.0, 10.0),
            vrd("b.rs", "ok2", 3.0, 10.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let a = summaries.iter().find(|f| f.file_path == "a.rs").unwrap();
        let b = summaries.iter().find(|f| f.file_path == "b.rs").unwrap();
        assert_eq!(a.exceeding_count, 2);
        assert_eq!(b.exceeding_count, 0);
    }

    #[test]
    fn exceeding_count_sum_matches_total() {
        // P2 (global): sum(file.exceeding_count) == verdicts.iter().filter(|v| v.exceeds).count().
        let verdicts = vec![
            vrd("a.rs", "ok", 5.0, 10.0),
            vrd("a.rs", "bad1", 15.0, 10.0),
            vrd("b.rs", "bad2", 50.0, 10.0),
            vrd("c.rs", "ok2", 3.0, 10.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let sum: usize = summaries.iter().map(|f| f.exceeding_count).sum();
        let manual = verdicts.iter().filter(|v| v.exceeds).count();
        assert_eq!(sum, manual);
    }

    #[test]
    fn max_crap_per_file_correct() {
        // P4: per-file max_crap matches manual max.
        let verdicts = vec![
            vrd("a.rs", "low", 5.0, 25.0),
            vrd("a.rs", "high", 50.0, 25.0),
            vrd("b.rs", "med", 12.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let a = summaries.iter().find(|f| f.file_path == "a.rs").unwrap();
        let b = summaries.iter().find(|f| f.file_path == "b.rs").unwrap();
        assert_eq!(a.max_crap.unwrap().value, 50.0);
        assert_eq!(a.worst_function.as_ref().unwrap().qualified_name, "high");
        assert_eq!(b.max_crap.unwrap().value, 12.0);
    }

    #[test]
    fn average_crap_per_file_correct() {
        // P5: per-file average_crap matches manual mean.
        let verdicts = vec![
            vrd("a.rs", "a1", 4.0, 25.0),
            vrd("a.rs", "a2", 6.0, 25.0),
            vrd("a.rs", "a3", 14.0, 25.0),
            vrd("b.rs", "b1", 3.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let a = summaries.iter().find(|f| f.file_path == "a.rs").unwrap();
        let b = summaries.iter().find(|f| f.file_path == "b.rs").unwrap();
        // (4 + 6 + 14) / 3 = 8.0
        assert!((a.average_crap - 8.0).abs() < 1e-9);
        assert!((b.average_crap - 3.0).abs() < 1e-9);
    }

    #[test]
    fn median_per_file_odd_and_even() {
        let verdicts = vec![
            vrd("a.rs", "a1", 1.0, 25.0),
            vrd("a.rs", "a2", 5.0, 25.0),
            vrd("a.rs", "a3", 9.0, 25.0),
            vrd("b.rs", "b1", 2.0, 25.0),
            vrd("b.rs", "b2", 8.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let a = summaries.iter().find(|f| f.file_path == "a.rs").unwrap();
        let b = summaries.iter().find(|f| f.file_path == "b.rs").unwrap();
        assert_eq!(a.median_crap, 5.0);
        assert_eq!(b.median_crap, 5.0); // (2+8)/2
    }

    #[test]
    fn distribution_per_file() {
        let verdicts = vec![
            vrd("a.rs", "low", 2.0, 25.0),        // Low (<=5)
            vrd("a.rs", "acceptable", 6.0, 25.0), // Acceptable (<=8)
            vrd("a.rs", "moderate", 15.0, 25.0),  // Moderate (<=30)
            vrd("a.rs", "high", 50.0, 25.0),      // High (>30)
        ];
        let summaries = compute_file_summaries(&verdicts);
        assert_eq!(summaries.len(), 1);
        let d = &summaries[0].distribution;
        assert_eq!(d.low, 1);
        assert_eq!(d.acceptable, 1);
        assert_eq!(d.moderate, 1);
        assert_eq!(d.high, 1);
    }

    #[test]
    fn nan_coverage_does_not_panic() {
        // P8 (NaN). Coverage NaN does not affect file aggregation since
        // file aggregates are over CRAP not coverage; assert no panic.
        let v = FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: "a.rs".to_string(),
                    qualified_name: "f".to_string(),
                    span: SourceSpan {
                        start_line: 1,
                        end_line: 10,
                        start_column: 0,
                        end_column: 0,
                    },
                },
                complexity: 1,
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: f64::NAN,
                crap: CrapScore {
                    value: 5.0,
                    risk_level: RiskLevel::Low,
                },
                contributors: vec![],
            },
            threshold: 25.0,
            exceeds: false,
            diagnostic: None,
        };
        let summaries = compute_file_summaries(&[v]);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].function_count, 1);
        // Defensive: all-NaN coverage produces 0 finite count; the guard
        // in `file_summary_for` must keep `average_coverage` at 0.0 (not NaN).
        assert_eq!(summaries[0].average_coverage, 0.0);
        assert!(!summaries[0].average_coverage.is_nan());
    }

    #[test]
    fn file_summary_for_empty_input_does_not_divide_by_zero() {
        // `file_summary_for` is private and `compute_file_summaries` never
        // calls it with empty input — but the defensive guards in the body
        // (`function_count > 0`, `finite_coverage_count > 0`) lock the
        // contract: an empty-bucket FileSummary has zeroed averages, never
        // NaN. This test pins both guards by direct invocation.
        let summary = super::file_summary_for("empty.rs".to_string(), &[]);
        assert_eq!(summary.function_count, 0);
        assert_eq!(summary.average_crap, 0.0);
        assert_eq!(summary.average_coverage, 0.0);
        assert!(!summary.average_crap.is_nan());
        assert!(!summary.average_coverage.is_nan());
    }

    #[test]
    fn average_coverage_excludes_nan() {
        // Two finite (50.0, 100.0) and one NaN → mean = 75.0
        let v_finite_a = vrd("a.rs", "f1", 1.0, 25.0); // coverage 100.0 (default in vrd)
        let mut v_finite_b = vrd("a.rs", "f2", 1.0, 25.0);
        v_finite_b.scored.coverage_percent = 50.0;
        let mut v_nan = vrd("a.rs", "f3", 1.0, 25.0);
        v_nan.scored.coverage_percent = f64::NAN;
        let summaries = compute_file_summaries(&[v_finite_a, v_finite_b, v_nan]);
        assert!((summaries[0].average_coverage - 75.0).abs() < 1e-9);
    }

    #[test]
    fn max_complexity_per_file() {
        let mut v1 = vrd("a.rs", "small", 1.0, 25.0);
        v1.scored.complexity = 3;
        let mut v2 = vrd("a.rs", "big", 1.0, 25.0);
        v2.scored.complexity = 17;
        let mut v3 = vrd("a.rs", "med", 1.0, 25.0);
        v3.scored.complexity = 8;
        let summaries = compute_file_summaries(&[v1, v2, v3]);
        assert_eq!(summaries[0].max_complexity, 17);
    }

    #[test]
    fn tied_max_first_wins() {
        // Two functions in the same file at the same CRAP — first wins.
        let verdicts = vec![
            vrd("a.rs", "first", 10.0, 25.0),
            vrd("a.rs", "second", 10.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        assert_eq!(
            summaries[0].worst_function.as_ref().unwrap().qualified_name,
            "first"
        );
    }
}

#[cfg(test)]
mod file_summary_proptests {
    use super::*;
    use crate::test_strategies::arb_verdict;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// P1: partition completeness.
        #[test]
        fn prop_partition_completeness(
            verdicts in prop::collection::vec(arb_verdict(), 0..50)
        ) {
            let summaries = compute_file_summaries(&verdicts);
            let total: usize = summaries.iter().map(|f| f.function_count).sum();
            prop_assert_eq!(total, verdicts.len());
        }

        /// P2: aggregation faithfulness for `exceeding_count`.
        #[test]
        fn prop_exceeding_count_aggregation(
            verdicts in prop::collection::vec(arb_verdict(), 0..50)
        ) {
            let summaries = compute_file_summaries(&verdicts);
            let sum: usize = summaries.iter().map(|f| f.exceeding_count).sum();
            let manual = verdicts.iter().filter(|v| v.exceeds).count();
            prop_assert_eq!(sum, manual);
        }

        /// P3: one row per distinct file path.
        #[test]
        fn prop_one_row_per_distinct_file(
            verdicts in prop::collection::vec(arb_verdict(), 0..50)
        ) {
            let summaries = compute_file_summaries(&verdicts);
            let distinct: std::collections::HashSet<&str> = verdicts
                .iter()
                .map(|v| v.scored.identity.file_path.as_str())
                .collect();
            prop_assert_eq!(summaries.len(), distinct.len());
            // and the file_paths in summaries match the distinct set
            let summary_paths: std::collections::HashSet<&str> =
                summaries.iter().map(|f| f.file_path.as_str()).collect();
            prop_assert_eq!(summary_paths, distinct);
        }

        /// P4: per-file max_crap is the max within the file.
        #[test]
        fn prop_max_crap_correct(
            verdicts in prop::collection::vec(arb_verdict(), 1..50)
        ) {
            let summaries = compute_file_summaries(&verdicts);
            for f in &summaries {
                let in_file: Vec<f64> = verdicts
                    .iter()
                    .filter(|v| v.scored.identity.file_path == f.file_path)
                    .map(|v| v.scored.crap.value)
                    .collect();
                let manual_max = in_file
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max);
                let got = f.max_crap.unwrap().value;
                prop_assert!((got - manual_max).abs() < 1e-9,
                    "file {} max_crap mismatch: got {got}, expected {manual_max}",
                    f.file_path);
            }
        }

        /// P5: per-file average_crap is the mean within the file.
        #[test]
        fn prop_average_crap_correct(
            verdicts in prop::collection::vec(arb_verdict(), 1..50)
        ) {
            let summaries = compute_file_summaries(&verdicts);
            for f in &summaries {
                let in_file: Vec<f64> = verdicts
                    .iter()
                    .filter(|v| v.scored.identity.file_path == f.file_path)
                    .map(|v| v.scored.crap.value)
                    .collect();
                let manual_mean = in_file.iter().sum::<f64>() / in_file.len() as f64;
                prop_assert!((f.average_crap - manual_mean).abs() < 1e-6,
                    "file {} average_crap mismatch: got {}, expected {}",
                    f.file_path, f.average_crap, manual_mean);
            }
        }

        /// P8: never panics on empty / arbitrary inputs.
        #[test]
        fn prop_never_panics(
            verdicts in prop::collection::vec(arb_verdict(), 0..50)
        ) {
            let _ = compute_file_summaries(&verdicts);
        }
    }
}

#[cfg(test)]
mod crap_delta_tests {
    use super::*;
    use crate::domain::delta::{self, AnalysisDelta};
    use crate::domain::types::{
        AnalysisResult, ComplexityMetric, CrapScore, FunctionIdentity, ScoredFunction, SourceSpan,
    };

    fn make_verdict_at(
        file: &str,
        name: &str,
        line: usize,
        crap_value: f64,
        threshold: f64,
    ) -> FunctionVerdict {
        let risk_level = super::super::crap::classify_risk(crap_value);
        FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: file.to_string(),
                    qualified_name: name.to_string(),
                    span: SourceSpan {
                        start_line: line,
                        end_line: line + 5,
                        start_column: 0,
                        end_column: 0,
                    },
                },
                complexity: 1,
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: 100.0,
                crap: CrapScore {
                    value: crap_value,
                    risk_level,
                },
                contributors: vec![],
            },
            threshold,
            exceeds: crap_value > threshold,
            diagnostic: None,
        }
    }

    fn make_result(verdicts: Vec<FunctionVerdict>) -> AnalysisResult {
        let summary = compute_summary(&verdicts);
        let passed = summary.exceeding_threshold == 0;
        AnalysisResult {
            functions: verdicts,
            summary,
            passed,
        }
    }

    fn make_delta(baseline: &AnalysisResult, current: &AnalysisResult) -> AnalysisDelta {
        delta::compute(baseline.clone(), current.clone())
    }

    // ── No-baseline (single-snapshot) mode ───────────────────────────

    #[test]
    fn no_baseline_no_violations_is_green_with_zero_delta() {
        let current = make_result(vec![make_verdict_at("a.rs", "ok", 1, 5.0, 15.0)]);
        let row = project_crap_delta_row(&current, None, None, 15);
        assert_eq!(row.status, CrapDeltaStatus::Green);
        assert_eq!(row.delta_count, 0);
        assert_eq!(row.threshold, 15);
        assert_eq!(row.delta_text, "0 over threshold (no baseline)");
        assert!(row.failure_detail_md.is_none());
    }

    #[test]
    fn no_baseline_with_violations_is_red_with_absolute_count() {
        let current = make_result(vec![
            make_verdict_at("a.rs", "ok", 1, 5.0, 15.0),
            make_verdict_at("a.rs", "bad", 10, 23.4, 15.0),
            make_verdict_at("b.rs", "worse", 4, 30.0, 15.0),
        ]);
        let row = project_crap_delta_row(&current, None, None, 15);
        assert_eq!(row.status, CrapDeltaStatus::Red);
        assert_eq!(row.delta_count, 2);
        assert_eq!(row.delta_text, "2 over threshold (no baseline)");
        let detail = row.failure_detail_md.expect("Red implies failure_detail");
        assert!(detail.starts_with("**Functions over CRAP threshold (>15):**"));
        // Sorted by CRAP descending: worse (30.0) before bad (23.4).
        let worse_idx = detail.find("worse").expect("worse listed");
        let bad_idx = detail.find("bad").expect("bad listed");
        assert!(worse_idx < bad_idx, "expected CRAP-desc order");
        assert!(detail.contains("`b.rs:4`"));
        assert!(detail.contains("CRAP 30.0"));
    }

    // ── Baseline + delta — Red on new violations ─────────────────────

    #[test]
    fn red_when_added_function_exceeds_threshold() {
        let baseline = make_result(vec![make_verdict_at("a.rs", "stable", 1, 5.0, 15.0)]);
        let current = make_result(vec![
            make_verdict_at("a.rs", "stable", 1, 5.0, 15.0),
            make_verdict_at("a.rs", "newly_added", 20, 22.0, 15.0),
        ]);
        let analysis_delta = make_delta(&baseline, &current);
        let row = project_crap_delta_row(
            &current,
            Some(&baseline),
            Some((&analysis_delta.summary, &analysis_delta.changes)),
            15,
        );
        assert_eq!(row.status, CrapDeltaStatus::Red);
        assert_eq!(row.delta_count, 1);
        assert_eq!(row.delta_text, "0 → 1 (+1)");
        let detail = row.failure_detail_md.expect("Red implies failure_detail");
        assert!(detail.contains("newly_added"));
        assert!(detail.contains("newly added"));
    }

    #[test]
    fn red_when_modified_function_crosses_threshold() {
        let baseline = make_result(vec![make_verdict_at("a.rs", "borderline", 1, 11.2, 15.0)]);
        let current = make_result(vec![make_verdict_at("a.rs", "borderline", 1, 23.4, 15.0)]);
        let analysis_delta = make_delta(&baseline, &current);
        let row = project_crap_delta_row(
            &current,
            Some(&baseline),
            Some((&analysis_delta.summary, &analysis_delta.changes)),
            15,
        );
        assert_eq!(row.status, CrapDeltaStatus::Red);
        assert_eq!(row.delta_count, 1);
        assert_eq!(row.delta_text, "0 → 1 (+1)");
        let detail = row.failure_detail_md.expect("Red implies failure_detail");
        assert!(detail.contains("borderline"));
        assert!(detail.contains("CRAP 23.4"));
        assert!(detail.contains("was 11.2"));
    }

    // ── Baseline + delta — Yellow on regression-only ────────────────

    #[test]
    fn yellow_when_existing_function_regresses_below_threshold() {
        let baseline = make_result(vec![make_verdict_at("a.rs", "fn", 1, 5.0, 15.0)]);
        let current = make_result(vec![make_verdict_at("a.rs", "fn", 1, 9.0, 15.0)]);
        let analysis_delta = make_delta(&baseline, &current);
        let row = project_crap_delta_row(
            &current,
            Some(&baseline),
            Some((&analysis_delta.summary, &analysis_delta.changes)),
            15,
        );
        assert_eq!(row.status, CrapDeltaStatus::Yellow);
        assert_eq!(row.delta_count, 0);
        assert_eq!(row.delta_text, "0 → 0 (regressions on existing functions)");
        assert!(row.failure_detail_md.is_none());
    }

    // ── Baseline + delta — Green ─────────────────────────────────────

    #[test]
    fn green_when_no_changes_at_all() {
        let baseline = make_result(vec![make_verdict_at("a.rs", "fn", 1, 5.0, 15.0)]);
        let current = make_result(vec![make_verdict_at("a.rs", "fn", 1, 5.0, 15.0)]);
        let analysis_delta = make_delta(&baseline, &current);
        let row = project_crap_delta_row(
            &current,
            Some(&baseline),
            Some((&analysis_delta.summary, &analysis_delta.changes)),
            15,
        );
        assert_eq!(row.status, CrapDeltaStatus::Green);
        assert_eq!(row.delta_count, 0);
        assert_eq!(row.delta_text, "0 → 0");
        assert!(row.failure_detail_md.is_none());
    }

    #[test]
    fn green_when_violation_dropped_via_improvement() {
        let baseline = make_result(vec![make_verdict_at("a.rs", "was_bad", 1, 22.0, 15.0)]);
        let current = make_result(vec![make_verdict_at("a.rs", "was_bad", 1, 8.0, 15.0)]);
        let analysis_delta = make_delta(&baseline, &current);
        let row = project_crap_delta_row(
            &current,
            Some(&baseline),
            Some((&analysis_delta.summary, &analysis_delta.changes)),
            15,
        );
        assert_eq!(row.status, CrapDeltaStatus::Green);
        assert_eq!(row.delta_count, -1);
        assert_eq!(row.delta_text, "1 → 0 (-1)");
        assert!(row.failure_detail_md.is_none());
    }

    #[test]
    fn green_when_violation_dropped_via_removal() {
        let baseline = make_result(vec![
            make_verdict_at("a.rs", "stable", 1, 5.0, 15.0),
            make_verdict_at("a.rs", "deleted_bad", 20, 22.0, 15.0),
        ]);
        let current = make_result(vec![make_verdict_at("a.rs", "stable", 1, 5.0, 15.0)]);
        let analysis_delta = make_delta(&baseline, &current);
        let row = project_crap_delta_row(
            &current,
            Some(&baseline),
            Some((&analysis_delta.summary, &analysis_delta.changes)),
            15,
        );
        assert_eq!(row.status, CrapDeltaStatus::Green);
        assert_eq!(row.delta_count, -1);
        assert_eq!(row.delta_text, "1 → 0 (-1)");
        assert!(row.failure_detail_md.is_none());
    }

    // ── Failure detail ordering ──────────────────────────────────────

    #[test]
    fn red_detail_lists_violators_sorted_by_crap_descending() {
        let baseline = make_result(vec![]);
        let current = make_result(vec![
            make_verdict_at("a.rs", "low_violator", 1, 16.0, 15.0),
            make_verdict_at("b.rs", "high_violator", 1, 30.0, 15.0),
            make_verdict_at("c.rs", "mid_violator", 1, 22.0, 15.0),
        ]);
        let analysis_delta = make_delta(&baseline, &current);
        let row = project_crap_delta_row(
            &current,
            Some(&baseline),
            Some((&analysis_delta.summary, &analysis_delta.changes)),
            15,
        );
        let detail = row.failure_detail_md.expect("Red implies failure_detail");
        let high_idx = detail.find("high_violator").unwrap();
        let mid_idx = detail.find("mid_violator").unwrap();
        let low_idx = detail.find("low_violator").unwrap();
        assert!(high_idx < mid_idx);
        assert!(mid_idx < low_idx);
    }

    // ── Threshold pass-through ───────────────────────────────────────

    #[test]
    fn threshold_value_passed_through() {
        let current = make_result(vec![]);
        let row = project_crap_delta_row(&current, None, None, 25);
        assert_eq!(row.threshold, 25);
    }
}
