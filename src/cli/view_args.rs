//! CLI → ViewSpec adapter.
//!
//! V1a stub: every CLI invocation maps to `ViewSpec::default()`. Wave 1
//! (V1b: `--only-failing` relocation) and Wave 2 (`--top`,
//! `--min/max-coverage`, `--sort-by`) flesh this out without further
//! changes to `cli/mod.rs::run_inner`.

use super::Cli;
use crap4rs::domain::view::ViewSpec;

/// Build a `ViewSpec` from the parsed CLI. V1a returns the default
/// spec unconditionally — `apply()` is a no-op shape-preserving pass
/// over the analysis. Subsequent waves pull `top`, `coverage_range`,
/// `sort_by`, and `only_failing` off the CLI struct here.
pub(super) fn build_view_spec(_cli: &Cli) -> ViewSpec {
    ViewSpec::default()
}

/// Validation hook for view-specific args. V1a: vacuously OK. W2's
/// `--min-coverage` / `--max-coverage` will translate
/// `CoverageRangeError` to user-facing prose here.
#[allow(dead_code)] // V1b/V2 wires this in; reserved scaffold for now.
pub(super) fn validate_view_args(_cli: &Cli) -> anyhow::Result<()> {
    Ok(())
}
