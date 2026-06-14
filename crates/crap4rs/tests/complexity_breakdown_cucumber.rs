//! Cucumber-rs runner for `@wired` scenarios in
//! `tests/features/complexity_breakdown.feature` (issue #169 umbrella).
//!
//! This harness pins only the CLI-process contracts the running binary
//! uniquely captures: the `--breakdown` flag wiring (sub-rows appear for
//! an exceeding function; absent by default), the `--explain` legend, the
//! `--explain` requires `--breakdown` validation exit code, and that
//! `--explain` leaves the JSON envelope shape intact with the
//! `contributors` array present. The lower-level behavior is owned by
//! crap-core unit tests: contributor EXTRACTION by
//! `adapters::complexity` (85 tests, one per node kind + the
//! sum/sorted/positive-increment invariants), the table sub-row RENDERING
//! (tree characters, `(nested)` suffix, line ordering, legend text) by
//! `reporters::table` `test_breakdown_*` / `test_explain_*`, and the JSON
//! contributor serde (kebab-case kind, null column, field shape) by
//! `domain::types`. (See `AGENTS.md` § BDD hygiene + `tests/features/TAGS.toml`.)
//!
//! Absorbs the former `explain_integration.rs` (which shelled the binary,
//! so it contributed no lib coverage — safe to fold into cucumber).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cucumber::{World, given, then, when, writer};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

/// A 3-level nested function: two `if-branch` contributors (one nested).
/// Fully uncovered it scores CRAP 20.0, so it exceeds even `--threshold 1`.
const NESTED_SRC: &str = "\
pub fn nested(x: bool, y: bool) -> i32 {
    if x {
        if y { 1 } else { 2 }
    } else {
        3
    }
}
";

const ZERO_COVERAGE_LCOV: &str = "\
SF:src/lib.rs
DA:1,0
DA:2,0
DA:3,0
DA:4,0
DA:5,0
DA:6,0
DA:7,0
end_of_record
";

#[derive(Debug, Default, World)]
struct BreakdownWorld {
    project_dir: Option<PathBuf>,
    _tempdir: Option<tempfile::TempDir>,
    output: Option<Output>,
}

impl BreakdownWorld {
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

    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.require_output().stderr).into_owned()
    }

    fn json(&self) -> serde_json::Value {
        let out = self.stdout();
        serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n{}", self.fail_context()))
    }

    /// A full failure context — exit status + both streams — for actionable
    /// CI panics when the binary misbehaves (e.g. crashes or errors instead
    /// of emitting the expected output, leaving stdout empty).
    fn fail_context(&self) -> String {
        let o = self.require_output();
        format!(
            "exit: {:?}\nstdout:\n{}\nstderr:\n{}",
            o.status.code(),
            self.stdout(),
            self.stderr()
        )
    }
}

fn parse_command(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().skip(1).map(str::to_string).collect()
}

// ── Given step ───────────────────────────────────────────────────────

#[given("a project with one nested function that exceeds threshold and is uncovered")]
fn given_nested(world: &mut BreakdownWorld) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    std::fs::create_dir_all(path.join("src")).expect("create src dir");
    std::fs::write(path.join("src/lib.rs"), NESTED_SRC).expect("write src/lib.rs");
    std::fs::write(path.join("lcov.info"), ZERO_COVERAGE_LCOV).expect("write lcov.info");
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

// ── When step ────────────────────────────────────────────────────────

#[when(regex = r#"^the operator runs `([^`]+)`$"#)]
fn when_run(world: &mut BreakdownWorld, cmd: String) {
    let args = parse_command(&cmd);
    let output = Command::new(BINARY)
        .current_dir(world.require_dir())
        .args(&args)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke crap4rs binary at {BINARY:?}: {e}"));
    world.output = Some(output);
}

// ── Then steps ───────────────────────────────────────────────────────

#[then(regex = r#"^stdout contains "([^"]+)"$"#)]
fn then_contains(world: &mut BreakdownWorld, needle: String) {
    assert!(
        world.stdout().contains(&needle),
        "stdout did not contain {needle:?}\n{}",
        world.fail_context()
    );
}

#[then(regex = r#"^stdout does not contain "([^"]+)"$"#)]
fn then_not_contains(world: &mut BreakdownWorld, needle: String) {
    assert!(
        !world.stdout().contains(&needle),
        "stdout unexpectedly contained {needle:?}\n{}",
        world.fail_context()
    );
}

#[then(regex = r#"^stderr contains "([^"]+)"$"#)]
fn then_stderr_contains(world: &mut BreakdownWorld, needle: String) {
    assert!(
        world.stderr().contains(&needle),
        "stderr did not contain {needle:?}\n{}",
        world.fail_context()
    );
}

#[then(regex = r"^the exit code is (\d+)$")]
fn then_exit_code(world: &mut BreakdownWorld, expected: i32) {
    let actual = world
        .require_output()
        .status
        .code()
        .expect("process exited via signal");
    assert_eq!(
        actual,
        expected,
        "exit code mismatch — expected {expected}, got {actual}\n{}",
        world.fail_context()
    );
}

/// Assert `result.functions[0].scored` carries a `contributors` array —
/// the contract that `--explain` (and JSON generally) preserves the
/// per-function contributor list.
#[then("the first function in the envelope carries a contributors array")]
fn then_first_fn_has_contributors(world: &mut BreakdownWorld) {
    let root = world.json();
    let contributors = root["result"]["functions"][0]["scored"].get("contributors");
    assert!(
        contributors.map(|c| c.is_array()).unwrap_or(false),
        "result.functions[0].scored.contributors is not an array; envelope:\n{root:#}"
    );
}

#[then(regex = r#"^the JSON envelope has no top-level "([^"]+)" key$"#)]
fn then_no_top_level(world: &mut BreakdownWorld, key: String) {
    let root = world.json();
    assert!(
        root.get(&key).is_none(),
        "top-level key {key:?} unexpectedly present; envelope:\n{root:#}"
    );
}

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    BreakdownWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit(
            "tests/features/complexity_breakdown.feature",
            |_, _, scenario| scenario.tags.iter().any(|t| t == "wired"),
        )
        .await;
}
