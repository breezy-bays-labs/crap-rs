//! Cucumber-rs runner for `tests/features/cyclomatic_walker.feature`.
//!
//! Wires the cyclomatic-walker BDD contract directly against
//! `OxcWalker` (the same library entry point `walker_smoke.rs` unit
//! tests), not the binary. The walker is pure (source in →
//! `Vec<FunctionComplexity>` out), so the spec is executable without a
//! coverage file or a tempdir: the feature's
//! `crap4ts --coverage cov.json --src .` line is the *spec's* narration
//! of "analyze this source"; the executable form is a direct
//! `extract(...)` call. This keeps the BDD layer inside the public
//! adapter contract and lets the spec stop drifting from the walker.
//!
//! The risk-classification scenario carries no source — it asserts the
//! metric-invariant CRAP formula, so it routes through
//! `crap_core::domain::crap::compute_crap` instead of the walker.
//!
//! Scenario-outline constructs (`if (cond) { … }`, `a ?? fallback`, …)
//! are synthesized into minimal valid TS that yields exactly the one
//! decision point under test — the canned bodies mirror the shapes
//! `walker_smoke.rs` fixtures already pin, so the two layers agree.
//!
//! Named `*_cucumber` (suffix, not the `cucumber_*` prefix the plan
//! prose used) because `.config/nextest.toml` excludes
//! `binary(/.*_cucumber$/)` — the suffix is the load-bearing convention
//! shared with crap4rs's `json_reporter_cucumber`.

use crap_core::domain::crap::compute_crap;
use crap_core::domain::types::{ComplexityMetric, CrapScore, FunctionComplexity, RiskLevel};
use crap_core::ports::ComplexityPort;
use crap4ts::adapters::walker::OxcWalker;
use cucumber::{World, gherkin::Step, given, then, when, writer};

/// State threaded through one scenario. Either `source` (→ run the
/// walker) or `synth` (→ run the CRAP formula) is populated by a Given;
/// the When materializes `fns` or `crap`.
#[derive(Debug, Default, World)]
struct WalkerWorld {
    source: Option<String>,
    is_jsx: bool,
    fns: Vec<FunctionComplexity>,
    synth: Option<(u32, f64)>,
    crap: Option<CrapScore>,
}

impl WalkerWorld {
    /// The single discovered function. Outline + JSX + compound
    /// scenarios each define exactly one top-level function, so the
    /// "the function's …" steps resolve unambiguously.
    fn only_fn(&self) -> &FunctionComplexity {
        assert_eq!(
            self.fns.len(),
            1,
            "expected exactly one function in the source; got {:?}",
            self.fns
                .iter()
                .map(|f| &f.identity.qualified_name)
                .collect::<Vec<_>>()
        );
        &self.fns[0]
    }

    fn find_fn(&self, name: &str) -> &FunctionComplexity {
        self.fns
            .iter()
            .find(|f| f.identity.qualified_name == name)
            .unwrap_or_else(|| {
                panic!(
                    "function `{name}` not discovered; got {:?}",
                    self.fns
                        .iter()
                        .map(|f| &f.identity.qualified_name)
                        .collect::<Vec<_>>()
                )
            })
    }

    fn count_kind(f: &FunctionComplexity, kind: &str) -> usize {
        f.contributors
            .iter()
            .filter(|c| c.kind.as_wire_str() == kind)
            .count()
    }
}

/// Strip backticks + collapse whitespace so the outline's
/// `` `if (cond) { … }` `` cell (which the step wraps in another pair
/// of backticks) normalizes to a stable match key.
fn normalize_construct(raw: &str) -> String {
    raw.replace('`', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Map an outline construct to minimal valid TS containing exactly the
/// one decision point under test. Bodies are deliberately tiny (no
/// stray branches) and reference undeclared helpers — oxc parses without
/// type-checking, and the walker only counts decision points. The
/// shapes mirror `walker_smoke.rs` fixtures so both layers agree on the
/// count.
fn synth_source(construct: &str) -> &'static str {
    match normalize_construct(construct).as_str() {
        "if (cond) { … }" => {
            "function f(cond: boolean): number { if (cond) { return 1; } return 0; }"
        }
        "for (let i = 0; i < n; i++) { … }" => {
            "function f(n: number): void { for (let i = 0; i < n; i++) { void i; } }"
        }
        "for (const x of xs) { … }" => {
            "function f(xs: number[]): void { for (const x of xs) { void x; } }"
        }
        "for (const k in obj) { … }" => {
            "function f(obj: Record<string, number>): void { for (const k in obj) { void k; } }"
        }
        "while (cond) { … }" => {
            "function f(cond: boolean): void { while (cond) { cond = step(); } }"
        }
        "do { … } while (cond)" => {
            "function f(cond: boolean): void { do { cond = step(); } while (cond); }"
        }
        // One `case`, plus a `default:` that is NOT a decision point —
        // matches `walker_smoke.rs::switch_case_contributes_one_case_branch`.
        "switch (x) { case 1: … }" => {
            "function f(x: number): string { switch (x) { case 1: return \"one\"; default: return \"other\"; } }"
        }
        "a && b" => "function f(a: any, b: any): any { return a && b; }",
        "a || b" => "function f(a: any, b: any): any { return a || b; }",
        "cond ? a : b" => "function f(cond: boolean, a: any, b: any): any { return cond ? a : b; }",
        "obj?.field" => "function f(obj: any): any { return obj?.field; }",
        "obj?.method()" => "function f(obj: any): any { return obj?.method(); }",
        "a ?? fallback" => "function f(a: any, fallback: any): any { return a ?? fallback; }",
        "try { … } catch (e) { … }" => {
            "function f(): void { try { risky(); } catch (e) { handle(e); } }"
        }
        other => panic!("no synthesized source for construct {other:?}"),
    }
}

// ── Given ────────────────────────────────────────────────────────────

/// cucumber-rs prefixes a docstring with a leading `\n` (the newline
/// after the opening `"""`). Strip exactly one so line 1 of the spec's
/// code block is line 1 to the walker — otherwise every reported
/// contributor line is off by one versus the feature author's intent
/// (e.g. the `if-branch at line 2` assertion).
fn docstring_source(step: &Step) -> String {
    let raw = step.docstring().expect("scenario docstring");
    raw.strip_prefix('\n').unwrap_or(raw).to_string()
}

#[given("a TypeScript source file containing:")]
fn given_ts_source(world: &mut WalkerWorld, step: &Step) {
    world.source = Some(docstring_source(step));
    world.is_jsx = false;
}

#[given("a TypeScript JSX source file containing:")]
fn given_tsx_source(world: &mut WalkerWorld, step: &Step) {
    world.source = Some(docstring_source(step));
    world.is_jsx = true;
}

#[given(regex = r"^a TypeScript source file containing the construct (.+)$")]
fn given_construct(world: &mut WalkerWorld, construct: String) {
    world.source = Some(synth_source(&construct).to_string());
    world.is_jsx = false;
}

#[given(regex = r"^a TypeScript function with cyclomatic complexity (\d+) and coverage (\d+)%$")]
fn given_synthetic_function(world: &mut WalkerWorld, complexity: u32, coverage: f64) {
    world.synth = Some((complexity, coverage));
}

// ── When ─────────────────────────────────────────────────────────────

#[when("the oxc walker analyzes the source")]
fn when_analyzed(world: &mut WalkerWorld) {
    if let Some((complexity, coverage)) = world.synth {
        world.crap =
            Some(compute_crap(complexity, coverage).expect("compute_crap on valid inputs"));
        return;
    }
    let source = world.source.as_ref().expect("source set by a Given step");
    let file = if world.is_jsx {
        "snippet.tsx"
    } else {
        "snippet.ts"
    };
    world.fns = OxcWalker::new()
        .extract(source, file, ComplexityMetric::Cyclomatic)
        .unwrap_or_else(|e| panic!("walker extract failed for {file}: {e}"));
}

// ── Then: per-named-function ─────────────────────────────────────────

#[then(regex = r"^the report includes function `(\w+)` with cyclomatic complexity (\d+)$")]
fn then_named_complexity(world: &mut WalkerWorld, name: String, expected: u32) {
    let f = world.find_fn(&name);
    assert_eq!(
        f.complexity, expected,
        "`{name}` cyclomatic complexity: expected {expected}, got {} (contributors: {:?})",
        f.complexity, f.contributors
    );
    assert_eq!(f.metric, ComplexityMetric::Cyclomatic);
}

#[then(regex = r"^no contributors are emitted for `(\w+)`$")]
fn then_no_contributors(world: &mut WalkerWorld, name: String) {
    let f = world.find_fn(&name);
    assert!(
        f.contributors.is_empty(),
        "expected no contributors for `{name}`, got {:?}",
        f.contributors
    );
}

#[then(regex = r"^the contributors include one `([a-z-]+)` at line (\d+)$")]
fn then_one_kind_at_line(world: &mut WalkerWorld, kind: String, line: usize) {
    let f = world.only_fn();
    let matching: Vec<_> = f
        .contributors
        .iter()
        .filter(|c| c.kind.as_wire_str() == kind)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one `{kind}` contributor, got {:?}",
        f.contributors
    );
    assert_eq!(
        matching[0].line, line,
        "`{kind}` should be at line {line}, got {}",
        matching[0].line
    );
}

#[then(regex = r"^`(\w+)`'s contributors include one `([a-z-]+)` entry$")]
fn then_named_one_kind(world: &mut WalkerWorld, name: String, kind: String) {
    let f = world.find_fn(&name);
    assert_eq!(
        WalkerWorld::count_kind(f, &kind),
        1,
        "`{name}` should have exactly one `{kind}` contributor, got {:?}",
        f.contributors
    );
}

// ── Then: single-function ────────────────────────────────────────────

#[then(regex = r"^the function's cyclomatic complexity is `?(\d+)`?$")]
fn then_single_complexity(world: &mut WalkerWorld, expected: u32) {
    let f = world.only_fn();
    assert_eq!(
        f.complexity, expected,
        "cyclomatic complexity: expected {expected}, got {} (contributors: {:?})",
        f.complexity, f.contributors
    );
}

#[then(regex = r"^the contributors include exactly (one|two|three) `([a-z-]+)` (?:entry|entries)$")]
fn then_exact_count(world: &mut WalkerWorld, count_word: String, kind: String) {
    let expected = match count_word.as_str() {
        "one" => 1,
        "two" => 2,
        "three" => 3,
        other => panic!("unhandled count word {other:?}"),
    };
    let f = world.only_fn();
    assert_eq!(
        WalkerWorld::count_kind(f, &kind),
        expected,
        "expected {expected} `{kind}` contributor(s), got {:?}",
        f.contributors
    );
}

#[then(regex = r"^the contributors list contains exactly one entry of kind `([a-z-]+)`$")]
fn then_exactly_one_of_kind(world: &mut WalkerWorld, kind: String) {
    let f = world.only_fn();
    assert_eq!(
        WalkerWorld::count_kind(f, &kind),
        1,
        "expected exactly one `{kind}` contributor, got {:?}",
        f.contributors
    );
}

#[then("the JSX conditional is counted via the existing `logical-operator` contributor")]
fn then_jsx_via_logical(world: &mut WalkerWorld) {
    let f = world.only_fn();
    assert!(
        WalkerWorld::count_kind(f, "logical-operator") >= 1,
        "expected a logical-operator contributor for the JSX `&&`, got {:?}",
        f.contributors
    );
}

// ── Then: CRAP formula (metric-invariant risk) ───────────────────────

#[then(regex = r"^the function's CRAP score is (\d+\.\d+)$")]
fn then_crap_score(world: &mut WalkerWorld, expected: f64) {
    let crap = world.crap.as_ref().expect("crap computed by the When step");
    assert!(
        (crap.value - expected).abs() < 1e-9,
        "CRAP score: expected {expected}, got {}",
        crap.value
    );
}

#[then(regex = r"^the function's risk classification is `([a-z]+)`$")]
fn then_risk(world: &mut WalkerWorld, expected: String) {
    let crap = world.crap.as_ref().expect("crap computed by the When step");
    let actual = match crap.risk_level {
        RiskLevel::Low => "low",
        RiskLevel::Acceptable => "acceptable",
        RiskLevel::Moderate => "moderate",
        RiskLevel::High => "high",
    };
    assert_eq!(actual, expected, "risk classification");
}

// ── Runner ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // `@wired`-only filter per AGENTS.md rule 5: scenarios still
    // `@unwired` (none in this file once W3.3 migrates them, but the
    // filter is the durable contract) are skipped, not failed.
    // `Libtest::or_basic()` emits libtest-JSON under nextest probing and
    // the human writer under plain `cargo test`. `filter_run_and_exit`
    // (not `run`) gives a non-zero exit on scenario failure.
    //
    // `with_default_cli()` skips argv parsing. `cargo mutants --package
    // crap4ts` (the per-merge walker-mutants gate) runs `cargo test --
    // --skip <name>` and those trailing libtest args reach EVERY crap4ts
    // test binary. cucumber's strict clap CLI rejects `--skip`, which
    // aborts the UNMUTATED baseline (`cargo test failed in an unmutated
    // tree`) and zeroes the gate — the crap-rs#224 gate-zeroing class.
    // A default `cli::Opts` makes the harness ignore those passthrough
    // args entirely, keeping the gate live with zero mutants-config
    // growth. (No libtest formatting args are lost: this binary is
    // excluded from nextest probing by the `_cucumber$` filter and runs
    // under plain `cargo test` everywhere else, where no libtest args
    // are passed.)
    WalkerWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .with_default_cli()
        .filter_run_and_exit("tests/features/cyclomatic_walker.feature", |_, _, sc| {
            sc.tags.iter().any(|t| t == "wired")
        })
        .await;
}
