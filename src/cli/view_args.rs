//! CLI → ViewSpec adapter.
//!
//! V1b wires `--only-failing` through `Filters::only_failing`. W2 layers
//! `--min/--max-coverage` through `Filters::coverage_range` (this module);
//! `--top` and `--sort-by` follow without further changes to
//! `cli/mod.rs::run_inner`.

use anyhow::{Result, bail};

use super::Cli;
use crap4rs::domain::view::{CoverageRange, CoverageRangeError, ViewSpec};
// `GroupKey` flows from CLI to `ViewSpec` via `cli::GroupByArg::Into<GroupKey>`.

/// Build a `ViewSpec` from the parsed CLI.
///
/// `Filters` and `ViewSpec` are `#[non_exhaustive]` (per ADR D3 — they
/// reserve namespace for future filters/sort keys), so we mutate fields
/// on a default rather than using a struct literal.
///
/// Assumes `validate_view_args` already ran and accepted the input —
/// the `CoverageRange::new` call below cannot fail in `run_inner`.
pub(super) fn build_view_spec(cli: &Cli) -> ViewSpec {
    let mut spec = ViewSpec::default();
    spec.filters.only_failing = cli.filter.only_failing;
    if let Some((lo, hi)) = resolve_coverage_bounds(cli) {
        spec.filters.coverage_range = CoverageRange::new(lo, hi).ok();
    }
    // `Some(0)` and `None` are both "no limit" (per `domain::view::truncate_to`);
    // canonicalise at the boundary so JSON consumers see effective behaviour
    // rather than the user's literal input.
    spec.limit = cli.filter.top.and_then(|n| (n > 0).then_some(n as usize));
    spec.sort = cli.filter.sort_by.map(Into::into).unwrap_or_default();
    spec.group_by = cli.filter.group_by.map(Into::into);
    spec
}

/// Validate view-specific args and translate domain errors to flag-attributed
/// CLI prose.
///
/// `CoverageRange::new` (in `domain::view`) is the single source of truth for
/// validity rules. We pre-check each bound for OutOfRange so the message can
/// name the offending flag, then rely on `CoverageRange::new` for the
/// relational `MinExceedsMax` check.
pub(super) fn validate_view_args(cli: &Cli) -> Result<()> {
    let Some((lo, hi)) = resolve_coverage_bounds(cli) else {
        return Ok(());
    };

    if cli
        .filter
        .min_coverage
        .is_some_and(|v| !(0.0..=100.0).contains(&v))
    {
        bail!("--min-coverage must be in [0, 100]");
    }
    if cli
        .filter
        .max_coverage
        .is_some_and(|v| !(0.0..=100.0).contains(&v))
    {
        bail!("--max-coverage must be in [0, 100]");
    }

    match CoverageRange::new(lo, hi) {
        Ok(_) => Ok(()),
        Err(CoverageRangeError::MinExceedsMax { .. }) => {
            bail!("--min-coverage must not exceed --max-coverage")
        }
        // OutOfRange is unreachable: per-bound checks above handle it. The
        // wildcard catches future `#[non_exhaustive]` variants too — domain
        // errors must be translated to flag-attributed prose at the CLI edge.
        Err(e) => bail!("invalid --min-coverage / --max-coverage: {e}"),
    }
}

/// Returns the resolved `(min, max)` pair iff at least one bound was passed.
/// Unspecified sides default to 0.0 / 100.0 — the natural domain of the
/// percentage scale (cli_ergonomics.feature:79-86).
fn resolve_coverage_bounds(cli: &Cli) -> Option<(f64, f64)> {
    match (cli.filter.min_coverage, cli.filter.max_coverage) {
        (None, None) => None,
        (lo, hi) => Some((lo.unwrap_or(0.0), hi.unwrap_or(100.0))),
    }
}
