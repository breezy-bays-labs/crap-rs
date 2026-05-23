//! Integration tests for the feature-independent
//! `crap4ts::analyze_to_json` library API — the orchestration entry
//! point both the binary path (via `main.rs::cli::run`) and the napi
//! cdylib path (via `src/napi.rs`) ultimately funnel through.
//!
//! Mirrors `end_to_end_smoke.rs`'s tempdir + jest-fixture template
//! pattern so the test exercises the same realistic source set the
//! binary canary uses. Exists primarily so the self-CRAP gate scores
//! `analyze_to_json` with non-zero coverage — without this test the
//! function shows up as 0%-covered (CRAP = c² + c) in the
//! `crates/crap4ts/src` self-check leg and trips the strict gate.

use std::path::PathBuf;

use crap_core::domain::types::ComplexityMetric;
use crap4ts::analyze_to_json;
use tempfile::TempDir;

const FIXTURE_TEMPLATE: &str = include_str!("fixtures/istanbul-jest/coverage-final.json");

/// Build a canonicalised tempdir with the W1.1 jest fixtures + a
/// substituted `coverage-final.json`. Mirrors
/// `end_to_end_smoke.rs::build_jest_fixture` (kept separate by design —
/// each integration test owns its fixture wiring so editing one
/// doesn't drag drift through the other).
fn build_jest_fixture() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");

    for (name, content) in [
        ("simple.ts", include_str!("fixtures/ts-fixtures/simple.ts")),
        ("arrow.ts", include_str!("fixtures/ts-fixtures/arrow.ts")),
        (
            "Button.tsx",
            include_str!("fixtures/ts-fixtures/Button.tsx"),
        ),
        ("map.ts", include_str!("fixtures/ts-fixtures/map.ts")),
        ("mixed.ts", include_str!("fixtures/ts-fixtures/mixed.ts")),
    ] {
        std::fs::write(canonical.join(name), content).expect("write fixture");
    }

    let payload = FIXTURE_TEMPLATE.replace(
        "{SRC_ROOT}",
        &canonical.to_string_lossy().replace('\\', "/"),
    );
    std::fs::write(canonical.join("coverage-final.json"), payload)
        .expect("write coverage-final.json");

    (tmp, canonical)
}

#[test]
fn analyze_to_json_happy_path_returns_valid_json_with_expected_shape() {
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");

    let json = analyze_to_json(&root, &coverage, None, ComplexityMetric::Cyclomatic)
        .expect("analyze_to_json succeeds on jest fixture");

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("returned string parses as JSON");

    // Top-level shape mirrors `crap_core::core::AnalysisOutput<P>` —
    // `{ result, diagnostics }`. Both keys must be present so Node
    // consumers can read the same fields a Rust embedder would.
    assert!(
        parsed.get("result").is_some(),
        "missing `result` key in JSON output:\n{json}"
    );
    assert!(
        parsed.get("diagnostics").is_some(),
        "missing `diagnostics` key in JSON output:\n{json}"
    );

    // At least one function gets analyzed (the jest fixture covers
    // five TS files with multiple functions). functions_extracted is
    // an unsigned counter inside diagnostics.
    let functions_extracted = parsed["diagnostics"]["functions_extracted"]
        .as_u64()
        .expect("functions_extracted is a positive integer");
    assert!(
        functions_extracted > 0,
        "expected functions_extracted > 0, got {functions_extracted}"
    );
}

#[test]
fn analyze_to_json_explicit_threshold_overrides_default() {
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");

    // Sky-high threshold guarantees `passed: true`; the default
    // metric-correct preset (cyclomatic: 16) would too on this
    // fixture, but pinning an explicit override exercises the
    // threshold-propagation path.
    let json = analyze_to_json(&root, &coverage, Some(1000.0), ComplexityMetric::Cyclomatic)
        .expect("analyze_to_json succeeds with explicit threshold");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let passed = parsed["result"]["passed"]
        .as_bool()
        .expect("`result.passed` is a bool");
    assert!(
        passed,
        "expected passed=true with threshold 1000, got result:\n{}",
        serde_json::to_string_pretty(&parsed["result"]["summary"]).unwrap_or_default()
    );
}

#[test]
fn analyze_to_json_cognitive_metric_surfaces_not_supported_error() {
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");

    // crap4ts only implements cyclomatic in 2.0; the walker returns
    // `CrapError::MetricNotSupported { metric: Cognitive }` which the
    // library API surfaces as a `String` containing the metric name.
    let err = analyze_to_json(&root, &coverage, None, ComplexityMetric::Cognitive)
        .expect_err("cognitive metric should surface MetricNotSupported");

    assert!(
        err.to_lowercase().contains("cognitive"),
        "expected error message to mention `cognitive`, got: {err}"
    );
}

#[test]
fn analyze_to_json_negative_threshold_is_rejected() {
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");

    // A negative threshold is nonsensical for a CRAP score (always
    // >= 1.0) — validate up front rather than silently flagging every
    // function.
    let err = analyze_to_json(&root, &coverage, Some(-1.0), ComplexityMetric::Cyclomatic)
        .expect_err("negative threshold should be rejected");

    assert!(
        err.contains("threshold"),
        "expected error message to mention `threshold`, got: {err}"
    );
}

#[test]
fn analyze_to_json_nan_threshold_is_rejected() {
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");

    // `NaN` can reach this entry point from a JS caller; comparisons
    // against it are always false, so without an explicit check it
    // would yield non-deterministic pass/fail classification.
    let err = analyze_to_json(
        &root,
        &coverage,
        Some(f64::NAN),
        ComplexityMetric::Cyclomatic,
    )
    .expect_err("NaN threshold should be rejected");

    assert!(
        err.contains("threshold"),
        "expected error message to mention `threshold`, got: {err}"
    );
}

/// `.d.ts` declaration files are skipped on the library / napi entry
/// point the same way they're skipped on the CLI path (crap-rs#253).
/// `analyze_to_json` populates `AnalyzeOptions.exclude` with
/// `FORCED_EXCLUDES` (chained into `DEFAULT_EXCLUDES`), so a tempdir
/// containing both a `.ts` file and a `.d.ts` file must produce only
/// the `.ts` file's functions. Without this, programmatic (Node)
/// callers would see ambient-type entries the CLI doesn't.
#[test]
fn analyze_to_json_skips_dts_declaration_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");

    // Two sibling files: a normal `.ts` carrying an executable
    // function, and a `.d.ts` carrying an ambient declaration. Both
    // syntactically parse with oxc; the `forced_excludes` skip stops
    // discovery from feeding the `.d.ts` into the AST walker at all.
    std::fs::write(
        canonical.join("app.ts"),
        "export function app() { return 1; }\n",
    )
    .expect("write app.ts");
    std::fs::write(
        canonical.join("types.d.ts"),
        "export declare function ambient(): number;\n",
    )
    .expect("write types.d.ts");

    // Coverage covers `app.ts` only — Istanbul never instruments
    // declaration files (no statements to wrap), so omitting the
    // `.d.ts` entry from the fixture mirrors what a real jest run
    // produces.
    let abs = canonical.to_string_lossy().replace('\\', "/");
    let coverage_payload = format!(
        r#"{{ "{abs}/app.ts": {{ "path": "{abs}/app.ts", "s": {{ "0": 1 }}, "statementMap": {{ "0": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 5 }} }} }} }} }}"#
    );
    let coverage = canonical.join("coverage-final.json");
    std::fs::write(&coverage, coverage_payload).expect("write coverage-final.json");

    let json = analyze_to_json(&canonical, &coverage, None, ComplexityMetric::Cyclomatic)
        .expect("analyze_to_json succeeds on app.ts + types.d.ts");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let functions = parsed["result"]["functions"]
        .as_array()
        .expect("`result.functions` is an array");
    let file_paths: Vec<&str> = functions
        .iter()
        .filter_map(|f| f["scored"]["identity"]["file_path"].as_str())
        .collect();

    assert!(
        file_paths.contains(&"app.ts"),
        "expected `app.ts` in report; got {file_paths:?}",
    );
    assert!(
        !file_paths.iter().any(|p| p.ends_with(".d.ts")),
        "no `.d.ts` file should appear in the report (crap-rs#253); got {file_paths:?}",
    );
}

#[test]
fn analyze_to_json_missing_source_root_is_rejected() {
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");
    let missing = root.join("does-not-exist");

    // A `source_root` that does not resolve to a directory fails fast
    // instead of walking an empty tree and returning a zero-function
    // result the caller would have to diagnose themselves.
    let err = analyze_to_json(&missing, &coverage, None, ComplexityMetric::Cyclomatic)
        .expect_err("missing source_root should be rejected");

    assert!(
        err.contains("source_root"),
        "expected error message to mention `source_root`, got: {err}"
    );
}
