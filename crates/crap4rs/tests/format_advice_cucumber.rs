//! Cucumber-rs runner for `@wired` scenarios in
//! `tests/features/format_advice.feature` (issue #76 / `--format advice`).
//!
//! This harness pins the CLI-process contracts the running binary uniquely
//! captures: the canonical envelope on stdout, diagnostic gating
//! (over-threshold verdicts carry the four-field diagnostic; under-threshold
//! ones omit the key) end-to-end through the real walker + coverage +
//! diagnostic engine, exit-code parity with `--format json`, the gate
//! keystone (`--no-fail` reports findings but flips only the exit code),
//! diagnostics surviving view shaping (`--top`), stdout/stderr stream
//! separation, and byte-determinism.
//!
//! The diagnostic *content* is owned elsewhere: the SuggestedAction
//! taxonomy, ProposedSplit shape + de-dup priority, `root_cause`
//! derivation, and the no-prose/no-names invariant by `domain::diagnostic`
//! (`pick_actions_*`, `dedup_splits_*`, `compute_diagnostic_*`, proptests);
//! the exact stderr line format by
//! `adapters::reporters::advice_summary` (`render_summary_*` + snapshot);
//! the view sort/filter shaping by `domain::view`; and `--explain` (a
//! `--breakdown` sub-feature, which exits 2 without `--breakdown`) by the
//! complexity_breakdown harness. So those cases live there, not here (see
//! `AGENTS.md` § BDD hygiene). Absorbs `format_advice_integration.rs`
//! (which shelled the binary → contributed no lib coverage; safe to fold).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cucumber::{World, given, then, when, writer};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

/// 5 functions: 2 trivial (covered, low CRAP), 3 branchy (uncovered, high
/// CRAP). At `--threshold 8` the three branchy functions exceed and carry a
/// diagnostic; the two trivial ones pass and omit it.
const FIXTURE_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn failing_a(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_b(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_c(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
";

const FIXTURE_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
DA:3,0
DA:4,0
DA:5,0
end_of_record
";

#[derive(Debug, Default, World)]
struct AdviceWorld {
    project_dir: Option<PathBuf>,
    _tempdir: Option<tempfile::TempDir>,
    last_cmd: Option<String>,
    output: Option<Output>,
}

impl AdviceWorld {
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

    fn shown(&self) -> Vec<serde_json::Value> {
        let root = self.json();
        at(&root, "view.shown")
            .as_array()
            .unwrap_or_else(|| panic!("view.shown is not an array; envelope:\n{root:#}"))
            .clone()
    }
}

fn at<'a>(root: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    let mut cur = root;
    for key in path.split('.') {
        cur = match key.parse::<usize>() {
            Ok(idx) => cur.get(idx).or_else(|| cur.get(key)),
            Err(_) => cur.get(key),
        }
        .unwrap_or_else(|| panic!("JSON path {path:?} missing; envelope:\n{root:#}"));
    }
    cur
}

fn parse_args(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().skip(1).map(str::to_string).collect()
}

fn run(dir: &Path, args: &[String]) -> Output {
    Command::new(BINARY)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke crap4rs binary at {BINARY:?}: {e}"))
}

fn is_exceeding(entry: &serde_json::Value) -> bool {
    entry["exceeds"].as_bool().unwrap_or(false)
}

/// Parse stdout and strip the wall-clock `timestamp` field so determinism
/// assertions are about the analysis, not the second the run landed in.
fn json_without_timestamp(stdout: &str) -> serde_json::Value {
    let mut v: serde_json::Value = serde_json::from_str(stdout).expect("stdout was not valid JSON");
    if let Some(obj) = v.as_object_mut() {
        obj.remove("timestamp");
    }
    v
}

// ── Given step ───────────────────────────────────────────────────────

#[given("a project with a mix of over-threshold and under-threshold functions")]
fn given_project(world: &mut AdviceWorld) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    std::fs::create_dir_all(path.join("src")).expect("create src dir");
    std::fs::write(path.join("src").join("lib.rs"), FIXTURE_SRC).expect("write lib.rs");
    std::fs::write(path.join("lcov.info"), FIXTURE_LCOV).expect("write lcov.info");
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

// ── When step ────────────────────────────────────────────────────────

#[when(regex = r#"^the operator runs `([^`]+)`$"#)]
fn when_run(world: &mut AdviceWorld, cmd: String) {
    let args = parse_args(&cmd);
    world.output = Some(run(world.require_dir(), &args));
    world.last_cmd = Some(cmd);
}

// ── Then steps: envelope ─────────────────────────────────────────────

#[then("stdout is parseable JSON")]
fn then_parseable(world: &mut AdviceWorld) {
    let _ = world.json();
}

#[then(regex = r#"^the JSON value at "([^"]+)" is (.+)$"#)]
fn then_json_value(world: &mut AdviceWorld, path: String, literal: String) {
    let root = world.json();
    let actual = at(&root, &path);
    let expected: serde_json::Value = serde_json::from_str(&literal)
        .unwrap_or_else(|e| panic!("expected literal {literal:?} is not valid JSON: {e}"));
    let matched = match (actual.as_f64(), expected.as_f64()) {
        (Some(a), Some(b)) => a == b,
        _ => *actual == expected,
    };
    assert!(
        matched,
        "JSON path {path:?}: expected {expected}, got {actual}"
    );
}

#[then(regex = r#"^the JSON path "([^"]+)" is an array$"#)]
fn then_json_array(world: &mut AdviceWorld, path: String) {
    let root = world.json();
    assert!(
        at(&root, &path).is_array(),
        "JSON path {path:?} is not an array; envelope:\n{root:#}"
    );
}

#[then(regex = r#"^the JSON value at "([^"]+)" has (\d+) entr(?:y|ies)$"#)]
fn then_json_len(world: &mut AdviceWorld, path: String, n: usize) {
    let root = world.json();
    let arr = at(&root, &path)
        .as_array()
        .unwrap_or_else(|| panic!("JSON path {path:?} is not an array; envelope:\n{root:#}"));
    assert_eq!(arr.len(), n, "JSON path {path:?}: expected {n} entries");
}

#[then("stdout is JSON-only with no table borders or prose")]
fn then_stdout_json_only(world: &mut AdviceWorld) {
    let out = world.stdout();
    let _ = world.json(); // parses as a single JSON value
    assert!(
        out.trim_start().starts_with('{'),
        "advice stdout must begin with a JSON object\n{}",
        world.fail_context()
    );
    for border in ['│', '┌', '┐', '└', '┘', '─', '╭', '╮', '├'] {
        assert!(
            !out.contains(border),
            "advice stdout contains a table-border char {border:?}\n{}",
            world.fail_context()
        );
    }
}

// ── Then steps: exit code ────────────────────────────────────────────

#[then(regex = r"^the exit code is (\d+)$")]
fn then_exit_code(world: &mut AdviceWorld, expected: i32) {
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

#[then("the exit code equals the same command under --format json")]
fn then_exit_parity(world: &mut AdviceWorld) {
    let cmd = world.last_cmd.clone().expect("no command was run");
    let json_cmd = cmd.replace("--format advice", "--format json");
    assert_ne!(json_cmd, cmd, "command did not contain `--format advice`");
    let json_run = run(world.require_dir(), &parse_args(&json_cmd));
    assert_eq!(
        world.require_output().status.code(),
        json_run.status.code(),
        "advice and json exit codes must match\nadvice: {}\njson exit: {:?}",
        world.fail_context(),
        json_run.status.code()
    );
}

// ── Then steps: diagnostic gating ────────────────────────────────────

#[then("every over-threshold entry carries a populated diagnostic")]
fn then_over_threshold_diagnostic(world: &mut AdviceWorld) {
    let shown = world.shown();
    let exceeding: Vec<_> = shown.iter().filter(|e| is_exceeding(e)).collect();
    assert!(
        !exceeding.is_empty(),
        "fixture must produce at least one over-threshold verdict"
    );
    for entry in exceeding {
        let diag = entry
            .get("diagnostic")
            .and_then(|d| d.as_object())
            .unwrap_or_else(|| panic!("over-threshold entry missing diagnostic: {entry}"));
        for field in [
            "coverage_gaps",
            "complexity_drivers",
            "suggested_actions",
            "root_cause",
        ] {
            assert!(
                diag.contains_key(field),
                "diagnostic missing field {field:?}: {diag:?}"
            );
        }
    }
}

#[then("every under-threshold entry omits the diagnostic key")]
fn then_under_threshold_no_diagnostic(world: &mut AdviceWorld) {
    let shown = world.shown();
    let passing: Vec<_> = shown.iter().filter(|e| !is_exceeding(e)).collect();
    assert!(
        !passing.is_empty(),
        "fixture must produce at least one under-threshold verdict"
    );
    for entry in passing {
        assert!(
            entry.get("diagnostic").is_none(),
            "under-threshold entry serialized a diagnostic key: {entry}"
        );
    }
}

#[then("no view.shown entry carries a diagnostic key")]
fn then_no_diagnostic_keys(world: &mut AdviceWorld) {
    for entry in world.shown() {
        assert!(
            entry.get("diagnostic").is_none(),
            "--format json must not populate diagnostic: {entry}"
        );
    }
}

// ── Then steps: stderr stream separation ─────────────────────────────

fn summary_line_count(stderr: &str) -> usize {
    stderr.lines().filter(|l| l.starts_with("[crap=")).count()
}

#[then(regex = r#"^stderr carries one "\[crap=" summary line per over-threshold function$"#)]
fn then_stderr_summary_per_function(world: &mut AdviceWorld) {
    let exceeders = world.shown().iter().filter(|e| is_exceeding(e)).count();
    let lines = summary_line_count(&world.stderr());
    assert!(
        exceeders > 0,
        "fixture must produce over-threshold functions"
    );
    assert_eq!(
        lines,
        exceeders,
        "expected one [crap=…] line per over-threshold function ({exceeders}), got {lines}\nstderr:\n{}",
        world.stderr()
    );
}

#[then(regex = r#"^stderr carries no "\[crap=" summary line$"#)]
fn then_no_stderr_summary(world: &mut AdviceWorld) {
    assert_eq!(
        summary_line_count(&world.stderr()),
        0,
        "no advice summary lines should appear; stderr:\n{}",
        world.stderr()
    );
}

// ── Then steps: determinism ──────────────────────────────────────────

#[then("running the same command again produces byte-identical stdout and stderr")]
fn then_deterministic(world: &mut AdviceWorld) {
    let cmd = world.last_cmd.clone().expect("no command was run");
    let again = run(world.require_dir(), &parse_args(&cmd));
    let again_stdout = String::from_utf8_lossy(&again.stdout).into_owned();
    // Strip the wall-clock timestamp before comparing stdout; stderr carries
    // no timestamp so it compares raw.
    assert_eq!(
        json_without_timestamp(&world.stdout()),
        json_without_timestamp(&again_stdout),
        "advice stdout must be deterministic (timestamp aside)"
    );
    assert_eq!(
        world.stderr(),
        String::from_utf8_lossy(&again.stderr),
        "advice stderr summary must be byte-identical across runs"
    );
}

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    AdviceWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit("tests/features/format_advice.feature", |_, _, scenario| {
            scenario.tags.iter().any(|t| t == "wired")
        })
        .await;
}
