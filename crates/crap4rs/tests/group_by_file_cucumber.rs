//! Cucumber-rs runner for `@wired` scenarios in
//! `tests/features/group_by_file.feature` (issue #64 / `--group-by file`).
//!
//! This harness pins the CLI-process contracts the running binary uniquely
//! captures against a real 3-file project: the `view.grouped` envelope
//! shape, the `--group-by file` / `--top` / `--only-failing` / `--minimal-view`
//! flag wiring, the CSV per-file header, the gate keystone (grouping never
//! changes the exit code), and `--help` discoverability. The grouping +
//! file-level sort/truncate/filter SEMANTICS are owned by `domain::view`'s
//! 17 `group_by_file_*` unit tests (sort-by-coverage-asc, -complexity-desc,
//! -path-alpha, truncate-files, filters-compose-before-grouping, …), so the
//! three `--sort-by` ordering scenarios live there, not here (see
//! `AGENTS.md` § BDD hygiene). Absorbs `group_by_file_integration.rs`
//! (which shelled the binary → contributed no lib coverage; safe to fold).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cucumber::{World, given, then, when, writer};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

// A 6-function, 3-file project. At `--threshold 8` the branchy uncovered
// functions exceed and the simple covered ones pass:
//   blob.rs  — 3 functions, 2 exceeding
//   index.rs — 2 functions, 1 exceeding
//   util.rs  — 1 function,  0 exceeding
const BLOB_SRC: &str = "\
pub fn blob_fail_a(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn blob_fail_b(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn blob_ok() -> i32 { 1 }
";
const INDEX_SRC: &str = "\
pub fn index_fail(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn index_ok() -> i32 { 2 }
";
const UTIL_SRC: &str = "pub fn util_ok() -> i32 { 3 }\n";

const LCOV: &str = "\
SF:blob.rs
DA:1,0
DA:2,0
DA:3,1
end_of_record
SF:index.rs
DA:1,0
DA:2,1
end_of_record
SF:util.rs
DA:1,1
end_of_record
";

#[derive(Debug, Default, World)]
struct GroupWorld {
    project_dir: Option<PathBuf>,
    _tempdir: Option<tempfile::TempDir>,
    output: Option<Output>,
}

impl GroupWorld {
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

    fn fail_context(&self) -> String {
        let o = self.require_output();
        format!(
            "exit: {:?}\nstdout:\n{}\nstderr:\n{}",
            o.status.code(),
            self.stdout(),
            String::from_utf8_lossy(&o.stderr)
        )
    }

    fn json(&self) -> serde_json::Value {
        let out = self.stdout();
        serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n{}", self.fail_context()))
    }
}

/// Navigate a dotted path; returns None (rather than panicking) if a key is
/// missing, so absence can be asserted.
fn try_at<'a>(root: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = root;
    for key in path.split('.') {
        cur = cur.get(key)?;
    }
    Some(cur)
}

fn at<'a>(root: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    try_at(root, path).unwrap_or_else(|| panic!("JSON path {path:?} missing; envelope:\n{root:#}"))
}

fn parse_command(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().skip(1).map(str::to_string).collect()
}

// ── Background given ─────────────────────────────────────────────────

#[given(
    "a project with 6 functions across 3 files (blob.rs 3 functions 2 exceeding, index.rs 2 functions 1 exceeding, util.rs 1 function 0 exceeding)"
)]
fn given_three_files(world: &mut GroupWorld) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    std::fs::create_dir_all(path.join("src")).expect("create src dir");
    std::fs::write(path.join("src/blob.rs"), BLOB_SRC).expect("write blob.rs");
    std::fs::write(path.join("src/index.rs"), INDEX_SRC).expect("write index.rs");
    std::fs::write(path.join("src/util.rs"), UTIL_SRC).expect("write util.rs");
    std::fs::write(path.join("lcov.info"), LCOV).expect("write lcov.info");
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

// ── When step ────────────────────────────────────────────────────────

#[when(regex = r#"^the operator runs `([^`]+)`$"#)]
fn when_run(world: &mut GroupWorld, cmd: String) {
    let args = parse_command(&cmd);
    let needs_project = cmd.contains("--coverage") && !cmd.contains("--help");
    let mut command = Command::new(BINARY);
    if needs_project {
        command.current_dir(world.require_dir());
    }
    command.args(&args);
    let output = command
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke crap4rs binary at {BINARY:?}: {e}"));
    world.output = Some(output);
}

// ── Then steps ───────────────────────────────────────────────────────

#[then(regex = r#"^the JSON envelope at "([^"]+)" is (.+)$"#)]
fn then_envelope_is(world: &mut GroupWorld, path: String, expected: String) {
    let root = world.json();
    let actual = at(&root, &path);
    let want: serde_json::Value = serde_json::from_str(&expected)
        .unwrap_or_else(|e| panic!("expected literal {expected:?} is not valid JSON: {e}"));
    assert_eq!(
        *actual, want,
        "JSON path {path:?}: expected {want}, got {actual}"
    );
}

#[then(regex = r#"^the JSON envelope at "([^"]+)" has (\d+) entr(?:y|ies)$"#)]
fn then_envelope_len(world: &mut GroupWorld, path: String, n: usize) {
    let root = world.json();
    let arr = at(&root, &path)
        .as_array()
        .unwrap_or_else(|| panic!("JSON path {path:?} is not an array; envelope:\n{root:#}"));
    assert_eq!(
        arr.len(),
        n,
        "JSON path {path:?}: expected {n} entries, got {}",
        arr.len()
    );
}

#[then(regex = r#"^the JSON envelope has no "([^"]+)" path$"#)]
fn then_path_absent(world: &mut GroupWorld, path: String) {
    let root = world.json();
    assert!(
        try_at(&root, &path).is_none(),
        "JSON path {path:?} unexpectedly present; envelope:\n{root:#}"
    );
}

/// Each entry in `view.grouped.files` carries the full FileSummary field set.
#[then("each grouped file carries the FileSummary fields")]
fn then_grouped_file_fields(world: &mut GroupWorld) {
    let root = world.json();
    let files = at(&root, "view.grouped.files")
        .as_array()
        .expect("view.grouped.files is an array");
    let fields = [
        "file_path",
        "function_count",
        "exceeding_count",
        "average_crap",
        "median_crap",
        "max_crap",
        "worst_function",
        "distribution",
        "average_coverage",
        "max_complexity",
    ];
    for f in files {
        for key in fields {
            assert!(f.get(key).is_some(), "FileSummary missing {key:?}: {f}");
        }
    }
}

#[then("every grouped file has at least one exceeding function")]
fn then_every_file_exceeds(world: &mut GroupWorld) {
    let root = world.json();
    let files = at(&root, "view.grouped.files")
        .as_array()
        .expect("view.grouped.files is an array");
    for f in files {
        let n = f["exceeding_count"]
            .as_u64()
            .expect("exceeding_count is a number");
        assert!(
            n >= 1,
            "file {} has exceeding_count {n} (< 1): {f}",
            f["file_path"]
        );
    }
}

#[then(regex = r#"^the first stdout line is "(.+)"$"#)]
fn then_first_line(world: &mut GroupWorld, expected: String) {
    let stdout = world.stdout();
    let first = stdout.lines().next().unwrap_or("");
    assert_eq!(
        first,
        expected,
        "first line mismatch\n{}",
        world.fail_context()
    );
}

#[then(regex = r#"^stdout contains "([^"]+)"$"#)]
fn then_stdout_contains(world: &mut GroupWorld, needle: String) {
    assert!(
        world.stdout().contains(&needle),
        "stdout did not contain {needle:?}\n{}",
        world.fail_context()
    );
}

#[then(regex = r"^the exit code is (\d+)$")]
fn then_exit_code(world: &mut GroupWorld, expected: i32) {
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

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    GroupWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit("tests/features/group_by_file.feature", |_, _, scenario| {
            scenario.tags.iter().any(|t| t == "wired")
        })
        .await;
}
