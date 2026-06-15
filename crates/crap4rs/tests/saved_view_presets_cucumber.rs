//! Cucumber-rs runner for `@wired` scenarios in
//! `tests/features/saved_view_presets.feature` (issue #80 / `--view`).
//!
//! This harness pins the CLI-process contracts the running binary uniquely
//! captures: a preset on disk is discovered, resolved, applied, and
//! reflected in the `view.spec` envelope; a CLI flag overrides the
//! preset's value *through the pipeline* (an ordering the in-isolation
//! merge unit can't prove); resolution and config-load errors surface as
//! exit 2; and a preset never moves the gate.
//!
//! The preset *merge logic* — every-field application, per-field CLI
//! override, the bool OR-merge, the unknown-preset / no-config message
//! text — is owned by `crap-core`'s `cli::view_args` unit suite
//! (`full_preset_no_cli_overrides_applies_every_field`,
//! `cli_top_wins_over_preset_top`, `bool_or_merge_*`,
//! `resolve_view_preset_unknown_name_lists_available`,
//! `resolve_view_preset_no_config_file_explains_requirement`). The TOML
//! parsing/validation — multiple-preset independence, coverage-range
//! rejection — is owned by `adapters::config`
//! (`parse_multiple_view_presets_independent`,
//! `parse_view_preset_max_coverage_out_of_range_rejected`). So those
//! cases live there, not here (see `AGENTS.md` § BDD hygiene). Absorbs
//! `saved_view_presets_integration.rs` (which shelled the binary →
//! contributed no lib coverage; safe to fold).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cucumber::gherkin::Step;
use cucumber::{World, given, then, when, writer};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

/// 6 functions: 3 trivial (covered, low CRAP), 3 branchy (uncovered, high
/// CRAP). At `--threshold 5` the three branchy functions exceed.
const FIXTURE_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn passing_c() -> i32 { 3 }
pub fn failing_a(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_b(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_c(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
";

const FIXTURE_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
DA:3,1
DA:4,0
DA:5,0
DA:6,0
end_of_record
";

#[derive(Debug, Default, World)]
struct PresetWorld {
    project_dir: Option<PathBuf>,
    _tempdir: Option<tempfile::TempDir>,
    output: Option<Output>,
}

impl PresetWorld {
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

    fn fail_context(&self) -> String {
        let o = self.require_output();
        format!(
            "exit: {:?}\nstdout:\n{}\nstderr:\n{}",
            o.status.code(),
            self.stdout(),
            self.stderr()
        )
    }

    fn json(&self) -> serde_json::Value {
        let out = self.stdout();
        serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n{}", self.fail_context()))
    }
}

/// Navigate a dotted path; numeric segments index arrays, string segments
/// index objects. Returns None (rather than panicking) on a miss.
fn try_at<'a>(root: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = root;
    for key in path.split('.') {
        cur = match key.parse::<usize>() {
            Ok(idx) => cur.get(idx).or_else(|| cur.get(key))?,
            Err(_) => cur.get(key)?,
        };
    }
    Some(cur)
}

fn at<'a>(root: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    try_at(root, path).unwrap_or_else(|| panic!("JSON path {path:?} missing; envelope:\n{root:#}"))
}

fn write_config(dir: &Path, toml: &str) {
    std::fs::write(dir.join("crap.toml"), toml).expect("write crap.toml");
}

fn parse_args(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().skip(1).map(str::to_string).collect()
}

// ── Given steps ──────────────────────────────────────────────────────

#[given("a project with `crap.toml` containing:")]
fn given_project(world: &mut PresetWorld, step: &Step) {
    let toml = step
        .docstring
        .as_deref()
        .expect("Background step requires a crap.toml docstring");
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    std::fs::create_dir_all(path.join("src")).expect("create src dir");
    std::fs::write(path.join("src/lib.rs"), FIXTURE_SRC).expect("write lib.rs");
    std::fs::write(path.join("lcov.info"), FIXTURE_LCOV).expect("write lcov.info");
    write_config(&path, toml);
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

#[given("the config file instead contains:")]
fn given_config_instead(world: &mut PresetWorld, step: &Step) {
    let toml = step
        .docstring
        .as_deref()
        .expect("step requires a crap.toml docstring");
    write_config(world.require_dir(), toml);
}

// ── When step ────────────────────────────────────────────────────────

#[when(regex = r#"^the operator runs `([^`]+)`$"#)]
fn when_run(world: &mut PresetWorld, cmd: String) {
    let args = parse_args(&cmd);
    let output = Command::new(BINARY)
        .current_dir(world.require_dir())
        .args(&args)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke crap4rs binary at {BINARY:?}: {e}"));
    world.output = Some(output);
}

// ── Then steps ───────────────────────────────────────────────────────

#[then(regex = r#"^the JSON value at "([^"]+)" is (.+)$"#)]
fn then_json_value(world: &mut PresetWorld, path: String, literal: String) {
    let root = world.json();
    let actual = at(&root, &path);
    let expected: serde_json::Value = serde_json::from_str(&literal)
        .unwrap_or_else(|e| panic!("expected literal {literal:?} is not valid JSON: {e}"));
    // Compare numbers by f64 so an integer literal (e.g. `90`) matches a
    // float field (e.g. coverage_range `90.0`).
    let matched = match (actual.as_f64(), expected.as_f64()) {
        (Some(a), Some(b)) => a == b,
        _ => *actual == expected,
    };
    assert!(
        matched,
        "JSON path {path:?}: expected {expected}, got {actual}"
    );
}

#[then(regex = r#"^the JSON path "([^"]+)" is absent$"#)]
fn then_json_absent(world: &mut PresetWorld, path: String) {
    let root = world.json();
    assert!(
        try_at(&root, &path).is_none(),
        "JSON path {path:?} unexpectedly present; envelope:\n{root:#}"
    );
}

#[then(regex = r"^the exit code is (\d+)$")]
fn then_exit_code(world: &mut PresetWorld, expected: i32) {
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

#[then(regex = r#"^stdout contains "([^"]+)"$"#)]
fn then_stdout_contains(world: &mut PresetWorld, needle: String) {
    assert!(
        world.stdout().contains(&needle),
        "stdout did not contain {needle:?}\n{}",
        world.fail_context()
    );
}

#[then(regex = r#"^stderr contains "([^"]+)"$"#)]
fn then_stderr_contains(world: &mut PresetWorld, needle: String) {
    assert!(
        world.stderr().contains(&needle),
        "stderr did not contain {needle:?}\n{}",
        world.fail_context()
    );
}

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    PresetWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit(
            "tests/features/saved_view_presets.feature",
            |_, _, scenario| scenario.tags.iter().any(|t| t == "wired"),
        )
        .await;
}
