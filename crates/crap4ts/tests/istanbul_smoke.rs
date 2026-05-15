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

// ─────────────────────────────────────────────────────────────────────
// W2.3 (#186) — Branch coverage (b + branchMap)
// W2.4 (#187) — Schema variance + path-mismatch + missing-field paths
// ─────────────────────────────────────────────────────────────────────

const BRANCHES_FIXTURE: &str = include_str!("fixtures/istanbul-jest/coverage-with-branches.json");
const ORPHAN_BRANCH_FIXTURE: &str =
    include_str!("fixtures/istanbul-broken/coverage-with-orphan-branch.json");
const ORPHAN_PATH_FIXTURE: &str =
    include_str!("fixtures/istanbul-broken/coverage-with-orphan-path.json");
const MISSING_FIELD_FIXTURE: &str =
    include_str!("fixtures/istanbul-broken/coverage-with-missing-field.json");
const VITEST_FIXTURE: &str = include_str!("fixtures/istanbul-vitest/coverage-final.json");
const VITEST_NULL_COLUMNS_FIXTURE: &str =
    include_str!("fixtures/istanbul-vitest/coverage-with-null-columns.json");
const NYC_FIXTURE: &str = include_str!("fixtures/istanbul-nyc/coverage-final.json");
const WRAPPED_FIXTURE: &str = include_str!("fixtures/istanbul-wrapped/coverage-final.json");

/// Set up a tempdir with the branch-heavy source written and the
/// `{SRC_ROOT}` placeholder substituted in the payload. Used by all
/// W2.3 branch-coverage smoke tests.
fn build_branch_heavy_fixture(payload_template: &str) -> (TempDir, PathBuf, String) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");
    write_fixture(
        &canonical,
        "branch-heavy.ts",
        include_str!("fixtures/ts-fixtures/branch-heavy.ts"),
    );
    let payload = payload_template.replace("{SRC_ROOT}", &canonical.to_string_lossy());
    (tmp, canonical, payload)
}

/// Set up a tempdir with the three vitest/nyc fixture sources written.
/// Returns the canonical root + the resolved payload.
fn build_three_file_fixture(payload_template: &str) -> (TempDir, PathBuf, String) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");
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
        "map.ts",
        include_str!("fixtures/ts-fixtures/map.ts"),
    );
    let payload = payload_template.replace("{SRC_ROOT}", &canonical.to_string_lossy());
    (tmp, canonical, payload)
}

/// W2.3: fixture with full `b:` + `branchMap` records populates
/// `ParseOutput.branches`. Verifies (a) the option is `Some(...)`, (b)
/// the file key is present, (c) per-arm fan-out matches the fixture
/// arms (3 branchIds × {2, 2, 3} arms = 7 BranchCoverage rows).
#[test]
fn w23_branches_populated_when_b_records_present() {
    let (_tmp, canonical, payload) = build_branch_heavy_fixture(BRANCHES_FIXTURE);
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("branch-heavy parses");

    let branches = out
        .branches
        .as_ref()
        .expect("branch-heavy fixture populates Some(branches)");
    let file_branches = branches
        .get("branch-heavy.ts")
        .expect("branch-heavy.ts keyed in branches map");
    // 2 (if arms) + 2 (ternary arms) + 3 (switch arms) = 7.
    assert_eq!(
        file_branches.len(),
        7,
        "expected one BranchCoverage row per arm; got {file_branches:?}"
    );
    // No diagnostics on the clean fixture.
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

/// W2.3: branch arms expand to per-arm `taken` counts (NOT summed
/// per branchId). The fixture's `b` records are:
///   branchId 0 (if at line 11): [2, 2]   → 2 rows at line 11
///   branchId 1 (ternary at line 19): [4, 2] → 2 rows at line 19
///   branchId 2 (switch at line 23): [1, 1, 2] → 3 rows at line 23
/// Per the matching consumer at
/// `crap-core::domain::matching::compute_branch_coverage`, this is
/// what produces the correct N-of-M arm-level coverage ratios.
#[test]
fn w23_branch_arms_fan_to_per_arm_rows_not_summed() {
    let (_tmp, canonical, payload) = build_branch_heavy_fixture(BRANCHES_FIXTURE);
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("branch-heavy parses");

    let branches = out.branches.unwrap();
    let file_branches = branches.get("branch-heavy.ts").unwrap();

    // Group by line for inspection.
    let mut by_line: HashMap<usize, Vec<u64>> = HashMap::new();
    for b in file_branches {
        by_line
            .entry(b.line)
            .or_default()
            .push(b.taken.expect("Istanbul always provides taken count"));
    }
    let mut line11 = by_line.remove(&11).unwrap_or_default();
    let mut line19 = by_line.remove(&19).unwrap_or_default();
    let mut line23 = by_line.remove(&23).unwrap_or_default();
    line11.sort_unstable();
    line19.sort_unstable();
    line23.sort_unstable();
    assert_eq!(line11, vec![2, 2], "if-arms");
    assert_eq!(line19, vec![2, 4], "ternary-arms (sorted)");
    assert_eq!(line23, vec![1, 1, 2], "switch-arms (sorted)");
}

/// W2.3: orphan branchId (`b` references a missing `branchMap`
/// entry) emits `BranchMismatch` and skips ONLY that branch — the
/// rest of the file's branch records still populate, and the file's
/// statement coverage is intact.
#[test]
fn w23_orphan_branch_id_emits_branch_mismatch_and_skips_only_that_branch() {
    let (_tmp, canonical, payload) = build_branch_heavy_fixture(ORPHAN_BRANCH_FIXTURE);
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("orphan-branch parses");

    // Exactly one `BranchMismatch` diagnostic — for branchId 42.
    let mismatches: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.kind == IstanbulDiagnosticKind::BranchMismatch)
        .collect();
    assert_eq!(
        mismatches.len(),
        1,
        "expected exactly one BranchMismatch; got {:?}",
        out.diagnostics
    );
    let d = mismatches[0];
    assert!(d.message.contains("`42`"), "{:?}", d.message);
    assert!(
        d.message.contains("coverage tool's issue tracker"),
        "{:?}",
        d.message
    );

    // The valid branch (id 0) still produced its 2 arms at line 11.
    let file_branches = out
        .branches
        .as_ref()
        .expect("valid branchId still populates Some(branches)")
        .get("branch-heavy.ts")
        .expect("branch-heavy.ts keyed");
    assert_eq!(
        file_branches.len(),
        2,
        "only the non-orphan branchId's 2 arms survive; got {file_branches:?}"
    );
    // The file's statement coverage is intact (1 statement at line 11).
    let lines = lines_for(&out, "branch-heavy.ts");
    assert!(!lines.is_empty(), "line coverage survives the orphan");
}

/// W2.3 regression: a fixture with no `b:` records keeps
/// `ParseOutput.branches.is_none()`. Re-runs the W1.1 happy path and
/// re-asserts. Locks the regression-pin gate in focus.md AC #15.
#[test]
fn w23_regression_no_b_records_leaves_branches_none() {
    let (_tmp, canonical, payload) = build_fixture();
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("happy path");
    assert!(
        out.branches.is_none(),
        "W1.1 jest fixture has `\"b\": {{}}` for every entry — branches must stay None"
    );
}

/// W2.4: vitest-emitted shape (close to jest plus emitter-specific
/// metadata fields like `_coverageSchema`, `all`, `inputSourceMap`)
/// parses cleanly with no diagnostics — `#[serde(default)]` already
/// tolerates unknown fields.
#[test]
fn w24_vitest_shape_parses_with_no_diagnostics() {
    let (_tmp, canonical, payload) = build_three_file_fixture(VITEST_FIXTURE);
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("vitest parses");

    let keys: Vec<_> = out.coverage.keys().cloned().collect();
    assert!(keys.contains(&"simple.ts".to_string()), "keys: {keys:?}");
    assert!(keys.contains(&"arrow.ts".to_string()), "keys: {keys:?}");
    assert!(keys.contains(&"map.ts".to_string()), "keys: {keys:?}");
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert!(out.branches.is_none());
}

/// #211 (surfaced during W3.1 #189): `@vitest/coverage-istanbul`
/// emits `"column": null` on the `end` side of every span entry
/// (statementMap, fnMap.decl, fnMap.loc, branchMap.loc,
/// branchMap.locations[]) because the underlying V8 inspector data
/// it transforms doesn't always have a precise end-column. The
/// parser must accept null columns and treat them as advisory
/// "unknown column" data; line-range matching is line-only so the
/// column value is never consulted downstream. Regression: before
/// the `Position.column: Option<u32>` fix, this fixture failed
/// deserialization with `invalid type: null, expected u32`, bailing
/// the entire analysis pre-discovery.
#[test]
fn w24_vitest_null_columns_parse_without_bailing() {
    let tmp = tempfile::tempdir().unwrap();
    let canonical = std::fs::canonicalize(tmp.path()).unwrap();
    write_fixture(
        &canonical,
        "simple.ts",
        include_str!("fixtures/ts-fixtures/simple.ts"),
    );
    let payload = VITEST_NULL_COLUMNS_FIXTURE.replace("{SRC_ROOT}", &canonical.to_string_lossy());

    let parser = IstanbulCoverage::new(canonical);
    let out = parser
        .parse(&payload)
        .expect("null-column fixture parses successfully (regression for #211)");

    let keys: Vec<_> = out.coverage.keys().cloned().collect();
    assert_eq!(
        keys,
        vec!["simple.ts".to_string()],
        "expected single file keyed `simple.ts`; got {keys:?}"
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);

    // Branch records also survive the null-column treatment: the fixture
    // declares one branch with two arms and `b: [3, 0]` — per-arm fan-out
    // produces two BranchCoverage rows at line 4.
    let branches = out
        .branches
        .as_ref()
        .expect("branchMap present → branches Some(...)");
    let file_branches = branches
        .get("simple.ts")
        .expect("simple.ts keyed in branches map");
    assert_eq!(file_branches.len(), 2, "{file_branches:?}");
}

/// W2.4: nyc-emitted shape uses absolute paths; the existing
/// `normalize_path` strip-prefix handles this since the
/// `{SRC_ROOT}` substitution canonicalizes paths to be tempdir-rooted.
#[test]
fn w24_nyc_absolute_paths_normalize_via_strip_prefix() {
    let (_tmp, canonical, payload) = build_three_file_fixture(NYC_FIXTURE);
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("nyc parses");

    let keys: Vec<_> = out.coverage.keys().cloned().collect();
    assert!(keys.contains(&"simple.ts".to_string()), "keys: {keys:?}");
    assert!(keys.contains(&"arrow.ts".to_string()), "keys: {keys:?}");
    assert!(keys.contains(&"map.ts".to_string()), "keys: {keys:?}");
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

/// W2.4: wrapped shape `{"coverage-final": {...flat...}}` parses via
/// the one-level unwrap arm; the inner flat map is consumed normally.
#[test]
fn w24_wrapped_shape_parses_via_single_level_unwrap() {
    let tmp = tempfile::tempdir().unwrap();
    let canonical = std::fs::canonicalize(tmp.path()).unwrap();
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
    let payload = WRAPPED_FIXTURE.replace("{SRC_ROOT}", &canonical.to_string_lossy());

    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("wrapped parses via unwrap");

    let keys: Vec<_> = out.coverage.keys().cloned().collect();
    assert!(keys.contains(&"simple.ts".to_string()), "keys: {keys:?}");
    assert!(keys.contains(&"arrow.ts".to_string()), "keys: {keys:?}");
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

/// W2.4: a JSON shape that is neither flat-Istanbul nor wrapped-
/// Istanbul emits exactly one `SchemaUnrecognized` diagnostic whose
/// message lists the detected top-level keys in sorted order — the
/// "received: [...]" hint per breadboard W-3.
#[test]
fn w24_unrecognized_shape_emits_schema_unrecognized_with_detected_keys() {
    let parser = IstanbulCoverage::new(PathBuf::from("/tmp"));
    let out = parser
        .parse(r#"{"foo": "bar", "baz": 42}"#)
        .expect("parse returns Ok with diagnostic; downstream produces non-zero exit");

    assert!(out.coverage.is_empty(), "no coverage on unrecognized shape");
    assert!(out.branches.is_none());
    assert_eq!(out.diagnostics.len(), 1, "{:?}", out.diagnostics);
    let d = &out.diagnostics[0];
    assert_eq!(d.kind, IstanbulDiagnosticKind::SchemaUnrecognized);
    assert_eq!(d.file_path, "");
    assert!(
        d.message
            .contains("top-level shape not recognized as Istanbul"),
        "{:?}",
        d.message
    );
    // Keys are sorted; expect "[baz, foo]" in the received-keys hint.
    assert!(d.message.contains("[baz, foo]"), "{:?}", d.message);
}

/// W2.4: orphan-path entries emit `PathUnresolved` but the valid
/// entries in the same fixture still parse — never abort first-record,
/// never silent-drop.
#[test]
fn w24_orphan_path_emits_path_unresolved_and_valid_entries_still_parse() {
    let (_tmp, canonical, payload) = build_branch_heavy_fixture(ORPHAN_PATH_FIXTURE);
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("orphan-path parses");

    let unresolved: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.kind == IstanbulDiagnosticKind::PathUnresolved)
        .collect();
    assert_eq!(unresolved.len(), 1, "{:?}", out.diagnostics);
    assert_eq!(unresolved[0].file_path, "/build/transpiled/foreign.js");
    assert!(
        unresolved[0]
            .message
            .contains("/build/transpiled/foreign.js"),
        "{:?}",
        unresolved[0].message
    );

    // The valid entry still parsed.
    let keys: Vec<_> = out.coverage.keys().cloned().collect();
    assert!(
        keys.contains(&"branch-heavy.ts".to_string()),
        "valid entry survives orphan-path; keys: {keys:?}"
    );
    let lines = lines_for(&out, "branch-heavy.ts");
    assert!(!lines.is_empty(), "valid line coverage intact");
}

/// W2.4: an entry that has `s` records but an empty `statementMap`
/// emits `MissingField` and skips only that entry — the valid entry
/// in the same fixture still parses.
#[test]
fn w24_missing_field_emits_diagnostic_and_skips_only_that_entry() {
    let (_tmp, canonical, payload) = build_branch_heavy_fixture(MISSING_FIELD_FIXTURE);
    let parser = IstanbulCoverage::new(canonical);
    let out = parser.parse(&payload).expect("missing-field parses");

    let missing: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.kind == IstanbulDiagnosticKind::MissingField)
        .collect();
    assert_eq!(missing.len(), 1, "{:?}", out.diagnostics);
    let d = missing[0];
    assert!(d.message.contains("orphan-sm.ts"), "{:?}", d.message);
    assert!(d.message.contains("`s`"), "{:?}", d.message);
    assert!(d.message.contains("`statementMap`"), "{:?}", d.message);

    // The valid entry still parsed.
    let keys: Vec<_> = out.coverage.keys().cloned().collect();
    assert!(
        keys.contains(&"branch-heavy.ts".to_string()),
        "valid entry survives missing-field skip; keys: {keys:?}"
    );
}

/// W2.4 regression: malformed JSON (not even valid JSON syntax)
/// continues to fail fatally with `Err(CrapError::SourceParse(
/// "istanbul: …"))` — the schema-tolerance arms do NOT swallow a
/// fundamental "this isn't JSON" error.
#[test]
fn w24_truly_malformed_json_still_returns_source_parse_error() {
    let parser = IstanbulCoverage::new(PathBuf::from("/tmp"));
    let err = parser.parse("{not json at all").unwrap_err();
    match err {
        CrapError::SourceParse(msg) => assert!(msg.starts_with("istanbul: "), "msg: {msg}"),
        other => panic!("expected SourceParse, got {other:?}"),
    }
}

/// W2.4 parity: `validate()` accepts wrapped shape so the CLI's
/// pre-flight gate doesn't short-circuit the parse cascade. Without
/// this parity, `IstanbulCoverage::validate` would reject
/// `{"coverage-final": {...}}` with "not a recognizable Istanbul JSON
/// shape" before parse ever ran the unwrap arm, defeating W2.4 at the
/// CLI layer.
#[test]
fn w24_validate_accepts_wrapped_shape_for_cli_parity_with_parse() {
    let tmp = tempfile::tempdir().unwrap();
    let canonical = std::fs::canonicalize(tmp.path()).unwrap();
    let payload = WRAPPED_FIXTURE.replace("{SRC_ROOT}", &canonical.to_string_lossy());
    let cov_path = canonical.join("coverage-final.json");
    std::fs::write(&cov_path, payload).unwrap();

    let parser = IstanbulCoverage::new(canonical);
    parser
        .validate(&cov_path)
        .expect("validate accepts wrapped shape (parity with parse)");
}

/// W2.4: a valid JSON array (e.g. `[]` or `["foo"]`) is not Istanbul
/// shape and emits `SchemaUnrecognized` with `received keys: array`
/// as the detected-type hint (objects-only get key listings).
#[test]
fn w24_top_level_array_emits_schema_unrecognized() {
    let parser = IstanbulCoverage::new(PathBuf::from("/tmp"));
    let out = parser
        .parse(r#"["not", "istanbul"]"#)
        .expect("returns Ok with diagnostic");
    assert_eq!(out.diagnostics.len(), 1);
    assert_eq!(
        out.diagnostics[0].kind,
        IstanbulDiagnosticKind::SchemaUnrecognized
    );
    assert!(
        out.diagnostics[0].message.contains("received keys: array"),
        "{:?}",
        out.diagnostics[0].message
    );
}
