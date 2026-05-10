use crate::domain::types::{
    BranchCoverage, ComplexityMetric, CrapError, FileChangeKind, FunctionComplexity, LineCoverage,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fmt::Debug;
use std::path::Path;

/// Port for extracting per-function complexity from source code.
pub trait ComplexityPort {
    fn extract(
        &self,
        source: &str,
        file_path: &str,
        metric: ComplexityMetric,
    ) -> Result<Vec<FunctionComplexity>, CrapError>;
}

/// Trait implemented by adapter-specific parse diagnostic types.
///
/// `LcovParseDiagnostic` (in `crap4rs`) is the v0.5 LCOV implementation;
/// future siblings (`crap4ts`'s Istanbul adapter) will define their own
/// concrete type implementing this trait. Bounds are minimal but
/// load-bearing: `Serialize + DeserializeOwned` lets `AnalysisDiagnostics<P>`
/// and `ParseOutput<P>` keep their auto-derived serde shapes; `Debug + Clone`
/// match the rest of the domain's pervasive derive set. The trait itself
/// is not object-safe (`Clone` requires `Sized`); object safety is enforced
/// one layer up on `&dyn CoveragePort<Diagnostic = …>` where the caller
/// fixes the associated type. See ADR D9 (mixed-dispatch-strategy).
pub trait ParseDiagnostic: Debug + Clone + Serialize + DeserializeOwned {}

/// Result of parsing coverage data: coverage map + non-fatal diagnostics.
///
/// Generic over `P: ParseDiagnostic` so the LCOV adapter (`crap4rs`),
/// the future Istanbul adapter (`crap4ts`), and any sibling adapter can
/// thread their own diagnostic shape through one shared analyzer
/// pipeline. Per ADR D9, `Vec<P>` storage of potentially thousands of
/// parse diagnostics avoids the per-element `Box<dyn ParseDiagnostic>`
/// heap-allocation cost.
#[derive(Debug)]
pub struct ParseOutput<P: ParseDiagnostic> {
    pub coverage: HashMap<String, Vec<LineCoverage>>,
    /// Branch coverage data from BRDA records, keyed by file path.
    /// `None` when no BRDA records were encountered in the entire input.
    pub branches: Option<HashMap<String, Vec<BranchCoverage>>>,
    pub diagnostics: Vec<P>,
}

/// Port for parsing coverage data into per-file, per-line hit counts.
///
/// `Diagnostic` associated type lets concrete impls fix their parse
/// diagnostic shape (`LcovParseDiagnostic` for the Rust adapter). The
/// trait is dyn-compatible when callers fix the associated type at the
/// dyn site: `&dyn CoveragePort<Diagnostic = LcovParseDiagnostic>`.
/// See ADR D9 for the mixed-dispatch rationale.
pub trait CoveragePort {
    type Diagnostic: ParseDiagnostic;
    fn parse(&self, data: &str) -> Result<ParseOutput<Self::Diagnostic>, CrapError>;
}

/// Port for computing which files/regions changed relative to a git ref.
pub trait DiffPort {
    fn changed_regions(
        &self,
        diff_ref: &str,
        working_dir: &Path,
        paths: &[String],
    ) -> Result<HashMap<String, FileChangeKind>, CrapError>;
}

#[cfg(test)]
mod object_safety {
    //! Compile-fence: locks `&dyn CoveragePort<Diagnostic = ...>` and
    //! `&dyn ComplexityPort` as object-safe, per ADR D9. If a future
    //! port method becomes generic, returns `impl Trait`, or takes
    //! `Self`, this test fails to compile and the dispatch contract
    //! is the loud failure point.
    use super::*;
    use crate::test_strategies::DummyParseDiagnostic;

    #[allow(dead_code)]
    fn _coverage_port_is_dyn_safe(_port: &dyn CoveragePort<Diagnostic = DummyParseDiagnostic>) {}

    #[allow(dead_code)]
    fn _complexity_port_is_dyn_safe(_port: &dyn ComplexityPort) {}
}
