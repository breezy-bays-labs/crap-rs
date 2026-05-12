//! Cucumber-rs runner for `@wired`-tagged scenarios in
//! `tests/features/cli_init.feature` (crap-rs#73).
//!
//! Each scenario sets up a tempdir (potentially with a `src/` or
//! `crates/` subdirectory to exercise auto-detect) and invokes the
//! `crap4rs init` subcommand via `CARGO_BIN_EXE_crap4rs`. Cross-adapter
//! parity (`crap4ts init` writing `crap4ts.toml`) is exercised by the
//! plain integration test at `crates/crap4ts/tests/cli_init_integration.rs`
//! — that env var is per-package and `CARGO_BIN_EXE_crap4ts` is not
//! available inside crap4rs's harness.
//!
//! The harness uses `filter_run_and_exit` with the `@wired` filter
//! pattern per AGENTS.md § BDD hygiene rule 5; `@unwired` scenarios
//! (if any are added) are skipped.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use cucumber::{World, given, then, when, writer};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

#[derive(Debug, Default, World)]
struct InitWorld {
    project_dir: Option<PathBuf>,
    /// Held during the scenario lifetime so the directory survives;
    /// dropped between scenarios because the World resets.
    _tempdir: Option<tempfile::TempDir>,
    output: Option<Output>,
}

impl InitWorld {
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

    fn config_path(&self, name: &str) -> PathBuf {
        self.require_dir().join(name)
    }

    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.require_output().stderr).into_owned()
    }
}

fn fresh_tempdir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    (dir, path)
}

/// Parse a backtick-wrapped `crap4rs ...` command into the args vec
/// (drops the binary name). Returns args for `Command::args(&args)`.
fn parse_command(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().skip(1).map(str::to_string).collect()
}

// ── Given steps ──────────────────────────────────────────────────────

#[given("an empty project directory")]
fn given_empty_dir(world: &mut InitWorld) {
    let (dir, path) = fresh_tempdir();
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

#[given(regex = r#"^a project directory with a "([^"]+)" subdirectory$"#)]
fn given_dir_with_subdir(world: &mut InitWorld, name: String) {
    let (dir, path) = fresh_tempdir();
    std::fs::create_dir_all(path.join(&name)).expect("create subdir");
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

#[given(
    regex = r#"^a project directory with a "([^"]+)" subdirectory but no "([^"]+)" subdirectory$"#
)]
fn given_dir_with_subdir_but_not(world: &mut InitWorld, present: String, _absent: String) {
    let (dir, path) = fresh_tempdir();
    std::fs::create_dir_all(path.join(&present)).expect("create subdir");
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

#[given(regex = r#"^a project directory with an existing "([^"]+)" containing '([^']+)'$"#)]
fn given_dir_with_existing_config(world: &mut InitWorld, name: String, line: String) {
    let (dir, path) = fresh_tempdir();
    std::fs::write(path.join(&name), format!("{line}\n")).expect("seed existing config");
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

// ── When steps ───────────────────────────────────────────────────────

#[when(regex = r#"^the operator runs `([^`]+)`$"#)]
fn when_run(world: &mut InitWorld, cmd: String) {
    run_with_stdin(world, &cmd, None);
}

#[when(regex = r#"^the operator runs `([^`]+)` with stdin "(.*)"$"#)]
fn when_run_with_stdin(world: &mut InitWorld, cmd: String, raw_stdin: String) {
    // Cucumber doc-strings pass literal backslashes through, so decode
    // `\n` and `\t` ourselves rather than rely on the parser.
    let decoded = raw_stdin.replace("\\n", "\n").replace("\\t", "\t");
    run_with_stdin(world, &cmd, Some(decoded));
}

fn run_with_stdin(world: &mut InitWorld, cmd: &str, stdin_text: Option<String>) {
    let args = parse_command(cmd);
    let mut command = Command::new(BINARY);
    if let Some(dir) = world.project_dir.as_deref() {
        command.current_dir(dir);
    }
    command.args(&args);

    if let Some(text) = stdin_text {
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn {BINARY:?}: {e}"));
        if let Some(mut sin) = child.stdin.take() {
            use std::io::Write;
            sin.write_all(text.as_bytes()).expect("write stdin");
        }
        let output = child
            .wait_with_output()
            .unwrap_or_else(|e| panic!("failed to wait on {BINARY:?}: {e}"));
        world.output = Some(output);
    } else {
        let output = command
            .output()
            .unwrap_or_else(|e| panic!("failed to invoke {BINARY:?}: {e}"));
        world.output = Some(output);
    }
}

// ── Then steps ───────────────────────────────────────────────────────

#[then(regex = r#"^a file named "([^"]+)" exists in the project directory$"#)]
fn then_file_exists(world: &mut InitWorld, name: String) {
    let path = world.config_path(&name);
    assert!(path.exists(), "expected {} to exist", path.display());
}

#[then(regex = r#"^no file named "([^"]+)" exists in the project directory$"#)]
fn then_no_file(world: &mut InitWorld, name: String) {
    let path = world.config_path(&name);
    assert!(!path.exists(), "expected {} to NOT exist", path.display());
}

#[then(regex = r#"^the config file contains '([^']+)'$"#)]
fn then_contains_single_quoted(world: &mut InitWorld, needle: String) {
    assert_config_contains(world, &needle);
}

#[then(regex = r#"^the config file contains "([^"]+)"$"#)]
fn then_contains_double_quoted(world: &mut InitWorld, needle: String) {
    assert_config_contains(world, &needle);
}

#[then(regex = r#"^the config file does not contain "([^"]+)"$"#)]
fn then_not_contains_double_quoted(world: &mut InitWorld, needle: String) {
    assert_config_excludes(world, &needle);
}

#[then(regex = r#"^the config file does not contain '([^']+)'$"#)]
fn then_not_contains_single_quoted(world: &mut InitWorld, needle: String) {
    assert_config_excludes(world, &needle);
}

#[then(regex = r#"^the config file still contains '([^']+)'$"#)]
fn then_still_contains(world: &mut InitWorld, needle: String) {
    assert_config_contains(world, &needle);
}

fn assert_config_contains(world: &InitWorld, needle: &str) {
    let path = world.config_path("crap4rs.toml");
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(
        content.contains(needle),
        "config file at {} did not contain {needle:?}\nfull content:\n{content}",
        path.display(),
    );
}

fn assert_config_excludes(world: &InitWorld, needle: &str) {
    let path = world.config_path("crap4rs.toml");
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(
        !content.contains(needle),
        "config file at {} unexpectedly contained {needle:?}\nfull content:\n{content}",
        path.display(),
    );
}

#[then("the generated config file loads without error")]
fn then_loads(world: &mut InitWorld) {
    let path = world.config_path("crap4rs.toml");
    let content = std::fs::read_to_string(&path).expect("read config");
    // We don't have the loader in scope here, so smoke via the toml
    // crate directly — the loader does the same thing.
    let parsed: toml::Value = toml::from_str(&content)
        .unwrap_or_else(|e| panic!("generated config did not parse as TOML: {e}\n{content}"));
    assert!(
        parsed.get("preset").is_some() || parsed.get("threshold").is_some(),
        "expected preset or threshold key in parsed TOML",
    );
}

#[then(regex = r#"^the loaded config has preset "([^"]+)"$"#)]
fn then_loaded_preset(world: &mut InitWorld, expected: String) {
    let path = world.config_path("crap4rs.toml");
    let content = std::fs::read_to_string(&path).expect("read config");
    let parsed: toml::Value = toml::from_str(&content).expect("parse config");
    let preset = parsed
        .get("preset")
        .and_then(|v| v.as_str())
        .expect("preset key missing or wrong type");
    assert_eq!(preset, expected);
}

#[then(regex = r#"^the loaded config has src "([^"]+)"$"#)]
fn then_loaded_src(world: &mut InitWorld, expected: String) {
    let path = world.config_path("crap4rs.toml");
    let content = std::fs::read_to_string(&path).expect("read config");
    let parsed: toml::Value = toml::from_str(&content).expect("parse config");
    let src = parsed
        .get("src")
        .and_then(|v| v.as_str())
        .expect("src key missing or wrong type");
    assert_eq!(src, expected);
}

#[then(regex = r#"^stderr contains "([^"]+)"$"#)]
fn then_stderr_contains(world: &mut InitWorld, needle: String) {
    let stderr = world.stderr();
    assert!(
        stderr.contains(&needle),
        "stderr did not contain {needle:?}\nstderr:\n{stderr}",
    );
}

#[then(regex = r#"^stdout contains "([^"]+)"$"#)]
fn then_stdout_contains(world: &mut InitWorld, needle: String) {
    let stdout = String::from_utf8_lossy(&world.require_output().stdout);
    assert!(
        stdout.contains(&needle),
        "stdout did not contain {needle:?}\nstdout:\n{stdout}",
    );
}

#[then(regex = r"^the exit code is (\d+)$")]
fn then_exit_code(world: &mut InitWorld, expected: i32) {
    let actual = world
        .require_output()
        .status
        .code()
        .expect("process exited via signal");
    let stdout = String::from_utf8_lossy(&world.require_output().stdout);
    let stderr = world.stderr();
    assert_eq!(
        actual, expected,
        "exit code mismatch — expected {expected}, got {actual}\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
}

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // `writer::Libtest::or_basic()` mirrors `cli_ergonomics_cucumber`:
    // libtest-compatible JSON under nextest's `--list` probe, basic
    // writer for plain `cargo test`. `filter_run_and_exit` with the
    // `@wired` filter is the AGENTS.md § BDD hygiene rule 5 pattern.
    InitWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit("tests/features/cli_init.feature", |_, _, scenario| {
            scenario.tags.iter().any(|t| t == "wired")
        })
        .await;
}
