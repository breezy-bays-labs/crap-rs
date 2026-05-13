//! Direct smoke tests for `OxcWalker`.
//!
//! Mirrors `istanbul_smoke.rs` in spirit — exercises the walker at the
//! unit level (no orchestrator, no CLI). The `cyclomatic_walker.feature`
//! contracts at `tests/features/cyclomatic_walker.feature` stay
//! `@unwired` until W3.3 attaches cucumber-rs harnesses; these smoke
//! tests ground-truth the same contracts directly so the W1.2 walker
//! lands with executable verification of every acceptance criterion.
//!
//! Each test asserts:
//! - exact cyclomatic complexity (decision-point count + 1)
//! - exact contributor kinds at expected line numbers
//! - exact contributor count (no double-counting, no skipping)
//!
//! Parse-failure tests assert `Err(CrapError::SourceParse(_))` with the
//! `"file/path.ts: "` prefix the orchestrator's
//! `extract_complexities` (`crap-core/src/core/mod.rs:286-310`) consumes
//! to emit `warning: skipping <file>: <error>` and increment
//! `AnalysisDiagnostics.files_unparseable`.

use crap_core::domain::types::{ComplexityMetric, ContributorKind, CrapError, FunctionComplexity};
use crap_core::ports::ComplexityPort;
use crap4ts::adapters::walker::OxcWalker;

const SIMPLE_TS: &str = include_str!("fixtures/ts-fixtures/simple.ts");
const IFBRANCH_TS: &str = include_str!("fixtures/ts-fixtures/ifbranch.ts");
const FORLOOP_TS: &str = include_str!("fixtures/ts-fixtures/forloop.ts");
const WHILELOOP_TS: &str = include_str!("fixtures/ts-fixtures/whileloop.ts");
const SWITCH_TS: &str = include_str!("fixtures/ts-fixtures/switch.ts");
const LOGICAL_TS: &str = include_str!("fixtures/ts-fixtures/logical.ts");
const NESTED_TS: &str = include_str!("fixtures/ts-fixtures/nested.ts");
const BROKEN_TS: &str = include_str!("fixtures/ts-fixtures/broken.ts");

fn extract(source: &str, file_path: &str) -> Vec<FunctionComplexity> {
    let walker = OxcWalker::new();
    walker
        .extract(source, file_path, ComplexityMetric::Cyclomatic)
        .unwrap_or_else(|e| panic!("expected Ok for fixture {file_path}, got Err: {e}"))
}

fn find_fn<'a>(fns: &'a [FunctionComplexity], name: &str) -> &'a FunctionComplexity {
    fns.iter()
        .find(|f| f.identity.qualified_name == name)
        .unwrap_or_else(|| {
            let names: Vec<_> = fns.iter().map(|f| &f.identity.qualified_name).collect();
            panic!("function `{name}` not found in walker output; got: {names:?}")
        })
}

// ── Simple: no decision points ──────────────────────────────────────────

#[test]
fn simple_function_scores_cyclomatic_one_with_no_contributors() {
    let fns = extract(SIMPLE_TS, "simple.ts");
    assert_eq!(fns.len(), 1, "expected one function in simple.ts");
    let f = &fns[0];
    assert_eq!(f.identity.qualified_name, "add");
    assert_eq!(
        f.complexity, 1,
        "base complexity is 1 for a function with no decision points"
    );
    assert_eq!(f.metric, ComplexityMetric::Cyclomatic);
    assert!(
        f.contributors.is_empty(),
        "expected no contributors for a function with no decision points; got {:?}",
        f.contributors
    );
}

// ── IfBranch ────────────────────────────────────────────────────────────

#[test]
fn if_branch_contributes_one_decision_point() {
    let fns = extract(IFBRANCH_TS, "ifbranch.ts");
    let f = find_fn(&fns, "classify");
    assert_eq!(f.complexity, 2, "if/else adds one decision point");
    assert_eq!(f.contributors.len(), 1);
    let c = &f.contributors[0];
    assert_eq!(c.kind, ContributorKind::IfBranch);
    assert_eq!(
        c.increment, 1,
        "cyclomatic contributor increments are always 1"
    );
    // The `if` keyword lives on line 3 of the fixture (1-based).
    assert_eq!(
        c.line, 3,
        "contributor should point at the `if` keyword's line"
    );
}

// ── ForLoop (all three flavours) ────────────────────────────────────────

#[test]
fn classic_for_statement_contributes_one_for_loop() {
    let fns = extract(FORLOOP_TS, "forloop.ts");
    let f = find_fn(&fns, "sumIndices");
    assert_eq!(f.complexity, 2);
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::ForLoop);
}

#[test]
fn for_of_statement_contributes_one_for_loop() {
    let fns = extract(FORLOOP_TS, "forloop.ts");
    let f = find_fn(&fns, "sumValues");
    assert_eq!(f.complexity, 2);
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::ForLoop);
}

#[test]
fn for_in_statement_contributes_one_for_loop() {
    let fns = extract(FORLOOP_TS, "forloop.ts");
    let f = find_fn(&fns, "sumKeys");
    assert_eq!(f.complexity, 2);
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::ForLoop);
}

// ── WhileLoop + DoWhileLoop ─────────────────────────────────────────────

#[test]
fn while_statement_contributes_one_while_loop() {
    let fns = extract(WHILELOOP_TS, "whileloop.ts");
    let f = find_fn(&fns, "countDown");
    assert_eq!(f.complexity, 2);
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::WhileLoop);
}

#[test]
fn do_while_statement_contributes_one_do_while_loop() {
    let fns = extract(WHILELOOP_TS, "whileloop.ts");
    let f = find_fn(&fns, "countUpAtLeastOnce");
    assert_eq!(f.complexity, 2);
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::DoWhileLoop);
}

// ── SwitchStatement: one case = one decision point ──────────────────────

#[test]
fn switch_case_contributes_one_case_branch_excluding_default() {
    let fns = extract(SWITCH_TS, "switch.ts");
    let f = find_fn(&fns, "describe");
    // `case 1:` adds 1; `default:` does not (no decision — it's the
    // fallthrough). Total = 1 + 1 = 2.
    assert_eq!(f.complexity, 2);
    let case_branches: Vec<_> = f
        .contributors
        .iter()
        .filter(|c| c.kind == ContributorKind::CaseBranch)
        .collect();
    assert_eq!(
        case_branches.len(),
        1,
        "expected exactly one case-branch contributor (default: is not counted); got contributors: {:?}",
        f.contributors
    );
    assert_eq!(f.contributors.len(), 1, "default: should NOT be counted");
}

// ── LogicalOperator: && and || only, NOT ?? ─────────────────────────────

#[test]
fn logical_and_contributes_one_logical_operator() {
    let fns = extract(LOGICAL_TS, "logical.ts");
    let f = find_fn(&fns, "bothTruthy");
    assert_eq!(f.complexity, 2);
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::LogicalOperator);
}

#[test]
fn logical_or_contributes_one_logical_operator() {
    let fns = extract(LOGICAL_TS, "logical.ts");
    let f = find_fn(&fns, "eitherTruthy");
    assert_eq!(f.complexity, 2);
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::LogicalOperator);
}

// ── Nested functions: each is its own complexity site ───────────────────

#[test]
fn nested_functions_are_separate_complexity_sites() {
    let fns = extract(NESTED_TS, "nested.ts");
    // Expect exactly two function entries: `outer` and `inner` (inner
    // is `function inner` declared inside `outer`).
    let names: Vec<_> = fns
        .iter()
        .map(|f| f.identity.qualified_name.clone())
        .collect();
    assert_eq!(
        fns.len(),
        2,
        "expected outer + inner = 2 functions; got names: {names:?}"
    );

    let outer = find_fn(&fns, "outer");
    let inner = find_fn(&fns, "inner");

    // Outer has exactly one `if (x > 0)` decision point — inner's
    // decision points must NOT bleed into outer.
    assert_eq!(outer.complexity, 2, "outer should be 1 + 1 (one if-branch)");
    let outer_ifs = outer
        .contributors
        .iter()
        .filter(|c| c.kind == ContributorKind::IfBranch)
        .count();
    assert_eq!(
        outer_ifs, 1,
        "outer should have exactly one if-branch contributor"
    );

    // Inner has exactly one `if (y > 0)` decision point — outer's
    // decision points must NOT bleed into inner.
    assert_eq!(inner.complexity, 2, "inner should be 1 + 1 (one if-branch)");
    let inner_ifs = inner
        .contributors
        .iter()
        .filter(|c| c.kind == ContributorKind::IfBranch)
        .count();
    assert_eq!(
        inner_ifs, 1,
        "inner should have exactly one if-branch contributor"
    );
}

// ── Parse failure: malformed TS bubbles up as SourceParse ───────────────

#[test]
fn malformed_typescript_returns_source_parse_with_file_prefix() {
    let walker = OxcWalker::new();
    let err = walker
        .extract(BROKEN_TS, "broken.ts", ComplexityMetric::Cyclomatic)
        .expect_err("expected parse-failure for malformed fixture");
    match err {
        CrapError::SourceParse(msg) => {
            assert!(
                msg.starts_with("broken.ts: "),
                "expected `<file_path>: ` prefix per crap4rs syn-walker convention, got: {msg:?}"
            );
            assert!(
                !msg.trim_end_matches('\n').ends_with(':'),
                "expected the underlying oxc error message to be appended, got: {msg:?}"
            );
        }
        other => panic!("expected CrapError::SourceParse, got: {other:?}"),
    }
}
