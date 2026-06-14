//! Cucumber-rs runner for `@wired` scenarios in
//! `tests/features/diff_mode.feature` (issue #81 / `--diff <ref>`).
//!
//! diff_mode.feature's CLI-acceptance contracts — the function-selection
//! a `--diff <ref>` run surfaces (only changed functions; hunk-level
//! precision), the empty-diff exit code, the `diff_ref` envelope field,
//! the validation exit codes (not a git repo / invalid ref / dash ref),
//! filter composition with `--exclude` / `--only-failing`, and rename
//! handling — are wired here. The lower-level diff MECHANICS live in
//! crap-core: the git-diff adapter (unified-diff + hunk parsing,
//! deletion-only skipping, new-file, bad-ref, empty-diff, path
//! normalization) is owned by `adapters::diff` unit tests, the `.rs`
//! extension filter by `core`/`walker`, and the `diff_ref` serialization
//! by `reporters::json`. This harness pins the things that need the real
//! binary process against a real git repo — including the
//! function-selection step (`core::compute_diff_regions`), which has no
//! crap-core unit test of its own (see `AGENTS.md` § BDD hygiene +
//! `tests/features/TAGS.toml`).
//!
//! Each scenario builds a tempdir git repo with controlled commits, then
//! runs the binary via `CARGO_BIN_EXE_crap4rs` with `--diff <ref>`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cucumber::{World, given, then, when, writer};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

#[derive(Debug, Default, World)]
struct DiffWorld {
    project_dir: Option<PathBuf>,
    /// Held during the scenario lifetime so the directory survives;
    /// dropped between scenarios because the World resets.
    _tempdir: Option<tempfile::TempDir>,
    output: Option<Output>,
}

impl DiffWorld {
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
            .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nraw stdout:\n{out}"))
    }

    /// The qualified names of every function at a `*.functions` /
    /// `*.shown` JSON array of FunctionVerdicts.
    fn names_at(&self, ptr: &[&str]) -> Vec<String> {
        let root = self.json();
        let mut cur = &root;
        for key in ptr {
            cur = cur.get(key).unwrap_or_else(|| {
                panic!("JSON path {ptr:?} missing at {key:?}; envelope:\n{root:#}")
            });
        }
        cur.as_array()
            .unwrap_or_else(|| panic!("JSON path {ptr:?} is not an array"))
            .iter()
            .map(|f| {
                f["scored"]["identity"]["qualified_name"]
                    .as_str()
                    .expect("qualified_name is a string")
                    .to_string()
            })
            .collect()
    }

    /// Names in the unshapeable analysis result (diff-scoped — the `--diff`
    /// filter applies to the analysis, not just the view).
    fn function_names(&self) -> Vec<String> {
        self.names_at(&["result", "functions"])
    }

    /// Names in the shaped view (`view.shown`) — what `--only-failing`
    /// and friends filter, distinct from `result.functions`.
    fn view_names(&self) -> Vec<String> {
        self.names_at(&["view", "shown"])
    }
}

// ── git + fixture helpers ─────────────────────────────────────────────

fn run_git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git command failed to start");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_init(dir: &Path) {
    run_git(dir, &["init", "-q"]);
    run_git(dir, &["config", "user.email", "test@test.com"]);
    run_git(dir, &["config", "user.name", "Test"]);
}

fn git_commit_all(dir: &Path, message: &str) {
    run_git(dir, &["add", "-A"]);
    run_git(dir, &["commit", "-q", "-m", message]);
}

fn new_project(world: &mut DiffWorld) -> PathBuf {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    std::fs::create_dir_all(path.join("src")).expect("create src dir");
    world.project_dir = Some(path.clone());
    world._tempdir = Some(dir);
    path
}

fn write(dir: &Path, rel: &str, contents: &str) {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(p, contents).expect("write file");
}

/// Parse a backtick/plain `crap4rs ...` command into the args vec
/// (drops the binary name). Returns args for `Command::args(&args)`.
fn parse_command(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().skip(1).map(str::to_string).collect()
}

// ── Given steps ──────────────────────────────────────────────────────

#[given("a git repo whose latest commit changed only function foo")]
fn given_repo_foo_changed(world: &mut DiffWorld) {
    let dir = new_project(world);
    git_init(&dir);
    write(
        &dir,
        "src/lib.rs",
        "pub fn foo() -> i32 { 1 }\npub fn bar() -> i32 { 2 }\n",
    );
    write(
        &dir,
        "lcov.info",
        "SF:lib.rs\nDA:1,1\nDA:2,1\nend_of_record\n",
    );
    git_commit_all(&dir, "base");
    // Modify only foo; bar untouched.
    write(
        &dir,
        "src/lib.rs",
        "pub fn foo() -> i32 {\n    let x = 1;\n    x\n}\npub fn bar() -> i32 { 2 }\n",
    );
    write(
        &dir,
        "lcov.info",
        "SF:lib.rs\nDA:1,1\nDA:2,1\nDA:3,1\nDA:4,1\nend_of_record\n",
    );
    git_commit_all(&dir, "change foo");
}

#[given("a git repo whose latest commit changed only function alpha, leaving beta untouched")]
fn given_repo_alpha_changed(world: &mut DiffWorld) {
    let dir = new_project(world);
    git_init(&dir);
    let base = "pub fn alpha() -> i32 {\n    let a = 1;\n    let b = 2;\n    a + b\n}\npub fn beta() -> i32 {\n    let x = 1;\n    let y = 2;\n    x + y\n}\n";
    write(&dir, "src/lib.rs", base);
    write(
        &dir,
        "lcov.info",
        "SF:lib.rs\nDA:1,1\nDA:2,1\nDA:3,1\nDA:4,1\nDA:6,1\nDA:7,1\nDA:8,1\nend_of_record\n",
    );
    git_commit_all(&dir, "base");
    // Change only alpha's body (line 3); beta's lines are untouched.
    let changed = "pub fn alpha() -> i32 {\n    let a = 1;\n    let b = 22;\n    a + b\n}\npub fn beta() -> i32 {\n    let x = 1;\n    let y = 2;\n    x + y\n}\n";
    write(&dir, "src/lib.rs", changed);
    git_commit_all(&dir, "change alpha");
}

#[given("a git repo with no changes since HEAD")]
fn given_repo_no_changes(world: &mut DiffWorld) {
    let dir = new_project(world);
    git_init(&dir);
    write(&dir, "src/lib.rs", "pub fn stable() -> i32 { 42 }\n");
    write(&dir, "lcov.info", "SF:lib.rs\nDA:1,1\nend_of_record\n");
    git_commit_all(&dir, "only commit");
}

#[given("a git repo with changes in src/lib.rs and src/tests/test_lib.rs")]
fn given_repo_src_and_tests_changed(world: &mut DiffWorld) {
    // Both files live under --src (src/); the test file in a tests/
    // subdir so `--exclude tests/**` can drop it. An empty baseline
    // commit means both files are Added at HEAD~1, so both are "changed".
    let dir = new_project(world);
    git_init(&dir);
    run_git(&dir, &["commit", "--allow-empty", "-q", "-m", "baseline"]);
    write(&dir, "src/lib.rs", "pub fn kept() -> i32 { 1 }\n");
    write(
        &dir,
        "src/tests/test_lib.rs",
        "pub fn excluded() -> i32 { 1 }\n",
    );
    write(
        &dir,
        "lcov.info",
        "SF:lib.rs\nDA:1,1\nend_of_record\nSF:tests/test_lib.rs\nDA:1,1\nend_of_record\n",
    );
    git_commit_all(&dir, "add files");
}

#[given("a git repo with changed functions, one passing and one exceeding threshold")]
fn given_repo_mixed_threshold_changed(world: &mut DiffWorld) {
    // `simple` is covered (low CRAP, passes); `complex` is deeply nested
    // with an uncovered body line (high CRAP, exceeds a low threshold).
    // Both bodies change between the two commits.
    let dir = new_project(world);
    git_init(&dir);
    let base = "pub fn simple() -> i32 { 1 }\npub fn complex(x: i32) -> i32 {\n    if x > 0 { if x > 10 { if x > 100 { 3 } else { 2 } } else { 1 } } else { 0 }\n}\n";
    write(&dir, "src/lib.rs", base);
    write(
        &dir,
        "lcov.info",
        "SF:lib.rs\nDA:1,1\nDA:2,1\nDA:3,0\nend_of_record\n",
    );
    git_commit_all(&dir, "base");
    let changed = "pub fn simple() -> i32 { 2 }\npub fn complex(x: i32) -> i32 {\n    if x > 0 { if x > 10 { if x > 100 { 4 } else { 3 } } else { 2 } } else { 1 }\n}\n";
    write(&dir, "src/lib.rs", changed);
    git_commit_all(&dir, "change both");
}

#[given("a project that is not a git repository")]
fn given_not_a_git_repo(world: &mut DiffWorld) {
    let dir = new_project(world);
    write(&dir, "src/lib.rs", "pub fn f() -> i32 { 1 }\n");
    write(&dir, "lcov.info", "SF:lib.rs\nDA:1,1\nend_of_record\n");
}

#[given("a git repo")]
fn given_a_git_repo(world: &mut DiffWorld) {
    let dir = new_project(world);
    git_init(&dir);
    write(&dir, "src/lib.rs", "pub fn f() -> i32 { 1 }\n");
    write(&dir, "lcov.info", "SF:lib.rs\nDA:1,1\nend_of_record\n");
    git_commit_all(&dir, "only commit");
}

#[given("a git repo where src/old.rs was renamed to src/new.rs with changes")]
fn given_repo_renamed(world: &mut DiffWorld) {
    let dir = new_project(world);
    git_init(&dir);
    write(
        &dir,
        "src/old.rs",
        "pub fn moved() -> i32 {\n    let a = 1;\n    a\n}\n",
    );
    write(
        &dir,
        "lcov.info",
        "SF:old.rs\nDA:1,1\nDA:2,1\nDA:3,1\nend_of_record\n",
    );
    git_commit_all(&dir, "base");
    run_git(&dir, &["mv", "src/old.rs", "src/new.rs"]);
    write(
        &dir,
        "src/new.rs",
        "pub fn moved() -> i32 {\n    let a = 11;\n    a\n}\n",
    );
    write(
        &dir,
        "lcov.info",
        "SF:new.rs\nDA:1,1\nDA:2,1\nDA:3,1\nend_of_record\n",
    );
    git_commit_all(&dir, "rename + change");
}

// ── When step ────────────────────────────────────────────────────────

#[when(regex = r#"^the operator runs `([^`]+)`$"#)]
fn when_run(world: &mut DiffWorld, cmd: String) {
    let args = parse_command(&cmd);
    let dir = world.require_dir();
    let output = Command::new(BINARY)
        .current_dir(dir)
        .args(&args)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke crap4rs binary at {BINARY:?}: {e}"));
    world.output = Some(output);
}

// ── Then steps ───────────────────────────────────────────────────────

#[then(regex = r#"^the report includes function "([^"]+)"$"#)]
fn then_report_includes(world: &mut DiffWorld, name: String) {
    let names = world.function_names();
    assert!(
        names.iter().any(|n| n == &name),
        "expected function {name:?} in the report; got {names:?}"
    );
}

#[then(regex = r#"^the report excludes function "([^"]+)"$"#)]
fn then_report_excludes(world: &mut DiffWorld, name: String) {
    let names = world.function_names();
    assert!(
        !names.iter().any(|n| n == &name),
        "function {name:?} should be absent from the report; got {names:?}"
    );
}

#[then(regex = r"^the report contains (\d+) functions?$")]
fn then_report_count(world: &mut DiffWorld, n: usize) {
    let names = world.function_names();
    assert_eq!(names.len(), n, "expected {n} functions, got {names:?}");
}

#[then(regex = r#"^the view includes function "([^"]+)"$"#)]
fn then_view_includes(world: &mut DiffWorld, name: String) {
    let names = world.view_names();
    assert!(
        names.iter().any(|n| n == &name),
        "expected function {name:?} in the view; got {names:?}"
    );
}

#[then(regex = r#"^the view excludes function "([^"]+)"$"#)]
fn then_view_excludes(world: &mut DiffWorld, name: String) {
    let names = world.view_names();
    assert!(
        !names.iter().any(|n| n == &name),
        "function {name:?} should be absent from the view; got {names:?}"
    );
}

#[then(regex = r#"^the JSON envelope at "([^"]+)" is (.+)$"#)]
fn then_envelope_is(world: &mut DiffWorld, path: String, expected: String) {
    let root = world.json();
    let mut cur = &root;
    for key in path.split('.') {
        cur = cur.get(key).unwrap_or_else(|| {
            panic!("JSON path {path:?} missing at {key:?}; envelope:\n{root:#}")
        });
    }
    let want: serde_json::Value = serde_json::from_str(&expected)
        .unwrap_or_else(|e| panic!("expected literal {expected:?} is not valid JSON: {e}"));
    assert_eq!(*cur, want, "JSON path {path:?}: expected {want}, got {cur}");
}

#[then(regex = r"^the exit code is (\d+)$")]
fn then_exit_code(world: &mut DiffWorld, expected: i32) {
    let actual = world
        .require_output()
        .status
        .code()
        .expect("process exited via signal");
    let stdout = world.stdout();
    let stderr = world.stderr();
    assert_eq!(
        actual, expected,
        "exit code mismatch — expected {expected}, got {actual}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[then(regex = r#"^the result reports passed as (true|false)$"#)]
fn then_result_passed(world: &mut DiffWorld, expected: String) {
    let want: bool = expected.parse().expect("true/false");
    let got = world.json()["result"]["passed"]
        .as_bool()
        .expect("result.passed is a bool");
    assert_eq!(got, want, "result.passed: expected {want}, got {got}");
}

#[then(regex = r#"^stderr contains "([^"]+)"$"#)]
fn then_stderr_contains(world: &mut DiffWorld, needle: String) {
    let stderr = world.stderr();
    assert!(
        stderr.contains(&needle),
        "stderr did not contain {needle:?}:\nstderr:\n{stderr}"
    );
}

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // `writer::Libtest::or_basic()` emits libtest-compatible JSON under
    // nextest and falls back to the basic writer for plain `cargo test`.
    // `filter_run_and_exit` executes only `@wired` scenarios; `run_and_exit`
    // semantics propagate failure to CI (memory `cucumber-run-vs-run-and-exit`).
    DiffWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit("tests/features/diff_mode.feature", |_, _, scenario| {
            scenario.tags.iter().any(|t| t == "wired")
        })
        .await;
}
