//! Direct smoke tests for `IstanbulCoverage`.
//!
//! Exercises the parser at the unit level (no walker, no full CLI
//! dispatch). The .feature contracts at `tests/features/{istanbul_parser,
//! arrow_function_coverage}.feature` stay `@unwired` until W3.3 attaches
//! cucumber-rs harnesses; these smoke tests ground-truth the same
//! contracts directly so the W1.1 implementation lands with executable
//! verification of every acceptance criterion.

use std::collections::HashMap;
use std::path::PathBuf;

use crap_core::domain::types::{CrapError, LineCoverage};
use crap_core::ports::{CoveragePort, ParseOutput};
use crap4ts::adapters::coverage::IstanbulCoverage;
use crap4ts::parse_diagnostic::IstanbulDiagnosticKind;
use tempfile::TempDir;

const FIXTURE_TEMPLATE: &str = include_str!("fixtures/istanbul-jest/coverage-final.json");

/// Build a canonicalised tempdir and write the five jest-fixture
/// source files into it, returning the canonical root + the
/// `coverage-final.json` payload with `{SRC_ROOT}` substituted.
///
/// Smoke tests construct `IstanbulCoverage` with the canonical root so
/// the parser's `normalize_path` can strip a real, existing prefix
/// (matching what `crap_core::core::canonicalize_src` hands the
/// factory closure at runtime).
fn build_fixture() -> (TempDir, PathBuf, String) {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Canonicalize the temp root because macOS routes /tmp through
    // /private/tmp; without canonicalization the parser sees a
    // different prefix than the fixture path entries and emits
    // `PathUnresolved` for every entry.
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");

    // Write the five TS source files. The parser doesn't actually read
    // them (the test ground-truths line-coverage records, not AST), but
    // we still write them so future integration tests can run the
    // walker against the same tree.
    write_fixture(
        &canonical,
        "simple.ts",
        include_str!("fixtures/ts-fixtures/simple.ts"),
    );
    write_fixture(
        &canonical,
        "arrow.ts",
        include_str!("fixtures/ts-fixtures/arrow.ts"),
    );
    write_fixture(
        &canonical,
        "Button.tsx",
        include_str!("fixtures/ts-fixtures/Button.tsx"),
    );
    write_fixture(
        &canonical,
        "map.ts",
        include_str!("fixtures/ts-fixtures/map.ts"),
    );
    write_fixture(
        &canonical,
        "mixed.ts",
        include_str!("fixtures/ts-fixtures/mixed.ts"),
    );

    let payload = FIXTURE_TEMPLATE.replace("{SRC_ROOT}", &canonical.to_string_lossy());
    (tmp, canonical, payload)
}

fn write_fixture(root: &std::path::Path, name: &str, content: &str) {
    let path = root.join(name);
    std::fs::write(&path, content).expect("write fixture");
}

/// Look up the `LineCoverage` records the parser emitted for the given
/// (workspace-relative) file. Helper to keep AC assertions readable.
fn lines_for<'a>(
    out: &'a ParseOutput<crap4ts::parse_diagnostic::IstanbulParseDiagnostic>,
    file: &str,
) -> &'a [LineCoverage] {
    out.coverage
        .get(file)
        .unwrap_or_else(|| {
            panic!(
                "no coverage for {file}; have: {:?}",
                out.coverage.keys().collect::<Vec<_>>()
            )
        })
        .as_slice()
}

/// Sum hits at a specific 1-based source line. Multiple statements
/// can share a line in Istanbul (e.g. the body of a single-expression
/// arrow lives on the same line as the function-declaration statement),
/// so the per-line "did it execute" answer is `sum(hits) > 0`.
fn hits_at(lines: &[LineCoverage], line: usize) -> u64 {
    lines
        .iter()
        .filter(|lc| lc.line == line)
        .map(|lc| lc.hits)
        .sum()
}

// ── 1. Happy path: parse jest fixture succeeds ────────────────────────

#[test]
fn parses_jest_fixture_with_all_five_files() {
    let (_tmp, canonical, payload) = build_fixture();
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("happy path parses");

    // All five fixture files surface in the coverage map.
    let keys: Vec<_> = out.coverage.keys().cloned().collect();
    assert!(keys.contains(&"simple.ts".to_string()), "keys: {keys:?}");
    assert!(keys.contains(&"arrow.ts".to_string()), "keys: {keys:?}");
    assert!(keys.contains(&"Button.tsx".to_string()), "keys: {keys:?}");
    assert!(keys.contains(&"map.ts".to_string()), "keys: {keys:?}");
    assert!(keys.contains(&"mixed.ts".to_string()), "keys: {keys:?}");
    // No diagnostics on a clean fixture.
    assert!(
        out.diagnostics.is_empty(),
        "unexpected: {:?}",
        out.diagnostics
    );
    // W1.1 emits no branch data.
    assert!(out.branches.is_none());
}

// ── 2. validate() returns Err on empty statementMap ───────────────────

#[test]
fn validate_returns_err_on_empty_statement_map() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("coverage-final.json");
    // Valid Istanbul shape with empty statementMap on every entry.
    let empty = r#"{ "/x/y/z.ts": { "path": "/x/y/z.ts", "s": {}, "statementMap": {} } }"#;
    std::fs::write(&path, empty).unwrap();

    let parser = IstanbulCoverage::new(PathBuf::from("/tmp"));
    let err = parser
        .validate(&path)
        .expect_err("empty statementMap rejected");
    assert!(err.contains("no statement coverage records"), "err: {err}");
}

// ── 3. validate() returns Err on bad shape ────────────────────────────

#[test]
fn validate_returns_err_on_bad_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("coverage-final.json");
    // Top-level shape isn't a HashMap<String, IstanbulCoverageFile>;
    // it's an array — common when users accidentally pipe a list of
    // file paths instead of jest's emitter output.
    std::fs::write(&path, r#"["not", "istanbul"]"#).unwrap();

    let parser = IstanbulCoverage::new(PathBuf::from("/tmp"));
    let err = parser.validate(&path).expect_err("bad shape rejected");
    assert!(
        err.contains("not a recognizable Istanbul JSON shape"),
        "err: {err}"
    );
}

// ── 4. parse() on malformed JSON returns SourceParse(istanbul:…) ──────

#[test]
fn parse_malformed_json_returns_source_parse_with_istanbul_prefix() {
    let parser = IstanbulCoverage::new(PathBuf::from("/tmp"));
    let err = parser.parse("{ not json").expect_err("malformed rejected");
    match err {
        CrapError::SourceParse(msg) => {
            assert!(
                msg.starts_with("istanbul: "),
                "msg should be prefixed: {msg}"
            );
        }
        other => panic!("expected SourceParse, got {other:?}"),
    }
}

// ── 4b. PathUnresolved diagnostic emitted on out-of-tree entries ──────

#[test]
fn parse_emits_path_unresolved_for_out_of_tree_entries() {
    let (_tmp, canonical, _payload) = build_fixture();
    // Hand-craft a fixture whose single entry sits outside the tempdir.
    let stray = r#"{
        "/somewhere/else/foreign.ts": {
            "path": "/somewhere/else/foreign.ts",
            "s": { "0": 1 },
            "statementMap": { "0": { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 5 } } }
        }
    }"#;
    let parser = IstanbulCoverage::new(canonical);
    let out = parser
        .parse(stray)
        .expect("stray entry parses but diagnoses");

    assert!(
        out.coverage.is_empty(),
        "stray entry must not become coverage"
    );
    assert_eq!(out.diagnostics.len(), 1);
    let d = &out.diagnostics[0];
    assert_eq!(d.kind, IstanbulDiagnosticKind::PathUnresolved);
    assert_eq!(d.file_path, "/somewhere/else/foreign.ts");
    assert!(
        d.message.contains("/somewhere/else/foreign.ts"),
        "msg: {}",
        d.message
    );
    assert!(
        d.message
            .contains("does not resolve to a discovered source file under"),
        "msg: {}",
        d.message
    );
}

// ── 5a. arrow.ts: square=100 hits at body line; cube=0 hits ───────────
// Per CQO ADVISORY-6: BOTH assertions required, not just one.

#[test]
fn arrow_ac_5a_square_covered_cube_uncovered() {
    let (_tmp, canonical, payload) = build_fixture();
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("happy path");

    let arrow = lines_for(&out, "arrow.ts");

    // square's body is at line 1 (single-line arrow). 100 invocations.
    let square_hits = hits_at(arrow, 1);
    assert!(
        square_hits >= 100,
        "expected `square` arrow body at line 1 to record >= 100 hits; got {square_hits}; lines={arrow:?}"
    );

    // cube's body is at line 2. 0 invocations.
    let cube_body_stmt_hits: Vec<u64> = arrow
        .iter()
        .filter(|lc| lc.line == 2)
        .map(|lc| lc.hits)
        .collect();
    // The declaration statement at line 2 still has hits=1 (the const
    // declaration ran during module load), but the arrow body
    // statement at line 2 has hits=0. AT LEAST ONE statement at line 2
    // must report 0 hits — that's the "cube body uncovered" signal.
    assert!(
        cube_body_stmt_hits.contains(&0),
        "expected at least one statement at line 2 to have 0 hits (the `cube` arrow body); got hits={cube_body_stmt_hits:?}"
    );
}

// ── 5b. Button.tsx: Button=5 + handle=5 line coverage ─────────────────

#[test]
fn arrow_ac_5b_button_and_use_callback_handle_both_100() {
    let (_tmp, canonical, payload) = build_fixture();
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("happy path");

    let button = lines_for(&out, "Button.tsx");

    // Button function body spans lines 2-5. Line 3 is the useCallback
    // declaration statement, line 4 is the return. Both must have
    // non-zero hits.
    let line3 = hits_at(button, 3);
    let line4 = hits_at(button, 4);
    assert!(
        line3 > 0,
        "Button line 3 (useCallback decl + handle body) must be covered; got {line3}"
    );
    assert!(
        line4 > 0,
        "Button line 4 (return) must be covered; got {line4}"
    );

    // The `handle` arrow body sits on line 3 (same line as the
    // useCallback call). Its statement hits >= 5 (matches Button's
    // invocation count).
    let line3_total = hits_at(button, 3);
    assert!(
        line3_total >= 5,
        "handle arrow body (line 3) must accumulate >= 5 hits; got {line3_total}"
    );
}

// ── 5c. map.ts: inner xs.map(arrow) covered + increment's CRAP uses it ─

#[test]
fn arrow_ac_5c_inner_map_arrow_covered() {
    let (_tmp, canonical, payload) = build_fixture();
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("happy path");

    let map = lines_for(&out, "map.ts");

    // The inner arrow body sits on line 2 (same line as `return xs.map(x => x + 1)`).
    // Statement hits on line 2 must be non-zero (the outer return + inner arrow body).
    let line2 = hits_at(map, 2);
    assert!(
        line2 > 0,
        "map.ts line 2 (return + inner arrow) must be covered; got {line2}"
    );

    // The arrow-specific statement (stmt id "1" in the fixture) has 3 hits.
    // Per AC 5c: "the inner arrow's line coverage in the report is 100.0
    // AND increment's CRAP score uses the arrow's coverage value".
    // At parser level we verify the arrow's hits propagate into the
    // per-line records keyed at line 2; the CRAP-score computation
    // happens in crap-core::core which consumes our `LineCoverage`
    // output. Verifying that the arrow's hit count is at least the
    // arrow's `f` count (3) ensures the downstream join sees a
    // non-trivial coverage value for line 2.
    let line2_max = map
        .iter()
        .filter(|lc| lc.line == 2)
        .map(|lc| lc.hits)
        .max()
        .unwrap_or(0);
    assert!(
        line2_max >= 3,
        "expected at least one statement on map.ts line 2 to record >= 3 hits (the arrow's f count); got max={line2_max}"
    );
}

// ── 5d. mixed.ts: declared + expression + arrow all covered ───────────

#[test]
fn arrow_ac_5d_mixed_bodies_all_covered() {
    let (_tmp, canonical, payload) = build_fixture();
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("happy path");

    let mixed = lines_for(&out, "mixed.ts");

    // declared() body sits on line 1 (single-line function).
    let line1 = hits_at(mixed, 1);
    assert!(
        line1 > 0,
        "mixed.ts line 1 (declared body) must be covered; got {line1}"
    );

    // expression function body sits on line 2.
    let line2 = hits_at(mixed, 2);
    assert!(
        line2 > 0,
        "mixed.ts line 2 (expression body) must be covered; got {line2}"
    );

    // arrow body sits on line 3.
    let line3 = hits_at(mixed, 3);
    assert!(
        line3 > 0,
        "mixed.ts line 3 (arrow body) must be covered; got {line3}"
    );
}

// ── Helper: type-only assertion that ParseOutput is HashMap-keyed ─────
// This is a sanity check that the smoke-test imports compile cleanly
// against the public API.
#[allow(dead_code)]
fn _coverage_shape_compiles(out: &ParseOutput<crap4ts::parse_diagnostic::IstanbulParseDiagnostic>) {
    let _map: &HashMap<String, Vec<LineCoverage>> = &out.coverage;
}
