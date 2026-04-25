//! CLI → ViewSpec adapter.
//!
//! V1b wires `--only-failing` through `Filters::only_failing`. Wave 2
//! (`--top`, `--min/max-coverage`, `--sort-by`) extends this without
//! further changes to `cli/mod.rs::run_inner`.

use super::Cli;
use crap4rs::domain::view::ViewSpec;

/// Build a `ViewSpec` from the parsed CLI.
///
/// `Filters` and `ViewSpec` are `#[non_exhaustive]` (per ADR D3 — they
/// reserve namespace for future filters/sort keys), so we mutate fields
/// on a default rather than using a struct literal.
pub(super) fn build_view_spec(cli: &Cli) -> ViewSpec {
    let mut spec = ViewSpec::default();
    spec.filters.only_failing = cli.filter.only_failing;
    spec
}

/// Validation hook for view-specific args. V1a: vacuously OK. W2's
/// `--min-coverage` / `--max-coverage` will translate
/// `CoverageRangeError` to user-facing prose here.
#[allow(dead_code)] // V1b/V2 wires this in; reserved scaffold for now.
pub(super) fn validate_view_args(_cli: &Cli) -> anyhow::Result<()> {
    Ok(())
}
