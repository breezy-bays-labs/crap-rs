//! Cucumber-rs runner for `tests/features/parity_with_v1.feature`.
//!
//! Wires the crap4ts@1.x parity contract through `parity_helpers` — the
//! same pure parse + classify + diff module `parity_v1.rs` already
//! consumes. The harness adds NO oracle-diff logic of its own (AC of
//! crap-rs#229): scenarios route through `parse_oracle` / `parse_v2` /
//! `diff` and assert against the resulting `ParityReport`.
//!
//! Two scenarios ("scores match within tolerance", "risk labels match")
//! run the real W3.1 corpus: `crap4ts --format json` against
//! `tests/fixtures/crap4ts-v1/`. That run is expensive, so it is done
//! once behind a `OnceLock` and cloned per scenario. The other three
//! scenarios are deterministic classifier checks over synthetic
//! `FnRecord`s — the real corpus is regression-free, so
//! `ThresholdDefaultChange` and the `render()` follow-up block do not
//! fire naturally and must be constructed.
//!
//! Named `*_cucumber` (suffix) so `.config/nextest.toml`'s
//! `binary(/.*_cucumber$/)` filter excludes it from nextest probing.

mod parity_helpers;

use std::sync::OnceLock;

use cucumber::{World, given, then, when, writer};
use parity_helpers::{Class, FnRecord, ParityReport, diff, parse_oracle, parse_v2};

const ORACLE_JSON: &str = include_str!("fixtures/crap4ts-v1-reference.json");

/// Run `crap4ts --format json` against the committed v1.x corpus once,
/// classify it, and memoize. Scenarios 1 and 3 both need the real
/// report; shelling the binary twice would double the corpus analysis
/// for no gain.
fn real_parity() -> &'static ParityReport {
    static REAL: OnceLock<ParityReport> = OnceLock::new();
    REAL.get_or_init(|| {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let src = format!("{manifest}/tests/fixtures/crap4ts-v1/src");
        let coverage = format!("{manifest}/tests/fixtures/crap4ts-v1/coverage-final.json");

        let out = assert_cmd::Command::cargo_bin("crap4ts")
            .expect("crap4ts binary discoverable in workspace")
            .args(["--src", &src, "--coverage", &coverage, "--format", "json"])
            .arg("--no-fail")
            .output()
            .expect("crap4ts executes");
        assert!(
            out.status.success(),
            "crap4ts exited non-zero under --no-fail: stderr=\n{}",
            String::from_utf8_lossy(&out.stderr),
        );

        let oracle = parse_oracle(ORACLE_JSON);
        let v2 = parse_v2(std::str::from_utf8(&out.stdout).expect("crap4ts stdout is utf8"));
        diff(&oracle, &v2)
    })
}

/// Positional `FnRecord` builder for the synthetic-record scenarios —
/// mirrors `parity_v1.rs::rec` so the cucumber layer and the unit layer
/// describe a function the same way.
#[allow(clippy::too_many_arguments)]
fn rec(
    name: &str,
    start_line: i64,
    cc: u32,
    coverage: f64,
    crap: f64,
    risk: &str,
    exceeds: bool,
    contributors: &[&str],
) -> FnRecord {
    FnRecord {
        file: "src/x.ts".to_string(),
        name: name.to_string(),
        start_line,
        cc,
        coverage,
        crap,
        risk: risk.to_string(),
        exceeds,
        contributors: contributors.iter().map(|s| s.to_string()).collect(),
    }
}

/// State for one scenario. `oracle` / `v2` are populated by the
/// synthetic-scenario Givens; the When materializes `report` — either
/// from `diff` over the synthetic records or from the cached real run.
#[derive(Debug, Default, World)]
struct ParityWorld {
    oracle: Vec<FnRecord>,
    v2: Vec<FnRecord>,
    report: Option<ParityReport>,
}

impl ParityWorld {
    fn report(&self) -> &ParityReport {
        self.report
            .as_ref()
            .expect("a When step must produce the parity report first")
    }
}

// ── Given: real-corpus markers ───────────────────────────────────────
// The corpus + reference are committed fixtures; these Givens assert
// they exist so a fixture deletion fails loudly at the spec layer.

#[given("the snapshotted crap4ts@1.x source corpus at `tests/fixtures/crap4ts-v1/`")]
fn given_corpus(_world: &mut ParityWorld) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/crap4ts-v1/src");
    assert!(
        std::path::Path::new(dir).is_dir(),
        "v1.x corpus missing at {dir}"
    );
}

#[given("the captured v1.x reference outputs at `tests/fixtures/crap4ts-v1-reference.json`")]
fn given_reference(_world: &mut ParityWorld) {
    assert!(
        !parse_oracle(ORACLE_JSON).is_empty(),
        "v1.x reference oracle is empty"
    );
}

#[given("the v1.x corpus + reference outputs")]
fn given_corpus_and_reference(world: &mut ParityWorld) {
    given_corpus(world);
    given_reference(world);
}

// ── Given: synthetic records ─────────────────────────────────────────

#[given(
    "a function whose v1.x reference has cyclomatic 4 (contributors: 2× if-branch, 1× ternary)"
)]
fn given_v1_cc4(world: &mut ParityWorld) {
    // The oracle JSON carries a cyclomatic number but no contributor
    // list (see parity_helpers module docs), so the v1 side is built
    // with cc=4 and no contributors — the parenthetical names the
    // conceptual shape, not data the harness can diff.
    world.oracle = vec![rec("dropTernary", 5, 4, 100.0, 4.0, "low", false, &[])];
}

#[given("whose v2 output has cyclomatic 3 (contributors: 2× if-branch only — ternary missed)")]
fn given_v2_cc3(world: &mut ParityWorld) {
    world.v2 = vec![rec(
        "dropTernary",
        5,
        3,
        100.0,
        3.0,
        "low",
        false,
        &["if-branch", "if-branch"],
    )];
}

#[given("a function whose v1.x CRAP score crosses the 12 threshold but stays below 16")]
fn given_borderline(world: &mut ParityWorld) {
    // Same score both sides; only the gate verdict flips (oracle
    // threshold 12 → exceeds, crap4ts@2 threshold 16 → passes).
    world.oracle = vec![rec(
        "borderline",
        20,
        13,
        100.0,
        13.0,
        "moderate",
        true,
        &[],
    )];
    world.v2 = vec![rec(
        "borderline",
        20,
        13,
        100.0,
        13.0,
        "moderate",
        false,
        &[],
    )];
}

#[given("the parity harness has identified a function with score divergence > ε")]
fn given_divergence(world: &mut ParityWorld) {
    // Same CC + coverage, |Δcrap| > CRAP_EPS, no benign explanation →
    // a genuine ScoreRegression.
    world.oracle = vec![rec("regressed", 30, 5, 80.0, 6.0, "acceptable", false, &[])];
    world.v2 = vec![rec(
        "regressed",
        30,
        5,
        80.0,
        9.0,
        "moderate",
        false,
        &["if-branch", "ternary", "ternary"],
    )];
}

// ── When ─────────────────────────────────────────────────────────────

#[when(
    "the parity harness runs `crap4ts --src tests/fixtures/crap4ts-v1/src --coverage <v1-coverage>` and compares"
)]
fn when_run_real_corpus(world: &mut ParityWorld) {
    world.report = Some(real_parity().clone());
}

#[when("the parity harness compares scores function-by-function")]
fn when_compare_scores(world: &mut ParityWorld) {
    world.report = Some(real_parity().clone());
}

#[when("the parity harness reports the divergence")]
fn when_report_divergence(world: &mut ParityWorld) {
    world.report = Some(diff(&world.oracle, &world.v2));
}

#[when("the parity harness compares pass/fail gate outcomes")]
fn when_compare_gate_outcomes(world: &mut ParityWorld) {
    world.report = Some(diff(&world.oracle, &world.v2));
}

#[when("the divergence is NOT explained by threshold-default-change or arrow-function-undercount")]
fn when_unexplained_divergence(world: &mut ParityWorld) {
    world.report = Some(diff(&world.oracle, &world.v2));
}

// ── Then: real-corpus tolerance gate ─────────────────────────────────

#[then("95%+ of functions match cyclomatic complexity within ±0 (exact match)")]
fn then_exact_cc_rate(world: &mut ParityWorld) {
    let report = world.report();
    assert!(
        report.exact_cc_rate() >= 0.95,
        "exact-CC rate {:.3} fell below 0.95:\n{}",
        report.exact_cc_rate(),
        report.render(),
    );
}

#[then("every matched function's CRAP score is within tolerance (no unexplained regressions)")]
fn then_scores_within_tolerance(world: &mut ParityWorld) {
    // Score parity is the parity contract. Risk labels are derived
    // from the score by `classify_risk` and verified by that
    // function's own unit tests; they are not pinned across the v1.x
    // boundary so a tier recalibration does not surface here.
    let report = world.report();
    assert!(
        report.regressions().is_empty(),
        "{} score regression(s):\n{}",
        report.regressions().len(),
        report.render(),
    );
}

#[then("any divergence is reported per-function in the harness output")]
fn then_divergence_reported_per_function(world: &mut ParityWorld) {
    let rendered = world.report().render();
    assert!(
        rendered.contains("parity:") && rendered.contains("buckets:"),
        "render() should carry the per-function parity summary; got:\n{rendered}",
    );
}

#[then("risk-tier boundaries may be recalibrated across versions without tripping the gate")]
fn then_risk_tier_recalibration_does_not_trip_gate(world: &mut ParityWorld) {
    // Risk-tier boundaries live in `classify_risk`. They are allowed
    // to move across versions (e.g. #272 aligned them with the
    // threshold presets). Because the parity gate ranges over scores
    // (not derived labels), a tier-only change does not flip
    // `gate_passes`.
    let report = world.report();
    assert!(
        report.gate_passes(),
        "score parity should hold across a risk-tier recalibration:\n{}",
        report.render(),
    );
}

// ── Then: synthetic-record classifier checks ─────────────────────────

#[then("the report names the function")]
fn then_report_names_function(world: &mut ParityWorld) {
    let rendered = world.report().render();
    assert!(
        rendered.contains("dropTernary"),
        "render() should name the divergent function; got:\n{rendered}",
    );
}

#[then("the report shows v2 contributors: `2× if-branch`")]
fn then_report_shows_v2_contributors(world: &mut ParityWorld) {
    let rendered = world.report().render();
    assert!(
        rendered.contains("2× if-branch"),
        "render() should show v2's contributor breakdown; got:\n{rendered}",
    );
}

#[then("v1.x reports the function as `failing` (score > 12)")]
fn then_v1_failing(world: &mut ParityWorld) {
    let d = &world.report().divergences[0];
    assert!(
        d.v1_crap > 12.0 && world.oracle[0].exceeds,
        "v1 should exceed"
    );
}

#[then("v2 reports the function as `passing` (score < 16)")]
fn then_v2_passing(world: &mut ParityWorld) {
    let d = &world.report().divergences[0];
    assert!(d.v2_crap < 16.0 && !world.v2[0].exceeds, "v2 should pass");
}

#[then("the harness flags this as `threshold-default-change`, NOT as `score-regression`")]
fn then_flagged_threshold_default_change(world: &mut ParityWorld) {
    let report = world.report();
    assert_eq!(
        report.divergences[0].class,
        Class::ThresholdDefaultChange,
        "same score, gate verdict flipped → threshold-default-change",
    );
    assert!(
        report.gate_passes(),
        "threshold-default-change must NOT fail the parity gate",
    );
    assert!(report.render().contains("threshold-default-change"));
}

#[then("the harness output recommends filing a follow-up issue under epic #173")]
fn then_recommends_followup(world: &mut ParityWorld) {
    let report = world.report();
    assert_eq!(
        report.divergences[0].class,
        Class::ScoreRegression,
        "an unexplained divergence must classify as a regression",
    );
    let rendered = report.render();
    assert!(
        rendered.contains("file a follow-up") && rendered.contains("epic #173"),
        "render() should recommend a tracked follow-up; got:\n{rendered}",
    );
}

#[then("the recommended issue body includes the function name + v2 contributors")]
fn then_followup_includes_name_and_contributors(world: &mut ParityWorld) {
    let rendered = world.report().render();
    assert!(
        rendered.contains("regressed"),
        "follow-up should name the function; got:\n{rendered}",
    );
    assert!(
        rendered.contains("1× if-branch + 2× ternary"),
        "follow-up should include v2's contributor breakdown; got:\n{rendered}",
    );
}

// ── Runner ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // `@wired`-only filter (AGENTS.md rule 5). `with_default_cli()`
    // skips argv parsing so the `--skip <name>` libtest args that
    // `cargo mutants --package crap4ts` injects into every crap4ts test
    // binary do not abort cucumber's strict clap CLI (the crap-rs#224
    // gate-zeroing class — see `cyclomatic_walker_cucumber.rs`).
    ParityWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .with_default_cli()
        .filter_run_and_exit("tests/features/parity_with_v1.feature", |_, _, sc| {
            sc.tags.iter().any(|t| t == "wired")
        })
        .await;
}
