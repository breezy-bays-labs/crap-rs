//! Cucumber-rs runner for `@summary`-tagged scenarios in
//! `tests/features/cli_ergonomics.feature` (issue #131).
//!
//! Most scenarios in `cli_ergonomics.feature` are spec-only — they
//! describe behaviour validated indirectly via the `cli_*_integration.rs`
//! suite. The `@summary` block is wired here so the AC for #131
//! (one-line CLI verdict) executes through the same .feature file the
//! spec lives in. Other scenarios in the file are not wired; the
//! harness uses `filter_run_and_exit` to load only `@summary` tagged
//! scenarios. New BDD-driven flags can adopt their own tags and
//! extend this harness (or land siblings) without forcing migration of
//! the unwired specs.
//!
//! Each scenario sets up a tempdir with a small synthetic LCOV +
//! `src/lib.rs` (mirrors `cli_no_fail_integration.rs`'s pattern) and
//! invokes the binary via `CARGO_BIN_EXE_crap4rs`. The harness does
//! NOT depend on the workspace's self-fixture LCOV, so paths in
//! scenario commands stay relative to the tempdir cwd.
//!
//! The two synthetic fixtures cover the two pass/fail invariants needed
//! by the seven `@summary` scenarios:
//! - `WITHIN_*` — three trivial covered fns; every CRAP ≤ 2, so any
//!   reasonable threshold passes.
//! - `EXCEEDS_*` — three branchy uncovered fns; CRAPs roughly 20+, so
//!   `--threshold 5` deliberately trips the gate.

use std::path::{Path, PathBuf};
use std::process::Output;

use cucumber::{World, given, then, when, writer};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

const WITHIN_SRC: &str = "\
pub fn one() -> i32 { 1 }
pub fn two() -> i32 { 2 }
pub fn three() -> i32 { 3 }
";

const WITHIN_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
DA:3,1
end_of_record
";

const EXCEEDS_SRC: &str = "\
pub fn branchy_a(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn branchy_b(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn branchy_c(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
";

const EXCEEDS_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,0
DA:3,0
end_of_record
";

#[derive(Debug, Default, World)]
struct CliWorld {
    project_dir: Option<PathBuf>,
    /// Held during the scenario lifetime so the directory survives;
    /// dropped between scenarios because the World resets.
    _tempdir: Option<tempfile::TempDir>,
    output: Option<Output>,
}

impl CliWorld {
    fn require_dir(&self) -> &Path {
        self.project_dir
            .as_deref()
            .expect("scenario did not set up a project directory")
    }

    fn require_output(&self) -> &Output {
        self.output
            .as_ref()
            .expect("scenario did not run the binary")
    }

    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.require_output().stdout).into_owned()
    }
}

fn setup_project(world: &mut CliWorld, src: &str, lcov: &str) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    std::fs::create_dir_all(path.join("src")).expect("create src dir");
    std::fs::write(path.join("src/lib.rs"), src).expect("write src/lib.rs");
    std::fs::write(path.join("lcov.info"), lcov).expect("write lcov.info");
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

/// Parse a backtick-wrapped `crap4rs ...` command into the args vec
/// (drops the binary name). Returns args for `Command::args(&args)`.
fn parse_command(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().skip(1).map(str::to_string).collect()
}

// ── Background no-op steps ───────────────────────────────────────────
//
// `cli_ergonomics.feature` declares a Background that injects four
// descriptive Given steps (line 25-29) before every scenario. Those
// steps are aspirational — they describe the implicit project state
// the broader spec assumes — and have no executable step definitions
// in the rest of the codebase. To run `@summary` scenarios at all, the
// Background needs to "pass", so the steps below are intentional
// no-ops. The real fixture setup happens in the @summary-specific
// `Given a synthetic project where ...` step below.
#[given(regex = r#"^a project with an LCOV file at "[^"]+"$"#)]
fn given_background_lcov(_world: &mut CliWorld) {}

#[given("the project's analysis produces TOTAL_FUNCTIONS functions")]
fn given_background_total(_world: &mut CliWorld) {}

#[given("VIOLATING_FUNCTIONS of those functions exceed the threshold")]
fn given_background_violating(_world: &mut CliWorld) {}

#[given("TOTAL_FUNCTIONS > 0 and VIOLATING_FUNCTIONS > 0")]
fn given_background_positive(_world: &mut CliWorld) {}

// ── Given steps (real) ───────────────────────────────────────────────

#[given("a synthetic project where every function is within threshold")]
fn given_within(world: &mut CliWorld) {
    setup_project(world, WITHIN_SRC, WITHIN_LCOV);
}

#[given("a synthetic project where at least one function exceeds threshold")]
fn given_exceeds(world: &mut CliWorld) {
    setup_project(world, EXCEEDS_SRC, EXCEEDS_LCOV);
}

// ── When step ────────────────────────────────────────────────────────

#[when(regex = r#"^the operator runs `([^`]+)`$"#)]
fn when_run(world: &mut CliWorld, cmd: String) {
    let args = parse_command(&cmd);
    let needs_project = cmd.contains("--coverage") && !cmd.contains("--help");

    let mut command = std::process::Command::new(BINARY);
    if needs_project {
        let dir = world.require_dir();
        command.current_dir(dir);
    }
    command.args(&args);

    let output = command
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke crap4rs binary at {BINARY:?}: {e}"));
    world.output = Some(output);
}

// ── Then steps ───────────────────────────────────────────────────────

#[then("stdout contains exactly one line")]
fn then_one_line(world: &mut CliWorld) {
    let stdout = world.stdout();
    let trimmed = stdout.trim_end_matches('\n');
    let line_count = if trimmed.is_empty() {
        0
    } else {
        trimmed.lines().count()
    };
    assert_eq!(
        line_count, 1,
        "expected exactly one line on stdout, got {line_count}:\n{stdout}"
    );
}

#[then(regex = r#"^stdout matches "(.+)"$"#)]
fn then_matches(world: &mut CliWorld, pattern: String) {
    let stdout = world.stdout();
    let line = stdout.trim_end_matches('\n');
    let re =
        regex::Regex::new(&pattern).unwrap_or_else(|e| panic!("invalid regex {pattern:?}: {e}"));
    assert!(
        re.is_match(line),
        "stdout did not match {pattern:?}:\nstdout:\n{stdout}"
    );
}

#[then(regex = r#"^stdout starts with "([^"]+)"$"#)]
fn then_starts_with(world: &mut CliWorld, prefix: String) {
    let stdout = world.stdout();
    assert!(
        stdout.starts_with(&prefix),
        "stdout did not start with {prefix:?}:\nstdout:\n{stdout}"
    );
}

#[then(regex = r#"^stdout contains "([^"]+)"$"#)]
fn then_contains(world: &mut CliWorld, needle: String) {
    let stdout = world.stdout();
    assert!(
        stdout.contains(&needle),
        "stdout did not contain {needle:?}:\nstdout:\n{stdout}"
    );
}

#[then(regex = r#"^stdout does not contain "([^"]+)"$"#)]
fn then_not_contains(world: &mut CliWorld, needle: String) {
    let stdout = world.stdout();
    assert!(
        !stdout.contains(&needle),
        "stdout unexpectedly contained {needle:?}:\nstdout:\n{stdout}"
    );
}

#[then("stdout is empty")]
fn then_empty(world: &mut CliWorld) {
    let stdout = world.stdout();
    assert!(stdout.is_empty(), "expected empty stdout, got:\n{stdout}");
}

#[then(regex = r"^the exit code is (\d+)$")]
fn then_exit_code(world: &mut CliWorld, expected: i32) {
    let actual = world
        .require_output()
        .status
        .code()
        .expect("process exited via signal");
    let stdout = world.stdout();
    let stderr = String::from_utf8_lossy(&world.require_output().stderr);
    assert_eq!(
        actual, expected,
        "exit code mismatch — expected {expected}, got {actual}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // `writer::Libtest::or_basic()` emits libtest-compatible JSON under
    // nextest (which probes `--list`) and falls back to the basic
    // writer for plain `cargo test`. Matches `json_reporter_cucumber`.
    //
    // `filter_run_and_exit` loads `cli_ergonomics.feature` but executes
    // only `@summary`-tagged scenarios; other scenarios are aspirational
    // specs validated indirectly.
    //
    // `run_and_exit` (vs `run`) panics on scenario failure, propagating
    // a non-zero exit to CI — see memory `cucumber-run-vs-run-and-exit`.
    CliWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit("tests/features/cli_ergonomics.feature", |_, _, scenario| {
            scenario.tags.iter().any(|t| t == "summary")
        })
        .await;
}
