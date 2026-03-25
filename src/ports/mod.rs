use crate::domain::matching::LineCoverage;
use crate::domain::types::{ComplexityMetric, CrapError, FunctionComplexity};
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

/// Port for parsing coverage data into per-file, per-line hit counts.
pub trait CoveragePort {
    fn parse(
        &self,
        data: &str,
    ) -> Result<HashMap<String, Vec<LineCoverage>>, CrapError>;
}
