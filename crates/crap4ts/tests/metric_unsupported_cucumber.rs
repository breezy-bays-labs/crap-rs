//! Cucumber-rs runner for `tests/features/metric_unsupported.feature`.
//!
//! Wires the user-facing `--metric` error UX by shelling the `crap4ts`
//! binary (`assert_cmd::cargo_bin`) — these scenarios are about exit
//! codes and exact stderr strings, so the binary IS the unit under
//! test (mirrors `metric_unsupported_smoke.rs`).
//!
//! Scenario 3 ("crap4rs's --metric cognitive default continues to
//! work") is deliberately left `@unwired`: a crap4ts cucumber harness
//! shelling the **crap4rs** binary re-triggers the crap-rs#224
//! mutants-baseline bug class — `CARGO_BIN_EXE_crap4rs` is unset under
//! `cargo mutants --package crap4ts`, so the unmutated baseline would
//! panic and zero the per-merge walker-mutants gate. That cross-adapter
//! contract stays pinned by the already-mutants-skipped
//! `metric_unsupported_smoke.rs::crap4rs_no_flag_default_cognitive_still_works`.
//! Wiring the remaining feature is tracked at crap-rs#229 (pre-GA).
//!
//! Named `*_cucumber` (suffix) so `.config/nextest.toml`'s
//! `binary(/.*_cucumber$/)` filter excludes it from nextest probing —
//! same convention as crap4rs's `json_reporter_cucumber`.

use std::path::PathBuf;
use std::process::Output;

use assert_cmd::Command;
use cucumber::{World, gherkin::Step, given, then, when, writer};
use tempfile::TempDir;

const FIXTURE_TEMPLATE: &str = include_str!("fixtures/istanbul-jest/coverage-final.json");

/// Build a canonicalized tempdir with the W1.1 jest fixture TS files
/// and a `coverage-final.json` whose `{SRC_ROOT}` is substituted with
/// the canonical path (macOS routes `/tmp` → `/private/tmp`;
/// forward-slash normalize so the JSON parses on any platform).
/// Mirrors `metric_unsupported_smoke.rs::build_jest_fixture` — copy-3
/// of the per-file tempdir helper is still under the threshold where a
/// shared `crap4ts-fixtures` support crate would pay off.
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

/// State for one scenario. The fixture guard is held so the tempdir
/// survives until the scenario ends; `output` is the captured `crap4ts`
/// process result the Then steps assert against.
#[derive(Debug, Default, World)]
struct MetricWorld {
    fixture: Option<(TempDir, PathBuf)>,
    output: Option<Output>,
}

impl MetricWorld {
    fn ensure_fixture(&mut self) -> PathBuf {
        if self.fixture.is_none() {
            self.fixture = Some(build_jest_fixture());
        }
        self.fixture.as_ref().unwrap().1.clone()
    }

    /// Run `crap4ts` against the jest fixture with the given metric.
    /// `--no-fail` so threshold violations in the fixture don't mask
    /// the exit-code contract under test.
    fn run_crap4ts(&mut self, metric: &str) {
        let root = self.ensure_fixture();
        let coverage = root.join("coverage-final.json");
        let out = Command::cargo_bin("crap4ts")
            .expect("crap4ts binary discoverable in workspace")
            .arg("--coverage")
            .arg(&coverage)
            .arg("--src")
            .arg(&root)
            .args(["--metric", metric, "--no-fail"])
            .output()
            .expect("crap4ts executes");
        self.output = Some(out);
    }

    fn out(&self) -> &Output {
        self.output
            .as_ref()
            .expect("a When step must run crap4ts first")
    }

    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.out().stderr).into_owned()
    }
}

// ── Given ────────────────────────────────────────────────────────────

#[given("a TypeScript source tree under `src/`")]
fn given_ts_tree(world: &mut MetricWorld) {
    world.ensure_fixture();
}

#[given("a valid Istanbul `coverage-final.json`")]
fn given_istanbul_coverage(world: &mut MetricWorld) {
    let root = world.ensure_fixture();
    assert!(
        root.join("coverage-final.json").is_file(),
        "fixture coverage-final.json should exist"
    );
}

// ── When ─────────────────────────────────────────────────────────────

#[when(regex = r"^the operator runs `crap4ts .*--metric (\w+)`$")]
fn when_run_with_metric(world: &mut MetricWorld, metric: String) {
    world.run_crap4ts(&metric);
}

#[when("the binary renders the MetricNotSupported error for metric `Cognitive`")]
fn when_render_metric_not_supported(world: &mut MetricWorld) {
    world.run_crap4ts("cognitive");
}

// ── Then ─────────────────────────────────────────────────────────────

#[then(regex = r"^`crap4ts` exits with status (\d+)$")]
fn then_exit_status(world: &mut MetricWorld, code: i32) {
    assert_eq!(
        world.out().status.code(),
        Some(code),
        "expected exit {code}; stderr=\n{}",
        world.stderr()
    );
}

#[then("the user-facing error reads exactly:")]
fn then_error_reads_exactly(world: &mut MetricWorld, step: &Step) {
    let expected = step.docstring().expect("expected-error docstring").trim();
    let stderr = world.stderr();
    assert!(
        stderr.contains(expected),
        "stderr missing exact MetricNotSupported message.\nExpected to contain:\n{expected}\nActual stderr:\n{stderr}"
    );
}

#[then("`crap4ts` produces a complete CRAP scorecard")]
fn then_complete_scorecard(world: &mut MetricWorld) {
    let out = world.out();
    assert!(
        out.status.success(),
        "expected success exit; stderr=\n{}",
        world.stderr()
    );
    assert!(
        !out.stdout.is_empty(),
        "expected a scorecard on stdout, got empty output"
    );
}

#[then("no MetricNotSupported error is emitted")]
fn then_no_metric_not_supported(world: &mut MetricWorld) {
    let stderr = world.stderr();
    assert!(
        !stderr.contains("is not yet supported"),
        "unexpected MetricNotSupported signal in stderr:\n{stderr}"
    );
}

#[then(
    "the rendered metric name in the user message is `cognitive` (lowercase, matching CLI input)"
)]
fn then_display_lowercase(world: &mut MetricWorld) {
    let stderr = world.stderr();
    assert!(
        stderr.contains("`cognitive`"),
        "expected Display-format lowercase `cognitive` in the message; stderr=\n{stderr}"
    );
}

#[then("the rendered metric name is NOT `Cognitive` (PascalCase, Debug format)")]
fn then_not_debug_pascalcase(world: &mut MetricWorld) {
    let stderr = world.stderr();
    assert!(
        !stderr.contains("`Cognitive`"),
        "message leaked Debug-format PascalCase `Cognitive`; stderr=\n{stderr}"
    );
}

#[then("the error originates from clap's argument validation, NOT from MetricNotSupported")]
fn then_clap_not_metric_not_supported(world: &mut MetricWorld) {
    let stderr = world.stderr();
    assert!(
        !stderr.contains("is not yet supported"),
        "expected a clap validation error, but got the MetricNotSupported message:\n{stderr}"
    );
    assert!(
        stderr.contains("invalid value") || stderr.contains("possible values"),
        "expected clap's invalid-value phrasing; stderr=\n{stderr}"
    );
}

#[then("the error names the valid `--metric` values")]
fn then_names_valid_metrics(world: &mut MetricWorld) {
    let stderr = world.stderr();
    assert!(
        stderr.contains("cyclomatic") && stderr.contains("cognitive"),
        "clap error should list the valid --metric values (cyclomatic, cognitive); stderr=\n{stderr}"
    );
}

// ── Runner ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // `@wired`-only filter (AGENTS.md rule 5). Scenario 3 (crap4rs
    // sanity) stays `@unwired` — see the module doc for the crap-rs#224
    // rationale — so this filter skips it rather than the harness
    // shelling crap4rs.
    //
    // `with_default_cli()` skips argv parsing so the `--skip <name>`
    // libtest args `cargo mutants --package crap4ts` injects into every
    // crap4ts test binary do not abort cucumber's strict clap CLI (the
    // crap-rs#224 gate-zeroing class — see the matching note in
    // `cyclomatic_walker_cucumber.rs`).
    MetricWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .with_default_cli()
        .filter_run_and_exit("tests/features/metric_unsupported.feature", |_, _, sc| {
            sc.tags.iter().any(|t| t == "wired")
        })
        .await;
}
