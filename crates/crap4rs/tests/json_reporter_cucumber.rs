//! Cucumber-rs runner for `tests/features/json_reporter.feature` (#115).
//!
//! Establishes the BDD pattern crap4rs adopts for executing Gherkin
//! specs as integration tests. Each migrated `.feature` file gets its
//! own `[[test]]` target with `harness = false` (cucumber prints its
//! own output, not libtest's).
//!
//! These scenarios mirror the unit tests in
//! `src/adapters/reporters/json.rs::tests` intentionally — they're the
//! Gherkin-spec form of the same invariants. Both layers stay until
//! the cucumber migration policy decides otherwise (#115 acceptance
//! criterion: "do all `.feature` files migrate, or only new ones?").
//! The unit-test layer pins invariants tightly to the reporter's
//! internals; the BDD layer makes the same scenarios executable
//! directly from the spec file, so the spec stops drifting from the
//! implementation.
//!
//! `AnalysisResult` is `#[non_exhaustive]` per the v0.3.0 hardening, so
//! external struct-literal construction is intentionally restricted.
//! The cucumber world deserializes results from JSON instead — that
//! keeps the test target inside the public envelope contract and
//! requires no `pub(crate)` test-fixture exposure. Future feature
//! ports can follow the same pattern.

use cucumber::{World, given, then, when, writer};
use serde_json::{Value, json};

use crap4rs::adapters::reporters::{JsonConfig, format_json};
use crap4rs::domain::types::{AnalysisResult, ComplexityMetric};
use crap4rs::domain::view::{self, ViewSpec};

/// State threaded through Given/When/Then steps. Each scenario starts
/// with a fresh world; we accumulate the analysis fixture, the metric
/// and threshold the envelope should report, and finally the parsed
/// JSON output for assertions.
#[derive(Debug, Default, World)]
struct JsonWorld {
    result: Option<AnalysisResult>,
    metric: Option<ComplexityMetric>,
    threshold: Option<f64>,
    output: Option<Value>,
}

impl JsonWorld {
    fn set_result_from_json(&mut self, value: Value) {
        let result: AnalysisResult =
            serde_json::from_value(value).expect("fixture JSON should deserialize");
        self.result = Some(result);
    }

    fn render(&mut self) {
        let result = self.result.as_ref().expect("result must be set first");
        let metric = self.metric.unwrap_or(ComplexityMetric::Cognitive);
        let threshold = self.threshold.unwrap_or(8.0);
        let view = view::apply(result, ViewSpec::default());
        let config = JsonConfig {
            tool_version: "0.1.0".to_string(),
            metric,
            threshold,
            timestamp: "2026-05-04T00:00:00Z".to_string(),
            diagnostics: None,
            diff_ref: None,
            minimal_view: false,
            delta: None,
        };
        let json_str = format_json(&view, &config).expect("format_json should succeed");
        self.output = Some(serde_json::from_str(&json_str).expect("output should be valid JSON"));
    }

    fn json(&self) -> &Value {
        self.output.as_ref().expect("JSON not yet rendered")
    }
}

/// Resolve a dotted JSON path against the rendered output. Path is
/// `result.summary.distribution.low`-style — used by the Then steps to
/// match the feature file's assertion DSL.
fn lookup<'a>(value: &'a Value, path: &str) -> &'a Value {
    let mut cursor = value;
    for segment in path.split('.') {
        cursor = cursor
            .get(segment)
            .unwrap_or_else(|| panic!("path {path:?} missing at segment {segment:?} in {value}"));
    }
    cursor
}

// ── Fixtures ─────────────────────────────────────────────────────────

/// Minimal empty `AnalysisResult` JSON — round-trippable through serde.
fn empty_result_json() -> Value {
    json!({
        "functions": [],
        "summary": {
            "total_functions": 0,
            "total_files": 0,
            "exceeding_threshold": 0,
            "average_crap": 0.0,
            "median_crap": 0.0,
            "max_crap": null,
            "worst_function": null,
            "distribution": { "low": 0, "acceptable": 0, "moderate": 0, "high": 0 }
        },
        "passed": true
    })
}

/// Single-function fixture matching the json_reporter.feature scenario
/// "Function entries contain all scored fields".
fn single_function_json(
    name: &str,
    file: &str,
    complexity: u32,
    coverage_percent: f64,
    crap_value: f64,
    risk_level: &str,
    threshold: f64,
) -> Value {
    let exceeds = crap_value > threshold;
    json!({
        "functions": [{
            "scored": {
                "identity": {
                    "file_path": file,
                    "qualified_name": name,
                    "span": { "start_line": 1, "end_line": 10, "start_column": 0, "end_column": 0 }
                },
                "complexity": complexity,
                "complexity_metric": "cognitive",
                "coverage_percent": coverage_percent,
                "crap": { "value": crap_value, "risk_level": risk_level },
                "contributors": []
            },
            "threshold": threshold,
            "exceeds": exceeds
        }],
        "summary": {
            "total_functions": 1,
            "total_files": 1,
            "exceeding_threshold": if exceeds { 1 } else { 0 },
            "average_crap": crap_value,
            "median_crap": crap_value,
            "max_crap": { "value": crap_value, "risk_level": risk_level },
            "worst_function": {
                "file_path": file,
                "qualified_name": name,
                "span": { "start_line": 1, "end_line": 10, "start_column": 0, "end_column": 0 }
            },
            "distribution": {
                "low": if risk_level == "low" { 1 } else { 0 },
                "acceptable": if risk_level == "acceptable" { 1 } else { 0 },
                "moderate": if risk_level == "moderate" { 1 } else { 0 },
                "high": if risk_level == "high" { 1 } else { 0 }
            }
        },
        "passed": !exceeds
    })
}

/// `n` low-risk functions with `exceeding` of them tagged exceeds=true.
/// Used by the aggregate-statistics scenarios.
fn many_functions_json(n: usize, exceeding: usize) -> Value {
    let crap = 1.0_f64;
    let functions: Vec<Value> = (0..n)
        .map(|i| {
            json!({
                "scored": {
                    "identity": {
                        "file_path": format!("src/lib.rs"),
                        "qualified_name": format!("fn_{i}"),
                        "span": { "start_line": 1, "end_line": 10, "start_column": 0, "end_column": 0 }
                    },
                    "complexity": 1,
                    "complexity_metric": "cognitive",
                    "coverage_percent": 100.0,
                    "crap": { "value": crap, "risk_level": "low" },
                    "contributors": []
                },
                "threshold": 8.0,
                "exceeds": i < exceeding
            })
        })
        .collect();
    json!({
        "functions": functions,
        "summary": {
            "total_functions": n,
            "total_files": 1,
            "exceeding_threshold": exceeding,
            "average_crap": crap,
            "median_crap": crap,
            "max_crap": { "value": crap, "risk_level": "low" },
            "worst_function": null,
            "distribution": { "low": n, "acceptable": 0, "moderate": 0, "high": 0 }
        },
        "passed": exceeding == 0
    })
}

/// Distribution-tailored fixture: `low + acceptable + moderate + high`
/// functions in a result whose `summary.distribution` matches the bag.
fn distribution_json(low: usize, acceptable: usize, moderate: usize, high: usize) -> Value {
    let total = low + acceptable + moderate + high;
    let mut functions = Vec::with_capacity(total);
    let mut push = |risk: &str, count: usize, threshold_exceeded: bool| {
        for i in 0..count {
            functions.push(json!({
                "scored": {
                    "identity": {
                        "file_path": "src/lib.rs",
                        "qualified_name": format!("{risk}_{i}"),
                        "span": { "start_line": 1, "end_line": 10, "start_column": 0, "end_column": 0 }
                    },
                    "complexity": 1,
                    "complexity_metric": "cognitive",
                    "coverage_percent": 100.0,
                    "crap": { "value": 1.0, "risk_level": risk },
                    "contributors": []
                },
                "threshold": 8.0,
                "exceeds": threshold_exceeded
            }));
        }
    };
    push("low", low, false);
    push("acceptable", acceptable, false);
    push("moderate", moderate, true);
    push("high", high, true);
    let exceeding = moderate + high;
    json!({
        "functions": functions,
        "summary": {
            "total_functions": total,
            "total_files": 1,
            "exceeding_threshold": exceeding,
            "average_crap": 1.0,
            "median_crap": 1.0,
            "max_crap": { "value": 1.0, "risk_level": "low" },
            "worst_function": null,
            "distribution": { "low": low, "acceptable": acceptable, "moderate": moderate, "high": high }
        },
        "passed": exceeding == 0
    })
}

// ── Given steps ─────────────────────────────────────────────────────

#[given("an analysis result")]
fn given_analysis_result(world: &mut JsonWorld) {
    world.set_result_from_json(empty_result_json());
}

#[given(
    regex = r#"^an analysis with one function "([^"]+)" in "([^"]+)" with complexity (\d+), coverage ([\d.]+)%, and CRAP score ([\d.]+)$"#
)]
fn given_one_function(
    world: &mut JsonWorld,
    name: String,
    file: String,
    complexity: u32,
    coverage: f64,
    crap: f64,
) {
    let risk = if crap <= 8.0 {
        "low"
    } else if crap <= 15.0 {
        "acceptable"
    } else if crap <= 25.0 {
        "moderate"
    } else {
        "high"
    };
    world.set_result_from_json(single_function_json(
        &name, &file, complexity, coverage, crap, risk, 8.0,
    ));
}

#[given(regex = r"^an analysis with (\d+) functions, (\d+) exceeding threshold$")]
fn given_n_functions(world: &mut JsonWorld, n: usize, exceeding: usize) {
    world.set_result_from_json(many_functions_json(n, exceeding));
}

#[given(
    regex = r"^an analysis with distribution low=(\d+) acceptable=(\d+) moderate=(\d+) high=(\d+)$"
)]
fn given_distribution(
    world: &mut JsonWorld,
    low: usize,
    acceptable: usize,
    moderate: usize,
    high: usize,
) {
    world.set_result_from_json(distribution_json(low, acceptable, moderate, high));
}

#[given("an analysis where all functions are within threshold")]
fn given_all_within(world: &mut JsonWorld) {
    world.set_result_from_json(many_functions_json(3, 0));
}

#[given(regex = r"^an analysis where (\d+) function exceeds the threshold$")]
fn given_n_exceed(world: &mut JsonWorld, n: usize) {
    world.set_result_from_json(many_functions_json(3, n));
}

#[given("an analysis with no functions")]
fn given_no_functions(world: &mut JsonWorld) {
    world.set_result_from_json(empty_result_json());
}

#[given(regex = r"^the analysis used (cognitive|cyclomatic) complexity$")]
fn given_metric(world: &mut JsonWorld, metric: String) {
    world.metric = Some(match metric.as_str() {
        "cognitive" => ComplexityMetric::Cognitive,
        "cyclomatic" => ComplexityMetric::Cyclomatic,
        other => panic!("unknown metric {other:?}"),
    });
    if world.result.is_none() {
        world.set_result_from_json(empty_result_json());
    }
}

#[given(regex = r"^the analysis used threshold ([\d.]+)$")]
fn given_threshold(world: &mut JsonWorld, threshold: f64) {
    world.threshold = Some(threshold);
    if world.result.is_none() {
        world.set_result_from_json(empty_result_json());
    }
}

// ── When steps ──────────────────────────────────────────────────────

#[when("the JSON is formatted")]
fn when_formatted(world: &mut JsonWorld) {
    world.render();
}

#[when(regex = r#"^the JSON is formatted with metric "([^"]+)"$"#)]
fn when_formatted_with_metric(world: &mut JsonWorld, metric: String) {
    world.metric = Some(match metric.as_str() {
        "cognitive" => ComplexityMetric::Cognitive,
        "cyclomatic" => ComplexityMetric::Cyclomatic,
        other => panic!("unknown metric {other:?}"),
    });
    world.render();
}

#[when(regex = r"^the JSON is formatted with threshold ([\d.]+)$")]
fn when_formatted_with_threshold(world: &mut JsonWorld, threshold: f64) {
    world.threshold = Some(threshold);
    world.render();
}

// ── Then steps ──────────────────────────────────────────────────────

#[then(regex = r#"^the output contains "([^"]+)" with value (\d+)$"#)]
fn then_contains_with_int(world: &mut JsonWorld, key: String, expected: i64) {
    let actual = lookup(world.json(), &key).as_i64().unwrap();
    assert_eq!(actual, expected, "json[{key:?}]");
}

#[then(regex = r#"^the output contains "([^"]+)" with value "([^"]+)"$"#)]
fn then_contains_with_str(world: &mut JsonWorld, key: String, expected: String) {
    let actual = lookup(world.json(), &key).as_str().unwrap();
    assert_eq!(actual, expected, "json[{key:?}]");
}

#[then(regex = r#"^the output contains "([^"]+)"$"#)]
fn then_contains(world: &mut JsonWorld, key: String) {
    assert!(
        world.json().get(&key).is_some(),
        "expected key {key:?} in {}",
        world.json()
    );
}

#[then(regex = r#"^the output contains "([^"]+)" as an ISO 8601 string$"#)]
fn then_contains_iso(world: &mut JsonWorld, key: String) {
    let s = lookup(world.json(), &key).as_str().expect("string");
    assert!(s.contains('T') && s.ends_with('Z'), "ISO 8601: {s:?}");
}

#[then(regex = r#"^the "([^"]+)" object contains "([^"]+)"$"#)]
fn then_object_contains(world: &mut JsonWorld, parent: String, key: String) {
    let obj = lookup(world.json(), &parent).as_object().unwrap();
    assert!(obj.contains_key(&key), "{parent}.{key} missing");
}

#[then(regex = r#"^"([^"]+)" is the integer (\d+)$"#)]
fn then_is_integer(world: &mut JsonWorld, path: String, expected: i64) {
    let v = lookup(world.json(), &path);
    assert!(v.is_number(), "{path} should be number, got {v:?}");
    assert_eq!(v.as_i64().unwrap(), expected);
}

#[then("the functions array has one entry")]
fn then_one_entry(world: &mut JsonWorld) {
    let funcs = lookup(world.json(), "result.functions").as_array().unwrap();
    assert_eq!(funcs.len(), 1);
}

#[then(regex = r#"^the entry contains "([^"]+)" with "([^"]+)" equal to "([^"]+)"$"#)]
fn then_entry_nested_str(world: &mut JsonWorld, outer: String, inner: String, expected: String) {
    let func = &lookup(world.json(), "result.functions")[0];
    let actual = func["scored"][&outer][&inner].as_str().unwrap();
    assert_eq!(actual, expected, "functions[0].scored.{outer}.{inner}");
}

#[then(regex = r#"^the entry contains "([^"]+)" with "([^"]+)" equal to ([\d.]+)$"#)]
fn then_entry_nested_num(world: &mut JsonWorld, outer: String, inner: String, expected: f64) {
    let func = &lookup(world.json(), "result.functions")[0];
    let actual = func["scored"][&outer][&inner].as_f64().unwrap();
    assert!(
        (actual - expected).abs() < 1e-6,
        "functions[0].scored.{outer}.{inner}: {actual} != {expected}"
    );
}

#[then(regex = r#"^the entry contains "([^"]+)" equal to (true|false)$"#)]
fn then_entry_bool(world: &mut JsonWorld, key: String, expected: String) {
    let func = &lookup(world.json(), "result.functions")[0];
    let actual = func[&key].as_bool().unwrap();
    assert_eq!(actual, expected == "true", "functions[0].{key}");
}

#[then(regex = r#"^the entry contains "([^"]+)" equal to (\d+\.\d+)$"#)]
fn then_entry_float(world: &mut JsonWorld, key: String, expected: f64) {
    let func = &lookup(world.json(), "result.functions")[0];
    let actual = func["scored"][&key]
        .as_f64()
        .or_else(|| func[&key].as_f64())
        .unwrap_or_else(|| panic!("functions[0].(scored.)?{key} not float"));
    assert!(
        (actual - expected).abs() < 1e-6,
        "functions[0].{key}: {actual} != {expected}"
    );
}

#[then(regex = r#"^the entry contains "([^"]+)" equal to (\d+)$"#)]
fn then_entry_int(world: &mut JsonWorld, key: String, expected: i64) {
    let func = &lookup(world.json(), "result.functions")[0];
    let actual = func["scored"][&key]
        .as_i64()
        .or_else(|| func[&key].as_i64())
        .unwrap_or_else(|| panic!("functions[0].(scored.)?{key} not int"));
    assert_eq!(actual, expected);
}

#[then(regex = r#"^"([^"]+)" equals (\d+\.\d+)$"#)]
fn then_path_equals_float(world: &mut JsonWorld, path: String, expected: f64) {
    let actual = lookup(world.json(), &path).as_f64().unwrap();
    assert!(
        (actual - expected).abs() < 1e-6,
        "{path}: {actual} != {expected}"
    );
}

#[then(regex = r#"^"([^"]+)" equals (\d+)$"#)]
fn then_path_equals_int(world: &mut JsonWorld, path: String, expected: i64) {
    assert_eq!(lookup(world.json(), &path).as_i64().unwrap(), expected);
}

#[then(regex = r#"^"([^"]+)" equals "([^"]+)"$"#)]
fn then_path_equals_str(world: &mut JsonWorld, path: String, expected: String) {
    assert_eq!(lookup(world.json(), &path).as_str().unwrap(), expected);
}

#[then(regex = r#"^"([^"]+)" is a number$"#)]
fn then_path_is_number(world: &mut JsonWorld, path: String) {
    assert!(lookup(world.json(), &path).is_number(), "{path}");
}

#[then(regex = r#"^"([^"]+)" is true$"#)]
fn then_path_is_true(world: &mut JsonWorld, path: String) {
    assert_eq!(lookup(world.json(), &path).as_bool(), Some(true), "{path}");
}

#[then(regex = r#"^"([^"]+)" is false$"#)]
fn then_path_is_false(world: &mut JsonWorld, path: String) {
    assert_eq!(lookup(world.json(), &path).as_bool(), Some(false), "{path}");
}

#[then(regex = r#"^"([^"]+)" is an empty array$"#)]
fn then_path_is_empty_array(world: &mut JsonWorld, path: String) {
    let arr = lookup(world.json(), &path).as_array().unwrap();
    assert!(arr.is_empty(), "{path} expected empty, got {arr:?}");
}

#[then("the output is valid JSON")]
fn then_valid_json(world: &mut JsonWorld) {
    let _ = world.json();
}

#[then(regex = r#"^"([^"]+)" is a valid ISO 8601 datetime$"#)]
fn then_path_iso8601(world: &mut JsonWorld, path: String) {
    let s = lookup(world.json(), &path).as_str().unwrap();
    assert!(
        s.contains('T') && (s.ends_with('Z') || s.contains('+') || s.contains('-')),
        "ISO 8601: {s:?}"
    );
}

// ── Runner ─────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // `writer::Libtest::or_basic()` makes cucumber emit `libtest`-compatible
    // JSON when invoked via `cargo nextest run` (which probes `--list`),
    // and falls back to the human-readable basic writer for plain
    // `cargo test` runs. Required for nextest discovery (#115).
    //
    // `run_and_exit` (not `run`) panics on scenario failure, which gives
    // a non-zero process exit under `cargo test`. Plain `run` returns a
    // Writer and exits 0 even when scenarios fail — silently turning a
    // red CI step green.
    JsonWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .run_and_exit("tests/features/json_reporter.feature")
        .await;
}
