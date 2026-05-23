//! Cucumber-rs runner for `tests/features/branch_coverage.feature`.
//!
//! The four scenarios under #251 split across two output surfaces:
//!
//! - **Table** — scenarios 1 and 2 assert the conditional `Branch%`
//!   column rendered by `format_table_with_explain` (presence iff any
//!   verdict carries `branch_coverage_percent`).
//! - **JSON** — scenarios 2, 3, and 4 assert the per-function row
//!   shape of the `--format json` envelope, including
//!   `branch_coverage_percent` value, absence-vs-null semantics, and
//!   the BranchMismatch diagnostic emitted alongside a still-scored
//!   line-coverage row.
//!
//! Per-scenario the When-step shells the binary TWICE — once with
//! `--format table` and once with `--format json --verbose` — and the
//! Then-steps assert against the appropriate buffer. Both invocations
//! use `--no-fail` so threshold gating never short-circuits the
//! envelope.
//!
//! Named `*_cucumber` (suffix) so `.config/nextest.toml`'s
//! `binary(/.*_cucumber$/)` filter excludes it from nextest probing.
//! Pattern mirrors `arrow_function_coverage_cucumber.rs` /
//! `file_extensions_cucumber.rs` (lib-call shape would lose the
//! reporter half — table column rendering only exists end-to-end).

use std::path::PathBuf;

use cucumber::{World, given, then, when, writer};
use serde::Deserialize;
use serde_json::Value;
use tempfile::TempDir;

// ── Minimal envelope projection ──────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Envelope {
    result: EnvelopeResult,
    #[serde(default)]
    diagnostics: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct EnvelopeResult {
    functions: Vec<EnvelopeFn>,
}

#[derive(Debug, Deserialize)]
struct EnvelopeFn {
    scored: EnvelopeScored,
}

#[derive(Debug, Deserialize)]
struct EnvelopeScored {
    identity: EnvelopeIdentity,
    coverage_percent: f64,
    /// Per #251: `Option<f64>` with `skip_serializing_if = is_none`.
    /// Absent in the envelope ⇒ deserializes as `None` here.
    #[serde(default)]
    branch_coverage_percent: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct EnvelopeIdentity {
    file_path: String,
    qualified_name: String,
}

// ── Fixture shapes ───────────────────────────────────────────────────

/// Branch-rich fixture: the W2.3 committed
/// `branch-heavy.ts` + `coverage-with-branches.json` pair (3 functions,
/// every arm taken — 100% branch coverage across the board). Scenario
/// 1 ("a branch-coverage column for every covered function").
const BRANCH_HEAVY_TS: &str = include_str!("fixtures/ts-fixtures/branch-heavy.ts");
const COVERAGE_WITH_BRANCHES: &str =
    include_str!("fixtures/istanbul-jest/coverage-with-branches.json");

/// Empty-branches fixture: scenario 2 ("renders with no branch-coverage
/// column" + "envelope omits the `branches` field"). Uses the W1.1
/// `simple.ts` source — single function, no decision points.
const SIMPLE_TS: &str = include_str!("fixtures/ts-fixtures/simple.ts");

/// Authored inline for scenario 3 — a function whose branch arms add
/// up to 4 taken / 6 total so the rounded branch percentage is 66.7.
/// CC=3 (one if/else + one ternary), 100% line coverage; the Istanbul
/// `b`/`branchMap` declare 6 arms across two branchIds with `[1,1]`
/// + `[1,0,1,0]` hit counts — 4 of 6 taken.
///
/// The "6 branches" wording in the feature file is narration over
/// Istanbul arm counts (not the walker's CC primitive); the BDD
/// assertion is on the rounded 66.7 value.
const MIXED_TS: &str = "export function classify(n: number): string {\n  if (n > 0) {\n    return n === 0 ? \"zero\" : \"positive\";\n  } else {\n    return \"non-positive\";\n  }\n}\n";

/// Coverage JSON template for scenario 3 — substituted with `{SRC_ROOT}`
/// canonical path at fixture-build time. Two branchIds: an if (2 arms,
/// both taken) at line 2 and a 4-arm switch-style branch at line 3
/// (2 of 4 taken). Total: 4 of 6 arms taken → 66.7%. All five
/// statements covered → 100% line coverage.
const MIXED_COVERAGE_JSON: &str = r#"{
  "{SRC_ROOT}/mixed.ts": {
    "path": "{SRC_ROOT}/mixed.ts",
    "statementMap": {
      "0": { "start": { "line": 2, "column": 2 }, "end": { "line": 6, "column": 3 } },
      "1": { "start": { "line": 3, "column": 4 }, "end": { "line": 3, "column": 50 } },
      "2": { "start": { "line": 5, "column": 4 }, "end": { "line": 5, "column": 28 } }
    },
    "s": { "0": 5, "1": 3, "2": 2 },
    "branchMap": {
      "0": {
        "loc": { "start": { "line": 2, "column": 2 }, "end": { "line": 6, "column": 3 } },
        "type": "if",
        "locations": [
          { "start": { "line": 2, "column": 2 }, "end": { "line": 4, "column": 3 } },
          { "start": { "line": 4, "column": 9 }, "end": { "line": 6, "column": 3 } }
        ],
        "line": 2
      },
      "1": {
        "loc": { "start": { "line": 3, "column": 11 }, "end": { "line": 3, "column": 49 } },
        "type": "switch",
        "locations": [
          { "start": { "line": 3, "column": 17 }, "end": { "line": 3, "column": 23 } },
          { "start": { "line": 3, "column": 26 }, "end": { "line": 3, "column": 36 } },
          { "start": { "line": 3, "column": 39 }, "end": { "line": 3, "column": 44 } },
          { "start": { "line": 3, "column": 47 }, "end": { "line": 3, "column": 49 } }
        ],
        "line": 3
      }
    },
    "b": {
      "0": [1, 1],
      "1": [1, 0, 1, 0]
    },
    "fnMap": {
      "0": {
        "name": "classify",
        "decl": { "start": { "line": 1, "column": 16 }, "end": { "line": 1, "column": 24 } },
        "loc": { "start": { "line": 1, "column": 41 }, "end": { "line": 7, "column": 1 } },
        "line": 1
      }
    },
    "f": { "0": 5 }
  }
}"#;

/// Orphan-branch fixture (committed during W2.4): branchId `42` is in
/// `b` but absent from `branchMap`. The parser emits a
/// `BranchMismatch` diagnostic, skips THAT branch, and the rest of the
/// scorecard still scores. Scenario 4.
const ORPHAN_BRANCH_JSON: &str =
    include_str!("fixtures/istanbul-broken/coverage-with-orphan-branch.json");

// ── World ────────────────────────────────────────────────────────────

#[derive(Debug, Default, World)]
struct BranchWorld {
    fixture: Option<(TempDir, PathBuf)>,
    cov_path: Option<PathBuf>,
    /// Table-format stdout (whitespace-collapsed in colour-stripped
    /// form). Populated by the When-step's first invocation.
    table_output: String,
    /// `--format json --verbose` envelope, parsed.
    envelope: Option<Envelope>,
    /// `--format json --verbose` raw stdout — kept so step defs can do
    /// substring checks that distinguish absent-field from `null` when
    /// scenario 4's "`branchCoverage` is null" wording requires it.
    json_raw: String,
    stderr: String,
}

impl BranchWorld {
    fn root(&mut self) -> PathBuf {
        if self.fixture.is_none() {
            let tmp = tempfile::tempdir().expect("tempdir");
            let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");
            self.fixture = Some((tmp, canonical));
        }
        self.fixture.as_ref().unwrap().1.clone()
    }

    fn write(&mut self, name: &str, content: &str) {
        let root = self.root();
        std::fs::write(root.join(name), content).expect("write fixture file");
    }

    /// Look up the parsed envelope's row for `function` (panics with
    /// the discovered set on miss — keeps walker-naming drift legible
    /// in CI output).
    fn function(&self, name: &str) -> &EnvelopeFn {
        let env = self.envelope.as_ref().expect("envelope set by When step");
        env.result
            .functions
            .iter()
            .find(|f| f.scored.identity.qualified_name == name)
            .unwrap_or_else(|| {
                panic!(
                    "function `{name}` not in the envelope; discovered: {:?}",
                    env.result
                        .functions
                        .iter()
                        .map(|f| f.scored.identity.qualified_name.as_str())
                        .collect::<Vec<_>>(),
                )
            })
    }
}

// ── Givens ───────────────────────────────────────────────────────────

#[given("an Istanbul `coverage-final.json` whose entries include `b` and `branchMap` records")]
fn given_branch_records_present(world: &mut BranchWorld) {
    let root = world.root();
    world.write("branch-heavy.ts", BRANCH_HEAVY_TS);
    let payload =
        COVERAGE_WITH_BRANCHES.replace("{SRC_ROOT}", &root.to_string_lossy().replace('\\', "/"));
    let path = root.join("coverage-with-branches.json");
    std::fs::write(&path, payload).expect("write coverage-with-branches.json");
    world.cov_path = Some(path);
}

#[given("an Istanbul `coverage-final.json` whose entries have empty `b` and empty `branchMap`")]
fn given_empty_branches(world: &mut BranchWorld) {
    let root = world.root();
    world.write("simple.ts", SIMPLE_TS);
    // Hand-craft the empty-branches coverage entry calibrated to
    // simple.ts's `add` function at line 3 (matches the W1.1 fixture's
    // statementMap for simple.ts).
    let abs = root.join("simple.ts").to_string_lossy().replace('\\', "/");
    let payload = format!(
        r#"{{"{abs}":{{"path":"{abs}","statementMap":{{"0":{{"start":{{"line":4,"column":2}},"end":{{"line":4,"column":15}}}}}},"s":{{"0":3}},"branchMap":{{}},"b":{{}},"fnMap":{{"0":{{"name":"add","decl":{{"start":{{"line":3,"column":16}},"end":{{"line":3,"column":19}}}},"loc":{{"start":{{"line":3,"column":51}},"end":{{"line":5,"column":1}}}},"line":3}}}},"f":{{"0":3}}}}}}"#,
    );
    let path = root.join("coverage-final.json");
    std::fs::write(&path, payload).expect("write empty-branches coverage-final.json");
    world.cov_path = Some(path);
}

#[given("a TypeScript function with cyclomatic complexity 3 (one if/else, one ternary)")]
fn given_mixed_function(world: &mut BranchWorld) {
    world.write("mixed.ts", MIXED_TS);
}

#[given("a coverage-final.json showing 4 of 6 branches hit (66% branch coverage)")]
fn given_mixed_coverage(world: &mut BranchWorld) {
    let root = world.root();
    let payload =
        MIXED_COVERAGE_JSON.replace("{SRC_ROOT}", &root.to_string_lossy().replace('\\', "/"));
    let path = root.join("coverage-final.json");
    std::fs::write(&path, payload).expect("write mixed coverage-final.json");
    world.cov_path = Some(path);
}

#[given("the same function shows 100% line coverage in the `s` record")]
fn given_full_line_coverage(_world: &mut BranchWorld) {
    // The MIXED_COVERAGE_JSON template's `s` map covers every
    // instrumented statement (all three statementMap entries have
    // `s > 0`), so this Given just narrates the fixture state the
    // previous step wrote.
}

#[given(
    "an Istanbul `coverage-final.json` whose `b` references branchId `42` and `branchMap` omits `42`"
)]
fn given_orphan_branch(world: &mut BranchWorld) {
    let root = world.root();
    // The committed orphan-branch fixture targets `branch-heavy.ts` so
    // the source tree must mirror it.
    world.write("branch-heavy.ts", BRANCH_HEAVY_TS);
    let payload =
        ORPHAN_BRANCH_JSON.replace("{SRC_ROOT}", &root.to_string_lossy().replace('\\', "/"));
    let path = root.join("coverage-with-orphan-branch.json");
    std::fs::write(&path, payload).expect("write orphan-branch coverage-final.json");
    world.cov_path = Some(path);
}

// ── When ─────────────────────────────────────────────────────────────

#[when("the operator runs `crap4ts --coverage coverage-final.json --src src`")]
fn when_run_crap4ts(world: &mut BranchWorld) {
    let root = world.root();
    let cov = world
        .cov_path
        .clone()
        .expect("Given step must seed the coverage file");

    // 1) Table run — `Branch%` column visibility tests live here.
    let table_out = assert_cmd::Command::cargo_bin("crap4ts")
        .expect("crap4ts binary discoverable")
        .arg("--coverage")
        .arg(&cov)
        .arg("--src")
        .arg(&root)
        .args(["--format", "table", "--no-fail", "--color", "never"])
        .output()
        .expect("crap4ts (table run) executes");
    assert!(
        table_out.status.success(),
        "crap4ts (table) exited non-zero: stderr=\n{}",
        String::from_utf8_lossy(&table_out.stderr),
    );
    world.table_output = String::from_utf8_lossy(&table_out.stdout).into_owned();

    // 2) JSON run with `--verbose` so diagnostics surface for the
    //    BranchMismatch scenario.
    let json_out = assert_cmd::Command::cargo_bin("crap4ts")
        .expect("crap4ts binary discoverable")
        .arg("--coverage")
        .arg(&cov)
        .arg("--src")
        .arg(&root)
        .args(["--format", "json", "--no-fail", "--verbose"])
        .output()
        .expect("crap4ts (json run) executes");
    assert!(
        json_out.status.success(),
        "crap4ts (json) exited non-zero: stderr=\n{}",
        String::from_utf8_lossy(&json_out.stderr),
    );
    world.json_raw = String::from_utf8_lossy(&json_out.stdout).into_owned();
    world.stderr = String::from_utf8_lossy(&json_out.stderr).into_owned();
    world.envelope = Some(serde_json::from_str(&world.json_raw).expect("envelope parses"));
}

#[when("the operator runs `crap4ts --coverage coverage-final.json --src src --format json`")]
fn when_run_crap4ts_json(world: &mut BranchWorld) {
    // Scenario 3 explicitly invokes `--format json`; share the same
    // fixture/binary plumbing so the table + JSON outputs are both
    // available to Then steps that compose across them.
    when_run_crap4ts(world);
}

// ── Thens ────────────────────────────────────────────────────────────

#[then("the report includes a branch-coverage column for every covered function")]
fn then_branch_column_present(world: &mut BranchWorld) {
    assert!(
        world.table_output.contains("Branch%"),
        "expected the table to render a `Branch%` column when branch data is present; got=\n{}",
        world.table_output,
    );
    let env = world.envelope.as_ref().expect("envelope set");
    assert!(
        env.result
            .functions
            .iter()
            .all(|f| f.scored.branch_coverage_percent.is_some()),
        "expected every covered function to carry branch_coverage_percent; got: {:?}",
        env.result
            .functions
            .iter()
            .map(|f| (
                f.scored.identity.qualified_name.as_str(),
                f.scored.branch_coverage_percent
            ))
            .collect::<Vec<_>>(),
    );
}

#[then("the branch-coverage entries are keyed by workspace-relative paths")]
fn then_workspace_relative_paths(world: &mut BranchWorld) {
    let env = world.envelope.as_ref().expect("envelope set");
    // After `IstanbulCoverage::normalize_path`, file_path entries are
    // relative to `--src` — no absolute path or tempdir prefix should
    // appear in the envelope's per-function rows.
    for f in &env.result.functions {
        let p = &f.scored.identity.file_path;
        assert!(
            !p.starts_with('/') && !p.contains(":\\"),
            "file_path `{p}` should be workspace-relative, not absolute",
        );
    }
}

#[then("every branch record in the input is paired with its `branchMap` entry")]
fn then_branches_paired(world: &mut BranchWorld) {
    // The branch-heavy fixture has every branchId in `branchMap`;
    // a BranchMismatch diagnostic would be emitted on any orphan.
    // The verbose envelope's diagnostics field would surface one if
    // present — assert absence to confirm clean pairing.
    let env = world.envelope.as_ref().expect("envelope set");
    let saw_branch_mismatch = env
        .diagnostics
        .as_ref()
        .map(|d| d.to_string().contains("branch-mismatch"))
        .unwrap_or(false);
    assert!(
        !saw_branch_mismatch,
        "expected zero branch-mismatch diagnostics on a clean fixture; envelope diagnostics=\n{:#?}",
        env.diagnostics,
    );
}

#[then("the scorecard renders with no branch-coverage column")]
fn then_no_branch_column(world: &mut BranchWorld) {
    assert!(
        !world.table_output.contains("Branch%"),
        "expected NO `Branch%` column when all rows have absent branch_coverage_percent; got=\n{}",
        world.table_output,
    );
}

#[then("the JSON envelope omits the `branches` field (or sets it to null)")]
fn then_envelope_omits_or_nulls_branches(world: &mut BranchWorld) {
    // Per #251 the wire shape uses `skip_serializing_if =
    // "Option::is_none"`, so the field is absent on rows without
    // branch data. Accept both `null` and absent to satisfy the
    // scenario's "(or sets it to null)" alternative — the
    // distinction is opaque at the JSON-text level.
    let env = world.envelope.as_ref().expect("envelope set");
    for f in &env.result.functions {
        assert!(
            f.scored.branch_coverage_percent.is_none(),
            "function `{}` should have absent/null branch_coverage_percent on the empty-branches fixture; got {:?}",
            f.scored.identity.qualified_name,
            f.scored.branch_coverage_percent,
        );
    }
}

#[then(regex = r"^the function's `lineCoverage` is (\d+\.\d+)$")]
fn then_function_line_coverage(world: &mut BranchWorld, expected: f64) {
    let f = world.function("classify");
    assert!(
        (f.scored.coverage_percent - expected).abs() < 0.05,
        "`classify` line coverage: expected {expected}, got {}",
        f.scored.coverage_percent,
    );
}

#[then(regex = r"^the function's `branchCoverage` is (\d+\.\d+) \(rounded one decimal\)$")]
fn then_function_branch_coverage(world: &mut BranchWorld, expected: f64) {
    let f = world.function("classify");
    let actual = f
        .scored
        .branch_coverage_percent
        .expect("expected branchCoverage to be populated for the mixed-coverage fixture");
    // `expected` is the scenario's rounded value; the parser produces
    // the un-rounded percent. Match on rounded-to-one-decimal equality
    // to honour the scenario's "(rounded one decimal)" prose.
    let rounded = (actual * 10.0).round() / 10.0;
    assert!(
        (rounded - expected).abs() < 0.05,
        "`classify` branchCoverage rounded one decimal: expected {expected}, got {rounded} (raw {actual})",
    );
}

#[then("both values appear in the JSON envelope's row for the function")]
fn then_both_values_in_envelope_row(world: &mut BranchWorld) {
    let f = world.function("classify");
    assert!(f.scored.branch_coverage_percent.is_some());
    assert!(f.scored.coverage_percent.is_finite());
}

#[then("the parser emits an `IstanbulParseDiagnostic` with kind `branch-mismatch`")]
fn then_parser_emits_branch_mismatch(world: &mut BranchWorld) {
    let env = world.envelope.as_ref().expect("envelope set");
    let dx = env
        .diagnostics
        .as_ref()
        .map(|d| d.to_string())
        .unwrap_or_default();
    assert!(
        dx.contains("branch-mismatch"),
        "expected a `branch-mismatch` diagnostic in the verbose envelope; diagnostics=\n{dx}",
    );
}

#[then("the function's `branchCoverage` is `null` for the affected entry")]
fn then_affected_branch_coverage_null(world: &mut BranchWorld) {
    // The orphan fixture (`coverage-with-orphan-branch.json`) covers
    // `branch-heavy.ts` with ONE valid branchId (`0`, at line 11 —
    // inside `classify`'s span) and one orphan (`42`, dropped by the
    // parser). With the orphan skipped, the per-function shape is:
    //
    //   classify (lines 10-16) — branch 0 falls in span, 2/2 arms
    //                            taken → branch_coverage_percent = 100%
    //   sign    (lines 18-20) — no branches in span → None (absent)
    //   bucket  (lines 22-28) — no branches in span → None (absent)
    //
    // So the "is null for the affected entry" assertion lands sharply
    // on `sign` AND `bucket`: at least one function MUST have the
    // field absent (the JSON-text "null" sense — `Option<f64>::None`
    // serializes as absent under `skip_serializing_if`). The
    // assertion would fail (intentionally) if the orphan-skip
    // somehow inflated every function row with stale branch data, or
    // if the surfacing erroneously populated `Some(_)` for functions
    // without branches in their span.
    let env = world.envelope.as_ref().expect("envelope set");
    let null_count = env
        .result
        .functions
        .iter()
        .filter(|f| f.scored.branch_coverage_percent.is_none())
        .count();
    assert!(
        null_count >= 1,
        "expected at least one function row to carry branchCoverage = null/absent under the orphan-branch fixture; got rows: {:?}",
        env.result
            .functions
            .iter()
            .map(|f| (
                f.scored.identity.qualified_name.as_str(),
                f.scored.branch_coverage_percent
            ))
            .collect::<Vec<_>>(),
    );
}

#[then("the rest of the scorecard still produces line coverage for the file")]
fn then_rest_still_scored(world: &mut BranchWorld) {
    let env = world.envelope.as_ref().expect("envelope set");
    assert!(
        env.result
            .functions
            .iter()
            .any(|f| f.scored.coverage_percent.is_finite()),
        "expected at least one function with finite line coverage in the orphan-branch fixture; got=\n{:#?}",
        env.result
            .functions
            .iter()
            .map(|f| (
                f.scored.identity.qualified_name.as_str(),
                f.scored.coverage_percent
            ))
            .collect::<Vec<_>>(),
    );
}

// ── Runner ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    BranchWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .with_default_cli()
        .filter_run_and_exit("tests/features/branch_coverage.feature", |_, _, sc| {
            sc.tags.iter().any(|t| t == "wired")
        })
        .await;
}
