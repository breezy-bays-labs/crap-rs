//! Cucumber-rs runner for `tests/features/arrow_function_coverage.feature`.
//!
//! Per-function line coverage is produced by the line-range join in
//! `crap_core::core::analyze` (the walker's function spans matched
//! against the Istanbul `LineCoverage` records) — it is not visible in
//! `IstanbulCoverage::parse`'s `ParseOutput` alone. So this harness
//! shells the `crap4ts` binary with `--format json` and reads the
//! per-function `coverage_percent` out of the envelope, mirroring
//! `parity_v1.rs` / `metric_unsupported_cucumber.rs`.
//!
//! Scenario 1 ("An invoked arrow function has matching coverage") asserts
//! a never-invoked single-line arrow reports 0.0 function coverage. It is
//! `@wired` post-crap-rs#252 — the per-function rollup now reads MIN-
//! aggregated `LineCoverage` records (one per source line, regardless of
//! how many statements Istanbul originally emitted), so the `const`
//! declaration's module-load hit no longer masks the arrow body's
//! zero-hit signal.
//!
//! Named `*_cucumber` (suffix) so `.config/nextest.toml`'s
//! `binary(/.*_cucumber$/)` filter excludes it from nextest probing.

use std::path::PathBuf;

use cucumber::{World, given, then, when, writer};
use serde::Deserialize;
use tempfile::TempDir;

const JEST_FIXTURE: &str = include_str!("fixtures/istanbul-jest/coverage-final.json");

/// Minimal projection of the `crap4ts --format json` envelope — just
/// the per-function identity + line coverage these scenarios assert on.
#[derive(Debug, Deserialize)]
struct Envelope {
    result: EnvelopeResult,
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
}

#[derive(Debug, Deserialize)]
struct EnvelopeIdentity {
    file_path: String,
    qualified_name: String,
}

/// Build a canonicalized tempdir with the five W1.1 jest-fixture TS
/// source files and the `coverage-final.json` whose `{SRC_ROOT}` is
/// substituted with the canonical path. Mirrors
/// `metric_unsupported_cucumber.rs::build_jest_fixture`.
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

    let payload = JEST_FIXTURE.replace(
        "{SRC_ROOT}",
        &canonical.to_string_lossy().replace('\\', "/"),
    );
    std::fs::write(canonical.join("coverage-final.json"), payload).expect("write coverage-final");

    (tmp, canonical)
}

/// State for one scenario. The fixture guard is held so the tempdir
/// survives until the scenario ends; `functions` is the parsed envelope
/// the Then steps assert against.
#[derive(Debug, Default, World)]
struct ArrowWorld {
    fixture: Option<(TempDir, PathBuf)>,
    functions: Vec<EnvelopeFn>,
}

impl ArrowWorld {
    fn ensure_fixture(&mut self) -> PathBuf {
        if self.fixture.is_none() {
            self.fixture = Some(build_jest_fixture());
        }
        self.fixture.as_ref().unwrap().1.clone()
    }

    /// Line coverage for the function `name` in `file`, panicking with
    /// the discovered set on a miss so a walker-naming drift is legible.
    fn coverage_of(&self, file: &str, name: &str) -> f64 {
        self.functions
            .iter()
            .find(|f| {
                f.scored.identity.file_path == file && f.scored.identity.qualified_name == name
            })
            .map(|f| f.scored.coverage_percent)
            .unwrap_or_else(|| {
                panic!(
                    "function `{name}` in `{file}` not in the report; discovered: {:?}",
                    self.functions
                        .iter()
                        .map(|f| {
                            (
                                f.scored.identity.file_path.as_str(),
                                f.scored.identity.qualified_name.as_str(),
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
    }
}

// ── Given ────────────────────────────────────────────────────────────

#[given(regex = r"^a TypeScript source file `src/[\w.]+` containing:$")]
fn given_ts_source_file(world: &mut ArrowWorld) {
    // The committed `tests/fixtures/ts-fixtures/*` files are the source
    // of truth — the jest `coverage-final.json` is calibrated to their
    // exact line numbers, so the scenario docstring is narration only.
    world.ensure_fixture();
}

#[given(regex = r"^a jest-emitted `coverage-final\.json` recording .+$")]
fn given_jest_coverage(world: &mut ArrowWorld) {
    world.ensure_fixture();
}

// ── When ─────────────────────────────────────────────────────────────

#[when("the operator runs `crap4ts --coverage coverage-final.json --src src`")]
fn when_run_crap4ts(world: &mut ArrowWorld) {
    let root = world.ensure_fixture();
    let coverage = root.join("coverage-final.json");
    let out = assert_cmd::Command::cargo_bin("crap4ts")
        .expect("crap4ts binary discoverable in workspace")
        .arg("--coverage")
        .arg(&coverage)
        .arg("--src")
        .arg(&root)
        .args(["--format", "json", "--no-fail"])
        .output()
        .expect("crap4ts executes");
    assert!(
        out.status.success(),
        "crap4ts exited non-zero under --no-fail: stderr=\n{}",
        String::from_utf8_lossy(&out.stderr),
    );
    let envelope: Envelope =
        serde_json::from_slice(&out.stdout).expect("crap4ts --format json emits a valid envelope");
    world.functions = envelope.result.functions;
}

// ── Then ─────────────────────────────────────────────────────────────

#[then(regex = r"^the report does NOT show `(\w+)` as (\d+\.\d+) \(would be silent undercount\)$")]
fn then_not_named_function_coverage(world: &mut ArrowWorld, name: String, forbidden: f64) {
    // Negative assertion guarding against the inverse of #252: if a
    // future regression swapped square's and cube's coverage values
    // (or the matcher's MIN became MAX, etc.), `square` would silently
    // report 0.0 and the scenario's positive Then would still pass for
    // `cube`. This guard explicitly fails if `square` slipped to 0.0.
    let actual = world
        .functions
        .iter()
        .find(|f| f.scored.identity.qualified_name == name)
        .map(|f| f.scored.coverage_percent)
        .unwrap_or_else(|| panic!("function `{name}` not found in the report"));
    assert!(
        (actual - forbidden).abs() > 1e-6,
        "`{name}` line coverage must NOT be {forbidden} (silent-undercount guard); got {actual}",
    );
}

#[then(regex = r"^the report shows function `(\w+)` with line coverage (\d+\.\d+)$")]
fn then_named_function_coverage(world: &mut ArrowWorld, name: String, expected: f64) {
    // The named functions in these scenarios are unique across the
    // fixture set, so file disambiguation is unnecessary — find by name.
    let actual = world
        .functions
        .iter()
        .find(|f| f.scored.identity.qualified_name == name)
        .map(|f| f.scored.coverage_percent)
        .unwrap_or_else(|| panic!("function `{name}` not found in the report"));
    assert!(
        (actual - expected).abs() < 1e-6,
        "`{name}` line coverage: expected {expected}, got {actual}",
    );
}

#[then("the report shows the useCallback arrow with line coverage 100.0")]
fn then_use_callback_arrow_covered(world: &mut ArrowWorld) {
    // The walker names anonymous arrows `<arrow>`; the useCallback arrow
    // in Button.tsx is the file's only one.
    let cov = world.coverage_of("Button.tsx", "<arrow>");
    assert!(
        (cov - 100.0).abs() < 1e-6,
        "the useCallback arrow should be 100% covered; got {cov}",
    );
}

#[then("the inner arrow's line coverage in the report is 100.0")]
fn then_inner_arrow_covered(world: &mut ArrowWorld) {
    let cov = world.coverage_of("map.ts", "<arrow>");
    assert!(
        (cov - 100.0).abs() < 1e-6,
        "the inner map(arrow) should be 100% covered; got {cov}",
    );
}

#[then("`increment`'s CRAP score is computed using the arrow's coverage value")]
fn then_increment_uses_arrow_coverage(world: &mut ArrowWorld) {
    // `increment`'s body is the `return xs.map(arrow)` line, so its
    // function coverage tracks the inner arrow's — both resolve to the
    // same covered line.
    let increment = world.coverage_of("map.ts", "increment");
    let arrow = world.coverage_of("map.ts", "<arrow>");
    assert!(
        (increment - arrow).abs() < 1e-6,
        "`increment` coverage ({increment}) should reflect the inner arrow's ({arrow})",
    );
}

// ── Runner ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // `@wired`-only filter (AGENTS.md rule 5).
    // `with_default_cli()` skips argv parsing so the `--skip` libtest
    // args `cargo mutants --package crap4ts` injects do not abort
    // cucumber's strict clap CLI (the crap-rs#224 gate-zeroing class).
    ArrowWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .with_default_cli()
        .filter_run_and_exit(
            "tests/features/arrow_function_coverage.feature",
            |_, _, sc| sc.tags.iter().any(|t| t == "wired"),
        )
        .await;
}
