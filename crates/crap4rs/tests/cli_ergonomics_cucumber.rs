//! Cucumber-rs runner for `tests/features/cli_ergonomics.feature`
//! (issues #131, #168).
//!
//! Every scenario in `cli_ergonomics.feature` is `@wired`: the curated
//! BDD pass wired each CLI-level contract — flag-to-envelope echo,
//! clap/validate exit codes, the gate-keystone exit code, the table
//! "View:" subtitle line, and `--help` content — and pushed the view
//! SEMANTICS (sort/filter/truncate ordering, the result-block invariant)
//! down to crap-core's `domain::view` unit and property tests (see
//! `AGENTS.md` § BDD hygiene + `tests/features/TAGS.toml`). The harness
//! still uses `filter_run_and_exit` on `@wired` so a future `@unwired`
//! scenario is skipped until its step defs land.
//!
//! Each scenario sets up a tempdir with a small synthetic LCOV +
//! `src/lib.rs` and invokes the binary via `CARGO_BIN_EXE_crap4rs`. The
//! harness does NOT depend on the workspace's self-fixture LCOV, so paths
//! in scenario commands stay relative to the tempdir cwd; the `--help` /
//! `-h` scenarios run without a fixture (clap short-circuits before I/O).
//!
//! Three synthetic fixtures cover the pass / fail / range invariants:
//! - `WITHIN_*` — three trivial covered fns; every CRAP ≤ 2, so any
//!   reasonable threshold passes.
//! - `EXCEEDS_*` — three branchy uncovered fns; CRAPs roughly 20+, so
//!   `--threshold 5` deliberately trips the gate.
//! - `MIXED_*` — six functions (three of each), so truncation,
//!   filtering, and `view.eligible_count` are observable.

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

// Six functions spanning the CRAP range: three trivial+covered (CRAP ≤ 2)
// and three branchy+uncovered (CRAP > 5). Used by the --top scenarios,
// which need an eligible count (6) larger than the truncation limit so
// `view.truncated` is observably true. At `--threshold 5` exactly the
// three branchy functions exceed, which the keystone exit-code scenario
// relies on.
const MIXED_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn passing_c() -> i32 { 3 }
pub fn failing_a(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_b(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_c(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
";

const MIXED_LCOV: &str = "\
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

    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.require_output().stderr).into_owned()
    }

    /// Parse stdout as a JSON envelope (`--format json`). Panics with the
    /// raw stdout if it is not valid JSON, so a malformed envelope fails
    /// the scenario loudly rather than silently.
    fn json(&self) -> serde_json::Value {
        let out = self.stdout();
        serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nraw stdout:\n{out}"))
    }
}

/// Navigate a dotted object path (e.g. `view.spec.limit`) into a JSON
/// value. Panics naming the missing key if the path does not resolve, so
/// a renamed envelope field surfaces as a clear scenario failure.
fn json_at<'a>(root: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    let mut cur = root;
    for key in path.split('.') {
        cur = cur.get(key).unwrap_or_else(|| {
            panic!("JSON path {path:?} missing at key {key:?}; envelope:\n{root:#}")
        });
    }
    cur
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

// ── Given steps ──────────────────────────────────────────────────────

#[given("a synthetic project where every function is within threshold")]
fn given_within(world: &mut CliWorld) {
    setup_project(world, WITHIN_SRC, WITHIN_LCOV);
}

#[given("a synthetic project where at least one function exceeds threshold")]
fn given_exceeds(world: &mut CliWorld) {
    setup_project(world, EXCEEDS_SRC, EXCEEDS_LCOV);
}

#[given("a synthetic project with six functions spanning the CRAP range")]
fn given_six(world: &mut CliWorld) {
    setup_project(world, MIXED_SRC, MIXED_LCOV);
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

#[then(regex = r#"^stderr contains "([^"]+)"$"#)]
fn then_stderr_contains(world: &mut CliWorld, needle: String) {
    let stderr = world.stderr();
    assert!(
        stderr.contains(&needle),
        "stderr did not contain {needle:?}:\nstderr:\n{stderr}"
    );
}

/// Assert the `--help` text shows a command example, tolerant of clap's
/// width-based line wrapping: the after-help block is re-wrapped to the
/// terminal width, so a long example can split across lines (e.g.
/// `… --top 10\n      --no-fail`). Collapses every run of whitespace to a
/// single space on both sides before matching, so the assertion pins the
/// example's content without depending on the runner's column width.
#[then(regex = r#"^the help text shows the example "([^"]+)"$"#)]
fn then_help_shows_example(world: &mut CliWorld, example: String) {
    let collapse = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let stdout = collapse(&world.stdout());
    let want = collapse(&example);
    assert!(
        stdout.contains(&want),
        "help did not contain example {want:?} (whitespace-normalized):\nstdout:\n{stdout}"
    );
}

/// Assert a flag appears under a named `--help` heading (e.g.
/// `--only-failing` under `Filtering`). Extracts the section between the
/// `<heading>:` line and the next top-level heading, so this pins clap's
/// `next_help_heading` grouping — the V1b relocation of `--only-failing`
/// from the Output group to Filtering, observable only through `--help`.
#[then(regex = r#"^the help text lists "([^"]+)" under the "([^"]+)" heading$"#)]
fn then_help_lists_under_heading(world: &mut CliWorld, flag: String, heading: String) {
    let stdout = world.stdout();
    let lines: Vec<&str> = stdout.lines().collect();
    let header = format!("{heading}:");
    let start = lines
        .iter()
        .position(|l| l.trim_end() == header)
        .unwrap_or_else(|| panic!("help has no {header:?} heading:\nstdout:\n{stdout}"));
    // A top-level heading is a non-indented line ending in ':'.
    let is_heading = |l: &str| !l.starts_with(char::is_whitespace) && l.trim_end().ends_with(':');
    let end = lines[start + 1..]
        .iter()
        .position(|l| is_heading(l))
        .map(|off| start + 1 + off)
        .unwrap_or(lines.len());
    let section = lines[start..end].join("\n");
    assert!(
        section.contains(&flag),
        "{flag:?} not found under the {header:?} heading:\n{section}"
    );
}

/// Assert a scalar JSON envelope field. The expected token is parsed as a
/// JSON literal so `3`, `null`, `true`, and `false` all compare by value
/// against the envelope — e.g. `view.spec.limit is null` vs `is 3`.
#[then(regex = r#"^the JSON envelope at "([^"]+)" is (.+)$"#)]
fn then_envelope_is(world: &mut CliWorld, path: String, expected: String) {
    let root = world.json();
    let actual = json_at(&root, &path);
    let want: serde_json::Value = serde_json::from_str(&expected)
        .unwrap_or_else(|e| panic!("expected literal {expected:?} is not valid JSON: {e}"));
    assert_eq!(
        *actual, want,
        "JSON path {path:?}: expected {want}, got {actual}"
    );
}

#[then(regex = r#"^the JSON envelope at "([^"]+)" has (\d+) entr(?:y|ies)$"#)]
fn then_envelope_len(world: &mut CliWorld, path: String, n: usize) {
    let root = world.json();
    let arr = json_at(&root, &path)
        .as_array()
        .unwrap_or_else(|| panic!("JSON path {path:?} is not an array; envelope:\n{root:#}"));
    assert_eq!(
        arr.len(),
        n,
        "JSON path {path:?}: expected {n} entries, got {}",
        arr.len()
    );
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
    // only `@wired`-tagged scenarios; `@unwired` scenarios are
    // aspirational specs tracked via the umbrella issue (see
    // `AGENTS.md` § BDD hygiene). Tags inside `sc.tags` are stored
    // without the `@` prefix — verified empirically.
    //
    // `run_and_exit` (vs `run`) panics on scenario failure, propagating
    // a non-zero exit to CI — see memory `cucumber-run-vs-run-and-exit`.
    CliWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit("tests/features/cli_ergonomics.feature", |_, _, scenario| {
            scenario.tags.iter().any(|t| t == "wired")
        })
        .await;
}
