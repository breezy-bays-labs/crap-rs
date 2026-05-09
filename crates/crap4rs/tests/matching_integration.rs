//! Integration test: syn complexity walker + line-range matching end-to-end.
//!
//! Uses the real `SynComplexityAdapter` to parse fixture files,
//! then matches against constructed LCOV-like coverage data.

use crap4rs::adapters::complexity::SynComplexityAdapter;
use crap4rs::domain::matching::match_functions;
use crap4rs::domain::types::ComplexityMetric;
use crap4rs::domain::types::LineCoverage;
use crap4rs::ports::ComplexityPort;
use std::collections::HashMap;

fn load_fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read fixture {path}: {e}"))
}

fn make_da_for_range(start: usize, end: usize, hit: bool) -> Vec<LineCoverage> {
    (start..=end)
        .map(|line| LineCoverage {
            line,
            hits: if hit { 1 } else { 0 },
        })
        .collect()
}

#[test]
fn simple_functions_fully_covered() {
    let source = load_fixture("simple_functions.rs");
    let adapter = SynComplexityAdapter::new();
    let fns = adapter
        .extract(&source, "src/simple.rs", ComplexityMetric::Cognitive)
        .unwrap();

    // Create full coverage for the entire file
    let total_lines = source.lines().count();
    let mut line_data = HashMap::new();
    line_data.insert(
        "src/simple.rs".to_string(),
        make_da_for_range(1, total_lines, true),
    );

    let matched = match_functions(&fns, &line_data, None);

    assert_eq!(matched.len(), fns.len());
    for (comp, cov) in &matched {
        assert_eq!(
            cov.line_coverage.percent, 100.0,
            "Function {} should be 100% covered",
            comp.identity.qualified_name
        );
    }
}

#[test]
fn impl_methods_matched_correctly() {
    let source = load_fixture("impl_methods.rs");
    let adapter = SynComplexityAdapter::new();
    let fns = adapter
        .extract(&source, "src/calc.rs", ComplexityMetric::Cognitive)
        .unwrap();

    // Verify we got impl methods with qualified names
    let names: Vec<&str> = fns
        .iter()
        .map(|f| f.identity.qualified_name.as_str())
        .collect();
    assert!(names.contains(&"Calculator::new"));
    assert!(names.contains(&"Calculator::add"));
    assert!(names.contains(&"Calculator::divide"));

    // Create partial coverage — only cover the `new` method's range
    let new_fn = fns
        .iter()
        .find(|f| f.identity.qualified_name == "Calculator::new")
        .unwrap();
    let mut line_data = HashMap::new();
    line_data.insert(
        "src/calc.rs".to_string(),
        make_da_for_range(
            new_fn.identity.span.start_line,
            new_fn.identity.span.end_line,
            true,
        ),
    );

    let matched = match_functions(&fns, &line_data, None);
    assert_eq!(matched.len(), fns.len());

    // new should have coverage, others should have 0 total (no DA lines in their range)
    for (comp, cov) in &matched {
        if comp.identity.qualified_name == "Calculator::new" {
            assert!(cov.line_coverage.total > 0);
            assert_eq!(cov.line_coverage.percent, 100.0);
        } else {
            // Other functions have no DA data in their range → 100% trivially
            assert_eq!(cov.line_coverage.total, 0);
        }
    }
}

#[test]
fn function_with_no_coverage_file() {
    let source = load_fixture("simple_functions.rs");
    let adapter = SynComplexityAdapter::new();
    let fns = adapter
        .extract(&source, "src/missing.rs", ComplexityMetric::Cognitive)
        .unwrap();

    // Empty line data — no coverage for this file at all
    let line_data: HashMap<String, Vec<LineCoverage>> = HashMap::new();

    let matched = match_functions(&fns, &line_data, None);
    assert_eq!(matched.len(), fns.len());

    for (_, cov) in &matched {
        assert_eq!(cov.line_coverage.percent, 0.0);
        assert_eq!(cov.line_coverage.covered, 0);
        assert_eq!(cov.line_coverage.total, 0);
    }
}

#[test]
fn mixed_coverage_per_function() {
    let source = load_fixture("simple_functions.rs");
    let adapter = SynComplexityAdapter::new();
    let fns = adapter
        .extract(&source, "src/lib.rs", ComplexityMetric::Cyclomatic)
        .unwrap();

    // Create DA data: cover only even lines
    let total_lines = source.lines().count();
    let mut da_lines: Vec<LineCoverage> = Vec::new();
    for line in 1..=total_lines {
        da_lines.push(LineCoverage {
            line,
            hits: if line % 2 == 0 { 1 } else { 0 },
        });
    }

    let mut line_data = HashMap::new();
    line_data.insert("src/lib.rs".to_string(), da_lines);

    let matched = match_functions(&fns, &line_data, None);

    // Every function should have some coverage data
    for (comp, cov) in &matched {
        assert!(
            cov.line_coverage.total > 0,
            "Function {} should have DA lines in its span",
            comp.identity.qualified_name
        );
        // With alternating coverage, no function should be 100% or 0%
        // (unless the function is a single line)
        if cov.line_coverage.total > 1 {
            assert!(
                cov.line_coverage.percent > 0.0 && cov.line_coverage.percent < 100.0,
                "Function {} should have partial coverage, got {}%",
                comp.identity.qualified_name,
                cov.line_coverage.percent
            );
        }
    }
}
