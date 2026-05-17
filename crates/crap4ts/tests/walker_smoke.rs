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

// W2.1 (#184): TS-specific decision-point fixtures.
const TERNARY_TS: &str = include_str!("fixtures/ts-fixtures/ternary.ts");
const OPTIONAL_CHAIN_TS: &str = include_str!("fixtures/ts-fixtures/optional-chain.ts");
const NULLISH_TS: &str = include_str!("fixtures/ts-fixtures/nullish.ts");
const TRY_CATCH_TS: &str = include_str!("fixtures/ts-fixtures/try-catch.ts");
const JSX_CONDITIONAL_TSX: &str = include_str!("fixtures/ts-fixtures/jsx-conditional.tsx");
const CHAINED_TERNARY_TS: &str = include_str!("fixtures/ts-fixtures/chained-ternary.ts");
const CHAINED_LOGICAL_TS: &str = include_str!("fixtures/ts-fixtures/chained-logical.ts");
const NESTED_IFS_TS: &str = include_str!("fixtures/ts-fixtures/nested-ifs.ts");
const COMPOUND_IF_AND_TS: &str = include_str!("fixtures/ts-fixtures/compound-if-and.ts");

// W2.2 (#185): file-extension dispatch fixtures.
const EXAMPLE_TSX: &str = include_str!("fixtures/ts-fixtures/example.tsx");
const EXAMPLE_JSX: &str = include_str!("fixtures/ts-fixtures/example.jsx");
const EXAMPLE_JS: &str = include_str!("fixtures/ts-fixtures/example.js");
const EXAMPLE_MJS: &str = include_str!("fixtures/ts-fixtures/example.mjs");
const EXAMPLE_CJS: &str = include_str!("fixtures/ts-fixtures/example.cjs");

// #199: class-field arrow + static-block discovery fixtures.
const CLASS_FIELD_ARROWS_TSX: &str = include_str!("fixtures/ts-fixtures/class-field-arrows.tsx");
const STATIC_BLOCK_TS: &str = include_str!("fixtures/ts-fixtures/static-block.ts");

// #200 + #205: walker traversal coverage-gap fixtures.
const COMPUTED_OBJECT_KEY_TS: &str = include_str!("fixtures/ts-fixtures/computed-object-key.ts");
const NAMESPACE_TS: &str = include_str!("fixtures/ts-fixtures/namespace.ts");
const NAMESPACE_DOTTED_TS: &str = include_str!("fixtures/ts-fixtures/namespace-dotted.ts");
const NAMESPACE_NESTED_TS: &str = include_str!("fixtures/ts-fixtures/namespace-nested.ts");
const NAMESPACE_CLASS_TS: &str = include_str!("fixtures/ts-fixtures/namespace-class.ts");
const NAMESPACE_CONST_FN_TS: &str =
    include_str!("fixtures/ts-fixtures/namespace-const-fn.ts");
const ASSIGNMENT_COMPUTED_LHS_TS: &str =
    include_str!("fixtures/ts-fixtures/assignment-computed-lhs.ts");
const UPDATE_COMPUTED_OPERAND_TS: &str =
    include_str!("fixtures/ts-fixtures/update-computed-operand.ts");
const COMPUTED_CLASS_KEY_TS: &str = include_str!("fixtures/ts-fixtures/computed-class-key.ts");
// gemini PR #220 review: static-member object recursion (assignment + update).
const ASSIGNMENT_STATIC_MEMBER_OBJECT_TS: &str =
    include_str!("fixtures/ts-fixtures/assignment-static-member-object.ts");
const UPDATE_STATIC_MEMBER_OBJECT_TS: &str =
    include_str!("fixtures/ts-fixtures/update-static-member-object.ts");

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

// ── Panic-safety: multi-byte UTF-8 spans don't crash byte_to_line_col ──

#[test]
fn unicode_identifiers_do_not_panic_in_span_to_column_conversion() {
    // Regression: prior to using a byte-based scan, `byte_to_line_col`
    // sliced `&source[..limit]` where `limit = span.end - 1` could land
    // inside a multi-byte UTF-8 character (e.g., the trailing byte of
    // an identifier containing non-ASCII letters), panicking with
    // "byte index N is not a char boundary".
    let source = "function π(αβγ: number) {\n  if (αβγ > 0) return αβγ;\n  return 0;\n}\n";
    let fns = extract(source, "unicode.ts");
    assert_eq!(fns.len(), 1, "expected one function in unicode.ts");
    let f = &fns[0];
    assert_eq!(f.identity.qualified_name, "π");
    assert_eq!(f.complexity, 2, "if-branch contributes one decision point");
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::IfBranch);
}

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

// ── W2.1 (#184) — TS-specific cyclomatic decision points ───────────────
//
// Each fixture isolates one decision-point kind so the assertions stay
// surgical. Mirrors the `cyclomatic_walker.feature` TS-specific outline
// (`@unwired` until W3.3) row-for-row.

#[test]
fn ternary_contributes_one_ternary_decision_point() {
    let fns = extract(TERNARY_TS, "ternary.ts");
    let f = find_fn(&fns, "abs");
    assert_eq!(f.complexity, 2, "ternary adds one decision point");
    assert_eq!(f.contributors.len(), 1);
    let c = &f.contributors[0];
    assert_eq!(c.kind, ContributorKind::Ternary);
    assert_eq!(c.increment, 1);
    // The ternary lives on line 2 of the fixture (the `return` body).
    assert_eq!(c.line, 2);
}

#[test]
fn optional_member_chain_contributes_one_optional_chain() {
    let fns = extract(OPTIONAL_CHAIN_TS, "optional-chain.ts");
    let f = find_fn(&fns, "pickField");
    // `obj?.nested?.value` parses as ONE ChainExpression (two optional
    // links inside one chain). Per the BDD outline + ADR (a), this is
    // ONE OptionalChain contributor — chain count, not link count.
    assert_eq!(f.complexity, 2, "one ?.chain adds one decision point");
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::OptionalChain);
}

#[test]
fn optional_call_chain_contributes_one_optional_chain() {
    let fns = extract(OPTIONAL_CHAIN_TS, "optional-chain.ts");
    let f = find_fn(&fns, "callMethod");
    assert_eq!(f.complexity, 2, "obj?.method() is one chain → one point");
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::OptionalChain);
}

#[test]
fn nullish_coalescing_contributes_one_logical_operator() {
    let fns = extract(NULLISH_TS, "nullish.ts");
    let f = find_fn(&fns, "withDefault");
    // `??` maps to LogicalOperator per ADR (a) — no `NullishCoalesce`
    // variant; the existing enum is sufficient.
    assert_eq!(f.complexity, 2, "?? adds one decision point");
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::LogicalOperator);
}

#[test]
fn try_catch_contributes_one_catch_decision_point() {
    let fns = extract(TRY_CATCH_TS, "try-catch.ts");
    let f = find_fn(&fns, "safeParse");
    // try { ... } catch (e) { ... } scores ONE Catch contributor on
    // handler.is_some(). The body of `try` adds no decision points.
    assert_eq!(f.complexity, 2, "try/catch adds one decision point");
    let catches: Vec<_> = f
        .contributors
        .iter()
        .filter(|c| c.kind == ContributorKind::Catch)
        .collect();
    assert_eq!(catches.len(), 1, "expected exactly one Catch contributor");
    assert_eq!(f.contributors.len(), 1, "no other contributors expected");
}

#[test]
fn try_finally_without_handler_contributes_no_catch() {
    let fns = extract(TRY_CATCH_TS, "try-catch.ts");
    let f = find_fn(&fns, "noHandler");
    // `try { } finally { }` has no `handler` → no Catch contributor.
    // The function body otherwise has no decision points.
    assert_eq!(
        f.complexity, 1,
        "try/finally without handler is not a decision point"
    );
    assert!(
        f.contributors.is_empty(),
        "expected no contributors for try/finally without handler; got {:?}",
        f.contributors
    );
}

// ── W2.1 (#184) — JSX conditional rendering ────────────────────────────

#[test]
fn jsx_conditional_decomposes_through_logical_operator() {
    let fns = extract(JSX_CONDITIONAL_TSX, "jsx-conditional.tsx");
    let f = find_fn(&fns, "Greeting");
    // `<div>{visible && <span>...{name}</span>}</div>` — the `&&` is
    // the ONLY decision point. The JSX wrapper itself adds nothing.
    // `{name}` is a bare identifier inside a JSXExpressionContainer →
    // no contributor.
    assert_eq!(
        f.complexity, 2,
        "JSX conditional adds exactly one decision point via the inner &&"
    );
    let logical_ops: Vec<_> = f
        .contributors
        .iter()
        .filter(|c| c.kind == ContributorKind::LogicalOperator)
        .collect();
    assert_eq!(
        logical_ops.len(),
        1,
        "expected exactly one logical-operator contributor; got {:?}",
        f.contributors
    );
    assert_eq!(f.contributors.len(), 1, "no other contributors expected");
}

// ── W2.1 (#184) — Compound counting: chains/nestings never flatten ─────

#[test]
fn chained_ternary_contributes_one_per_question_mark() {
    let fns = extract(CHAINED_TERNARY_TS, "chained-ternary.ts");
    let f = find_fn(&fns, "classify");
    // `x < 0 ? "neg" : x === 0 ? "zero" : "pos"` parses as
    // `x < 0 ? "neg" : (x === 0 ? "zero" : "pos")` — two nested
    // ConditionalExpression nodes → CC=3, two Ternary contributors.
    assert_eq!(f.complexity, 3, "two ?'s add two decision points");
    let ternaries: Vec<_> = f
        .contributors
        .iter()
        .filter(|c| c.kind == ContributorKind::Ternary)
        .collect();
    assert_eq!(
        ternaries.len(),
        2,
        "expected two Ternary contributors (one per ?); got: {:?}",
        f.contributors
    );
}

#[test]
fn chained_logical_and_contributes_one_per_operator() {
    let fns = extract(CHAINED_LOGICAL_TS, "chained-logical.ts");
    let f = find_fn(&fns, "allTruthy");
    // `a && b && c && d` parses as `((a && b) && c) && d` —
    // three LogicalExpression nodes → CC=4, three LogicalOperator
    // contributors. No flattening.
    assert_eq!(f.complexity, 4, "three &&'s add three decision points");
    let logicals: Vec<_> = f
        .contributors
        .iter()
        .filter(|c| c.kind == ContributorKind::LogicalOperator)
        .collect();
    assert_eq!(
        logicals.len(),
        3,
        "expected three LogicalOperator contributors; got: {:?}",
        f.contributors
    );
}

#[test]
fn nested_ifs_contribute_one_per_if_branch() {
    let fns = extract(NESTED_IFS_TS, "nested-ifs.ts");
    let f = find_fn(&fns, "deep");
    // Outer `if (a > 0)` + inner `if (b > 0)` — both score even though
    // the inner is at higher nesting depth. CC = 1 + 2 = 3.
    assert_eq!(f.complexity, 3, "two nested ifs add two decision points");
    let ifs: Vec<_> = f
        .contributors
        .iter()
        .filter(|c| c.kind == ContributorKind::IfBranch)
        .collect();
    assert_eq!(
        ifs.len(),
        2,
        "expected two IfBranch contributors (one per if, no flattening); got: {:?}",
        f.contributors
    );
}

#[test]
fn compound_if_and_counts_both_if_and_logical_operator() {
    let fns = extract(COMPOUND_IF_AND_TS, "compound-if-and.ts");
    let f = find_fn(&fns, "both");
    // `if (a && b)` — counts the `if` AND the `&&`. CC = 1 + 2 = 3.
    assert_eq!(
        f.complexity, 3,
        "compound `if (a && b)` should add the if AND the && (no skipping)"
    );
    let ifs = f
        .contributors
        .iter()
        .filter(|c| c.kind == ContributorKind::IfBranch)
        .count();
    let logicals = f
        .contributors
        .iter()
        .filter(|c| c.kind == ContributorKind::LogicalOperator)
        .count();
    assert_eq!(ifs, 1, "expected exactly one IfBranch contributor");
    assert_eq!(
        logicals, 1,
        "expected exactly one LogicalOperator contributor"
    );
    assert_eq!(f.contributors.len(), 2, "expected exactly two contributors");
}

// ── W2.2 (#185) — File-extension dispatch ─────────────────────────────
//
// W1.2 wired SourceType::from_path as the canonical dispatcher, which
// already covers all six AdapterMeta::extensions plus `.mts` / `.cts` /
// `.d.ts`. These tests verify end-to-end that each extension parses +
// surfaces at least one function. See PR body for W2.2 plan deviation
// (no hand-rolled match needed).

#[test]
fn tsx_fixture_parses_and_discovers_jsx_function() {
    let fns = extract(EXAMPLE_TSX, "example.tsx");
    assert!(
        !fns.is_empty(),
        "expected at least one function in example.tsx; got: {:?}",
        fns
    );
    assert!(
        fns.iter().any(|f| f.identity.qualified_name == "Greet"),
        "expected `Greet` function in example.tsx; got names: {:?}",
        fns.iter()
            .map(|f| &f.identity.qualified_name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn jsx_fixture_parses_and_discovers_default_export_arrow() {
    let fns = extract(EXAMPLE_JSX, "example.jsx");
    assert!(
        !fns.is_empty(),
        "expected at least one function in example.jsx; got: {:?}",
        fns
    );
    assert!(
        fns.iter().any(|f| f.identity.qualified_name == "Greet"),
        "expected `Greet` function in example.jsx; got names: {:?}",
        fns.iter()
            .map(|f| &f.identity.qualified_name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn js_fixture_parses_and_discovers_function() {
    let fns = extract(EXAMPLE_JS, "example.js");
    assert!(
        !fns.is_empty(),
        "expected at least one function in example.js"
    );
    assert!(fns.iter().any(|f| f.identity.qualified_name == "greet"));
}

#[test]
fn mjs_fixture_parses_and_discovers_function() {
    let fns = extract(EXAMPLE_MJS, "example.mjs");
    assert!(
        !fns.is_empty(),
        "expected at least one function in example.mjs"
    );
    assert!(fns.iter().any(|f| f.identity.qualified_name == "greet"));
}

#[test]
fn cjs_fixture_parses_and_discovers_function() {
    let fns = extract(EXAMPLE_CJS, "example.cjs");
    // CJS `module.exports.greet = function (name) { ... }` is an
    // anonymous FunctionExpression assigned to a member — the walker
    // records it as `<anonymous>` (the only sentinel that fits without
    // tracking assignment LHS, which is beyond W2.2 scope). The
    // contract is "at least one function discovered" per the BDD spec.
    assert!(
        !fns.is_empty(),
        "expected at least one function in example.cjs; got: {:?}",
        fns
    );
}

// ── #199 — Class-field arrows + StaticBlock discovery ────────────────

#[test]
fn class_field_arrow_initializers_are_discovered_as_functions() {
    let fns = extract(CLASS_FIELD_ARROWS_TSX, "class-field-arrows.tsx");
    let names: Vec<_> = fns
        .iter()
        .map(|f| f.identity.qualified_name.clone())
        .collect();

    // Three function-entry sites on the class:
    //   onClick = () => {...}              (PropertyDefinition + arrow)
    //   onSubmit = (e) => { if (...) ... } (PropertyDefinition + arrow)
    //   static setup = function () {...}   (PropertyDefinition + FE)
    let on_click = find_fn(&fns, "Form.onClick");
    assert_eq!(
        on_click.complexity, 1,
        "Form.onClick has no decision points"
    );
    assert!(
        on_click.contributors.is_empty(),
        "expected no contributors for Form.onClick; got: {:?}",
        on_click.contributors
    );

    let on_submit = find_fn(&fns, "Form.onSubmit");
    assert_eq!(
        on_submit.complexity, 2,
        "Form.onSubmit's if-branch adds one decision point"
    );
    assert_eq!(on_submit.contributors.len(), 1);
    assert_eq!(on_submit.contributors[0].kind, ContributorKind::IfBranch);

    // `static setup = function () { return new Form(); }` is a
    // FunctionExpression initializer — discovered with no decision
    // points.
    let setup = find_fn(&fns, "Form.setup");
    assert_eq!(setup.complexity, 1);
    assert!(setup.contributors.is_empty());

    // Sanity check: the bare class-field `touched = false;` does NOT
    // mint a synthetic function (it's a literal initializer, not a
    // function-shaped expression).
    assert!(
        !names.iter().any(|n| n == "Form.touched"),
        "expected no entry for literal class-field `touched`; got names: {names:?}"
    );
}

#[test]
fn static_block_is_discovered_as_synthetic_static_init_function() {
    let fns = extract(STATIC_BLOCK_TS, "static-block.ts");
    let synthetic = find_fn(&fns, "Registry.<static-init>");
    // The static block contains `if (seed) { ... }` — one IfBranch.
    assert_eq!(
        synthetic.complexity, 2,
        "Registry.<static-init> has one if-branch"
    );
    let ifs = synthetic
        .contributors
        .iter()
        .filter(|c| c.kind == ContributorKind::IfBranch)
        .count();
    assert_eq!(ifs, 1, "expected one IfBranch in the static block body");
}

// ── #200 + #205 — Walker traversal coverage gaps ──────────────────────
//
// Each fixture isolates one previously-uncovered traversal path so the
// assertions stay surgical: exact CC, exact ContributorKind, exact
// line, exact function set (no leakage, no double-count). Mirrors the
// W2.1 smoke-test rigor above.

#[test]
fn computed_object_key_decision_point_charges_enclosing_function() {
    // #200 item 1: `{ [a && b]: 1, ...rest }` — the `&&` in the
    // computed key is a LogicalOperator that must charge `build`.
    // The spread property must NOT mint any contributor.
    let fns = extract(COMPUTED_OBJECT_KEY_TS, "computed-object-key.ts");
    assert_eq!(fns.len(), 1, "expected exactly one function (build)");
    let f = find_fn(&fns, "build");
    assert_eq!(
        f.complexity, 2,
        "the && inside the computed key adds exactly one decision point"
    );
    assert_eq!(f.contributors.len(), 1, "spread must not add a contributor");
    let c = &f.contributors[0];
    assert_eq!(c.kind, ContributorKind::LogicalOperator);
    assert_eq!(c.increment, 1);
    // `[a && b]: 1` is on line 7 of the fixture (1-based).
    assert_eq!(c.line, 7, "contributor points at the computed-key line");
}

#[test]
fn namespace_nested_function_is_discovered_as_separate_site() {
    // `namespace Foo { export function bar() { if … } }` — `bar` is
    // its own FunctionComplexity, recorded namespace-qualified as
    // `Foo.bar` (mirroring class methods, which qualify as `C.m`). Its
    // `if` charges `Foo.bar`, not module scope. The trailing
    // declaration-only `declare module "side-effect-only";` (body:
    // None) must be a clean no-op (no panic, no extra function).
    let fns = extract(NAMESPACE_TS, "namespace.ts");
    let names: Vec<_> = fns
        .iter()
        .map(|f| f.identity.qualified_name.clone())
        .collect();
    assert_eq!(
        fns.len(),
        1,
        "expected exactly one function (`Foo.bar`); got names: {names:?}"
    );
    let bar = find_fn(&fns, "Foo.bar");
    assert_eq!(bar.complexity, 2, "bar's `if` adds one decision point");
    assert_eq!(bar.contributors.len(), 1);
    assert_eq!(bar.contributors[0].kind, ContributorKind::IfBranch);
    // The `if` is on line 6 of the fixture (1-based).
    assert_eq!(bar.contributors[0].line, 6);
}

#[test]
fn namespace_dotted_continuation_qualifies_with_full_path() {
    // `namespace A.B { export function f }` parses as `A` whose module
    // body is the nested declaration `B`. The qualified name carries
    // the full dotted path `A.B.f` — not module scope, not a partial
    // `B.f` — and the `if` charges `A.B.f`.
    let fns = extract(NAMESPACE_DOTTED_TS, "namespace-dotted.ts");
    assert_eq!(fns.len(), 1, "expected exactly one function (`A.B.f`)");
    let f = find_fn(&fns, "A.B.f");
    assert_eq!(f.complexity, 2, "f's `if` adds one decision point");
    assert_eq!(f.contributors.len(), 1);
    assert_eq!(f.contributors[0].kind, ContributorKind::IfBranch);
}

#[test]
fn namespace_block_nested_qualifies_same_as_dotted_and_is_shallow() {
    // `namespace A { function outer; namespace B { function g } }` —
    // the block-nested path yields the same `A.B.g` as the dotted
    // form, and `outer` declared in the outer block is `A.outer`.
    // Qualification is SHALLOW: `inner`, nested inside `g`, keeps its
    // bare name (mirrors a function nested inside a class method).
    let fns = extract(NAMESPACE_NESTED_TS, "namespace-nested.ts");
    let names: Vec<_> = fns
        .iter()
        .map(|f| f.identity.qualified_name.clone())
        .collect();
    assert_eq!(
        fns.len(),
        3,
        "expected A.outer, A.B.g, inner; got: {names:?}"
    );
    find_fn(&fns, "A.outer");
    let g = find_fn(&fns, "A.B.g");
    assert_eq!(g.complexity, 2, "g's `if` adds one; `inner` does not bleed");
    let inner = find_fn(&fns, "inner");
    assert_eq!(
        inner.complexity, 1,
        "shallow qualification: nested `inner` stays bare and isolated"
    );
}

#[test]
fn namespace_class_methods_carry_namespace_then_class_prefix() {
    // A class inside a namespace qualifies methods with both prefixes:
    // `Svc.Repo.find`, not `Repo.find`. A namespace-level function
    // alongside it is `Svc.helper`.
    let fns = extract(NAMESPACE_CLASS_TS, "namespace-class.ts");
    let names: Vec<_> = fns
        .iter()
        .map(|f| f.identity.qualified_name.clone())
        .collect();
    assert_eq!(
        fns.len(),
        2,
        "expected Svc.helper + Svc.Repo.find; got: {names:?}"
    );
    find_fn(&fns, "Svc.helper");
    let find = find_fn(&fns, "Svc.Repo.find");
    assert_eq!(
        find.complexity, 2,
        "find's `if` adds one decision point"
    );
    assert_eq!(find.contributors[0].kind, ContributorKind::IfBranch);
}

#[test]
fn namespace_function_valued_const_bindings_carry_namespace_prefix() {
    // Function-valued `const`s in a namespace are discovered with the
    // namespace prefix regardless of whether they reach the walker
    // through the bare statement path (`const bare = …`) or the
    // `export` declaration path (`export const exported = …`):
    // `Calc.bare` and `Calc.exported`, each CC 2 from its own `if`.
    let fns = extract(NAMESPACE_CONST_FN_TS, "namespace-const-fn.ts");
    let names: Vec<_> = fns
        .iter()
        .map(|f| f.identity.qualified_name.clone())
        .collect();
    assert_eq!(
        fns.len(),
        2,
        "expected Calc.bare + Calc.exported; got: {names:?}"
    );
    let bare = find_fn(&fns, "Calc.bare");
    assert_eq!(bare.complexity, 2, "bare's `if` adds one decision point");
    assert_eq!(bare.contributors[0].kind, ContributorKind::IfBranch);
    let exported = find_fn(&fns, "Calc.exported");
    assert_eq!(
        exported.complexity, 2,
        "exported's `if` adds one decision point"
    );
    assert_eq!(exported.contributors[0].kind, ContributorKind::IfBranch);
}

#[test]
fn assignment_computed_lhs_iife_is_discovered() {
    // #200 item 3: `target[(() => { if … })()] = 1` — the IIFE arrow
    // in the index expression is its own FunctionComplexity (CC 2 from
    // its own `if`); `assign` itself stays CC 1 (the `if` must not
    // bleed into the enclosing function).
    let fns = extract(ASSIGNMENT_COMPUTED_LHS_TS, "assignment-computed-lhs.ts");
    let names: Vec<_> = fns
        .iter()
        .map(|f| f.identity.qualified_name.clone())
        .collect();
    assert_eq!(
        fns.len(),
        2,
        "expected assign + the IIFE arrow; got names: {names:?}"
    );
    let assign = find_fn(&fns, "assign");
    assert_eq!(
        assign.complexity, 1,
        "the nested IIFE's `if` must NOT bleed into assign"
    );
    assert!(
        assign.contributors.is_empty(),
        "assign has no decision points of its own; got {:?}",
        assign.contributors
    );
    let arrow = find_fn(&fns, "<arrow>");
    assert_eq!(
        arrow.complexity, 2,
        "the IIFE arrow's own `if` adds one decision point"
    );
    assert_eq!(arrow.contributors.len(), 1);
    assert_eq!(arrow.contributors[0].kind, ContributorKind::IfBranch);
}

#[test]
fn update_expression_computed_operand_iife_is_discovered() {
    // #200 item 4: `counters[(() => { if … })()]++` — the IIFE arrow
    // in the operand's index expression is its own FunctionComplexity;
    // `bump` stays CC 1.
    let fns = extract(UPDATE_COMPUTED_OPERAND_TS, "update-computed-operand.ts");
    let names: Vec<_> = fns
        .iter()
        .map(|f| f.identity.qualified_name.clone())
        .collect();
    assert_eq!(
        fns.len(),
        2,
        "expected bump + the IIFE arrow; got names: {names:?}"
    );
    let bump = find_fn(&fns, "bump");
    assert_eq!(
        bump.complexity, 1,
        "the nested IIFE's `if` must NOT bleed into bump"
    );
    assert!(bump.contributors.is_empty());
    let arrow = find_fn(&fns, "<arrow>");
    assert_eq!(arrow.complexity, 2, "the IIFE arrow's own `if`");
    assert_eq!(arrow.contributors.len(), 1);
    assert_eq!(arrow.contributors[0].kind, ContributorKind::IfBranch);
}

#[test]
fn assignment_static_member_object_iife_is_discovered() {
    // gemini PR #220 review: `(() => { if … })().prop = 1` — the IIFE
    // arrow is the *object* of a STATIC-member assignment LHS. It must
    // be discovered as its own FunctionComplexity (CC 2 from its own
    // `if`); `assignStatic` stays CC 1 (the arrow's `if` must not
    // bleed). Pre-fix this whole LHS was dropped (only the computed
    // case recursed).
    let fns = extract(
        ASSIGNMENT_STATIC_MEMBER_OBJECT_TS,
        "assignment-static-member-object.ts",
    );
    let names: Vec<_> = fns
        .iter()
        .map(|f| f.identity.qualified_name.clone())
        .collect();
    assert_eq!(
        fns.len(),
        2,
        "expected assignStatic + the IIFE arrow; got names: {names:?}"
    );
    let assign = find_fn(&fns, "assignStatic");
    assert_eq!(
        assign.complexity, 1,
        "the static-member-object IIFE's `if` must NOT bleed into assignStatic"
    );
    assert!(assign.contributors.is_empty());
    let arrow = find_fn(&fns, "<arrow>");
    assert_eq!(arrow.complexity, 2, "the IIFE arrow's own `if`");
    assert_eq!(arrow.contributors.len(), 1);
    assert_eq!(arrow.contributors[0].kind, ContributorKind::IfBranch);
}

#[test]
fn update_static_member_object_iife_is_discovered() {
    // gemini PR #220 review: `(() => { if … })().prop++` — the IIFE
    // arrow is the *object* of a STATIC-member UpdateExpression
    // operand. Same contract as the assignment case: arrow is its own
    // CC-2 site; `bumpStatic` stays CC 1.
    let fns = extract(
        UPDATE_STATIC_MEMBER_OBJECT_TS,
        "update-static-member-object.ts",
    );
    let names: Vec<_> = fns
        .iter()
        .map(|f| f.identity.qualified_name.clone())
        .collect();
    assert_eq!(
        fns.len(),
        2,
        "expected bumpStatic + the IIFE arrow; got names: {names:?}"
    );
    let bump = find_fn(&fns, "bumpStatic");
    assert_eq!(
        bump.complexity, 1,
        "the static-member-object IIFE's `if` must NOT bleed into bumpStatic"
    );
    assert!(bump.contributors.is_empty());
    let arrow = find_fn(&fns, "<arrow>");
    assert_eq!(arrow.complexity, 2, "the IIFE arrow's own `if`");
    assert_eq!(arrow.contributors.len(), 1);
    assert_eq!(arrow.contributors[0].kind, ContributorKind::IfBranch);
}

#[test]
fn computed_class_key_iife_is_discovered() {
    // #205 (class side): `class Widget { [(() => "x")()]: number = 0 }`
    // — the IIFE arrow in the computed class-property key is its own
    // FunctionComplexity (CC 1, no decision points), separate from the
    // class's regular method `Widget.regular` (CC 2 from its `if`).
    let fns = extract(COMPUTED_CLASS_KEY_TS, "computed-class-key.ts");
    let names: Vec<_> = fns
        .iter()
        .map(|f| f.identity.qualified_name.clone())
        .collect();
    assert_eq!(
        fns.len(),
        2,
        "expected Widget.regular + the computed-key IIFE arrow; got names: {names:?}"
    );
    let arrow = find_fn(&fns, "<arrow>");
    assert_eq!(
        arrow.complexity, 1,
        "the computed-key IIFE arrow has no decision points"
    );
    assert!(
        arrow.contributors.is_empty(),
        "arrow has no contributors; got {:?}",
        arrow.contributors
    );
    let regular = find_fn(&fns, "Widget.regular");
    assert_eq!(
        regular.complexity, 2,
        "Widget.regular's `if` adds one decision point"
    );
    assert_eq!(regular.contributors.len(), 1);
    assert_eq!(regular.contributors[0].kind, ContributorKind::IfBranch);
}
