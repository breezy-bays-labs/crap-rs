use crate::domain::matching::LineCoverage;
use crate::domain::types::{ComplexityMetric, CrapError, FunctionComplexity, ParseDiagnostic};
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
    pub diagnostics: Vec<ParseDiagnostic>,
}

/// Port for parsing coverage data into per-file, per-line hit counts.
pub trait CoveragePort {
    fn parse(&self, data: &str) -> Result<ParseOutput, CrapError>;
}
