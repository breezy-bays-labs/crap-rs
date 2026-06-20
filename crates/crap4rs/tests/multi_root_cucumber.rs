//! Cucumber-rs runner for `@wired` scenarios in
//! `tests/features/multi_root_src.feature` (issue #336 / repeatable `--src`).
//!
//! This harness pins the CLI-process contracts the running binary uniquely
//! captures: that `prepare_pipeline` resolves the run identity base from the
//! `--src` root COUNT (one root ⇒ src-relative, byte-compatible with the
//! pre-multi-root path; many roots ⇒ git-toplevel-relative so same-named
//! files in different roots stay distinct), and that multi-root outside a
//! git work tree is a hard error rather than a silent basename strip.
//!
//! The union/dedup invariant, the `core::identity::IdentityBase`
//! consumption, and the coverage-join no-bleed contract are owned IN-PROCESS
//! by `multi_root_integration.rs` — it constructs `IdentityBase` directly
//! and calls `analyze()` at the library boundary, the SOLE lib coverage of
//! that path, so it stays (a `harness = false` cucumber runner is skipped by
//! nextest and would contribute no coverage). The scorecard action's
//! comment-preamble / comment-header surfaces are owned at the CI layer
//! (`.github/actions/scorecard/action.yml` + the dogfood smoke jobs). So
//! those cases live there, not here (see `AGENTS.md` § BDD hygiene).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cucumber::{World, given, then, when, writer};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

/// Toplevel-relative LCOV covering both roots. Key assertions read the
/// walker-derived `file_path`, which is independent of coverage overlap, so
/// a single fixture serves the single-root scenario too (its src-relative
/// keys simply don't join this toplevel-keyed coverage — irrelevant here).
const LCOV: &str = "\
SF:crate-a/src/lib.rs
DA:1,1
end_of_record
SF:crate-b/src/lib.rs
DA:1,1
end_of_record
SF:crate-a/src/adapters/mod.rs
DA:1,1
end_of_record
SF:crate-b/src/adapters/mod.rs
DA:1,0
end_of_record
";

#[derive(Debug, Default, World)]
struct MultiRootWorld {
    project_dir: Option<PathBuf>,
    _tempdir: Option<tempfile::TempDir>,
    output: Option<Output>,
}

impl MultiRootWorld {
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

    /// Every `view.shown[].scored.identity.file_path` key.
    fn shown_keys(&self) -> Vec<String> {
        let out = self.stdout();
        let root: serde_json::Value = serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n{}", self.fail_context()));
        root["view"]["shown"]
            .as_array()
            .unwrap_or_else(|| panic!("view.shown is not an array\n{}", self.fail_context()))
            .iter()
            .map(|v| {
                v["scored"]["identity"]["file_path"]
                    .as_str()
                    .expect("file_path is a string")
                    .to_string()
            })
            .collect()
    }
}

fn run_git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke git {args:?}: {e}"));
    assert!(
        output.status.success(),
        "git {args:?} failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Scaffold two crate-like roots that share a crate-internal relative path
/// (`adapters/mod.rs`) plus distinct `lib.rs` files. `git` controls whether
/// a git work tree is initialized (the hard-error scenario needs none).
fn scaffold(git: bool) -> (PathBuf, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let root = tmp.path().to_path_buf();
    for (crate_name, lib_body, mod_body) in [
        (
            "crate-a",
            "pub fn a_only(x: i32) -> i32 { if x > 0 { x } else { -x } }\n",
            "pub fn shared_a() -> u8 { 1 }\n",
        ),
        (
            "crate-b",
            "pub fn b_only(y: i32) -> i32 { y * 2 }\n",
            "pub fn shared_b() -> u8 { 2 }\n",
        ),
    ] {
        let src = root.join(crate_name).join("src");
        std::fs::create_dir_all(src.join("adapters")).expect("create src/adapters dir");
        std::fs::write(src.join("lib.rs"), lib_body).expect("write lib.rs");
        std::fs::write(src.join("adapters").join("mod.rs"), mod_body)
            .expect("write adapters/mod.rs");
    }
    std::fs::write(root.join("lcov.info"), LCOV).expect("write lcov.info");
    if git {
        run_git(&root, &["init", "-q"]);
        run_git(&root, &["config", "user.email", "t@t.t"]);
        run_git(&root, &["config", "user.name", "t"]);
    }
    (root, tmp)
}

fn parse_args(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().skip(1).map(str::to_string).collect()
}

// ── Given steps ──────────────────────────────────────────────────────

#[given("a git work tree with source roots crate-a/src and crate-b/src")]
fn given_git(world: &mut MultiRootWorld) {
    let (dir, tmp) = scaffold(true);
    world.project_dir = Some(dir);
    world._tempdir = Some(tmp);
}

#[given("a non-git directory with source roots crate-a/src and crate-b/src")]
fn given_nongit(world: &mut MultiRootWorld) {
    let (dir, tmp) = scaffold(false);
    world.project_dir = Some(dir);
    world._tempdir = Some(tmp);
}

// ── When step ────────────────────────────────────────────────────────

#[when(regex = r#"^the operator runs `([^`]+)`$"#)]
fn when_run(world: &mut MultiRootWorld, cmd: String) {
    let args = parse_args(&cmd);
    let output = Command::new(BINARY)
        .current_dir(world.require_dir())
        .args(&args)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke crap4rs binary at {BINARY:?}: {e}"));
    world.output = Some(output);
}

// ── Then steps ───────────────────────────────────────────────────────

#[then(regex = r"^view\.shown has (\d+) functions$")]
fn then_shown_count(world: &mut MultiRootWorld, n: usize) {
    let keys = world.shown_keys();
    assert_eq!(
        keys.len(),
        n,
        "expected {n} functions in view.shown, got {}: {keys:?}",
        keys.len()
    );
}

#[then(regex = r#"^view\.shown contains a function keyed "([^"]+)"$"#)]
fn then_shown_contains_key(world: &mut MultiRootWorld, key: String) {
    let keys = world.shown_keys();
    assert!(
        keys.contains(&key),
        "view.shown has no function keyed {key:?}; keys: {keys:?}"
    );
}

#[then("every view.shown function key is src-relative")]
fn then_keys_src_relative(world: &mut MultiRootWorld) {
    let keys = world.shown_keys();
    assert!(!keys.is_empty(), "view.shown must not be empty");
    for key in &keys {
        assert!(
            !Path::new(key).is_absolute() && !key.starts_with("crate-"),
            "single-root identity must be src-relative, but {key:?} is absolute or carries a root prefix; keys: {keys:?}"
        );
    }
}

#[then(regex = r"^the exit code is (\d+)$")]
fn then_exit_code(world: &mut MultiRootWorld, expected: i32) {
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

#[then(regex = r#"^stderr contains "([^"]+)"$"#)]
fn then_stderr_contains(world: &mut MultiRootWorld, needle: String) {
    assert!(
        world.stderr().contains(&needle),
        "stderr did not contain {needle:?}\n{}",
        world.fail_context()
    );
}

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    MultiRootWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .with_default_cli()
        .filter_run_and_exit("tests/features/multi_root_src.feature", |_, _, scenario| {
            scenario.tags.iter().any(|t| t == "wired")
        })
        .await;
}
