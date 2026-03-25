//! Function matching — joins complexity data with coverage data.
//!
//! Uses line-range matching: for each function from the complexity adapter,
//! compute coverage from LCOV DA lines within the function's line range.
//! This is dramatically simpler than crap4ts's span-overlap matching because
//! we bypass function name matching entirely.

use super::types::{CoverageRatio, FunctionComplexity, FunctionCoverage, SourceSpan};
use std::collections::HashMap;

/// Line-level coverage data parsed from LCOV DA entries.
#[derive(Debug, Clone)]
pub struct LineCoverage {
    pub line: usize,
    pub hits: u64,
}

/// Match complexity entries with coverage using line-range overlap.
///
/// For each function in `complexities`, finds DA lines from `line_data`
/// that fall within the function's span, then computes coverage ratio.
pub fn match_functions(
    complexities: &[FunctionComplexity],
    line_data: &HashMap<String, Vec<LineCoverage>>,
) -> Vec<(FunctionComplexity, FunctionCoverage)> {
    let mut results = Vec::new();

    for comp in complexities {
        let file_lines = match line_data.get(&comp.identity.file_path) {
            Some(lines) => lines,
            None => {
                // No coverage data for this file — report as 0% coverage
                results.push((
                    comp.clone(),
                    zero_coverage(&comp.identity.file_path, comp.identity.span),
                ));
                continue;
            }
        };

        let coverage = compute_function_coverage(
            &comp.identity.file_path,
            comp.identity.span,
            file_lines,
        );
        results.push((comp.clone(), coverage));
    }

    results
}

fn compute_function_coverage(
    file_path: &str,
    span: SourceSpan,
    file_lines: &[LineCoverage],
) -> FunctionCoverage {
    let mut total = 0usize;
    let mut covered = 0usize;

    for line in file_lines {
        if line.line >= span.start_line && line.line <= span.end_line {
            total += 1;
            if line.hits > 0 {
                covered += 1;
            }
        }
    }

    let percent = if total > 0 {
        (covered as f64 / total as f64) * 100.0
    } else {
        100.0 // No instrumentable lines = trivially covered
    };

    FunctionCoverage {
        file_path: file_path.to_string(),
        span,
        line_coverage: CoverageRatio {
            covered,
            total,
            percent,
        },
    }
}

fn zero_coverage(file_path: &str, span: SourceSpan) -> FunctionCoverage {
    FunctionCoverage {
        file_path: file_path.to_string(),
        span,
        line_coverage: CoverageRatio {
            covered: 0,
            total: 0,
            percent: 0.0,
        },
    }
}
