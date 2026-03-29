use crate::domain::types::{
    BranchCoverage, ComplexityMetric, CrapError, FunctionComplexity, LineCoverage, ParseDiagnostic,
};
use std::collections::HashMap;

/// Port for extracting per-function complexity from source code.
pub trait ComplexityPort {
    fn extract(
        &self,
        source: &str,
        file_path: &str,
        metric: ComplexityMetric,
    ) -> Result<Vec<FunctionComplexity>, CrapError>;
}

/// Result of parsing coverage data: coverage map + non-fatal diagnostics.
#[derive(Debug)]
pub struct ParseOutput {
    pub coverage: HashMap<String, Vec<LineCoverage>>,
    /// Branch coverage data from BRDA records, keyed by file path.
    /// `None` when no BRDA records were encountered in the entire input.
    pub branches: Option<HashMap<String, Vec<BranchCoverage>>>,
    pub diagnostics: Vec<ParseDiagnostic>,
}

/// Port for parsing coverage data into per-file, per-line hit counts.
pub trait CoveragePort {
    fn parse(&self, data: &str) -> Result<ParseOutput, CrapError>;
}
