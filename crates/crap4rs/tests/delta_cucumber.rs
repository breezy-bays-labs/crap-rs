//! Cucumber-rs runner for `@wired` scenarios in
//! `tests/features/delta.feature` (issue #81).
//!
//! delta.feature's CLI-acceptance contracts — `--baseline` resolution,
//! the gate-semantics exit-code matrix, reporter rendering, the additive
//! JSON envelope shape, the shaping flags (`--delta-top` / `--delta-sort`
//! / `--delta-only`), validation exit codes, `--help` discoverability, and
//! identity / relocation through the real pipeline — are wired here. The
//! DOMAIN behaviors (FunctionChange classification, the matcher's
//! identity / rename pairing logic, new-violation counting,
//! threshold-border epsilon math, and DeltaViewSpec filter/sort/truncate)
//! live in crap-core's `domain::delta` unit and property tests; this
//! harness pins only what needs the real binary process: that the flags
//! wire end-to-end, the exit codes are right, and each reporter surfaces
//! the delta (see `AGENTS.md` § BDD hygiene + `tests/features/TAGS.toml`).
//! Every scenario in delta.feature is now `@wired` (curated-pass slices
//! 1 + 2); the harness still uses `filter_run_and_exit` on `@wired` so a
//! future `@unwired` scenario is skipped until its step defs land.
//!
//! Each scenario captures a baseline envelope from one source snapshot,
//! optionally mutates the source to a "current" snapshot, then runs the
//! binary with `--baseline baseline.json`. Fixtures are synthetic LCOV +
//! `src/*.rs` in a tempdir, invoked via `CARGO_BIN_EXE_crap4rs` — no
//! dependency on the workspace self-fixture, so paths in scenario
//! commands stay relative to the tempdir cwd.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cucumber::{World, given, then, when, writer};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

// ── Fixtures ──────────────────────────────────────────────────────────
// Two ranges of CRAP: trivial+covered functions stay ≤ 2; branchy+
// uncovered functions land ~20. A `--threshold 5` cleanly separates the
// two bands; a `--threshold 1000` makes the whole-project analysis gate
// pass so a scenario can isolate delta-gate behavior.

/// Three trivial covered functions — every CRAP ≤ 2. As a baseline they
/// pair (by identity) with the same three functions in [`SIX_MIXED_SRC`],
/// so growing the project to six introduces exactly three Added rows.
const THREE_PASSING_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn passing_c() -> i32 { 3 }
";

const THREE_PASSING_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
DA:3,1
end_of_record
";

/// Six functions: the three trivial passing ones plus three branchy
/// uncovered ones (CRAP > 5). At `--threshold 5` exactly the three
/// branchy functions exceed.
const SIX_MIXED_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn passing_c() -> i32 { 3 }
pub fn failing_a(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_b(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_c(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
";

const SIX_MIXED_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
DA:3,1
DA:4,0
DA:5,0
DA:6,0
end_of_record
";

/// Baseline for the reporter scenarios: two functions, `first` and
/// `second`.
const DRIFT_BASE_SRC: &str = "\
pub fn first() -> i32 { 1 }
pub fn second(x: i32) -> i32 { if x > 0 { 1 } else { 2 } }
";

const DRIFT_BASE_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
end_of_record
";

/// Current snapshot paired with [`DRIFT_BASE_SRC`]: `first` removed,
/// `second` modified (a new branch + a line shift), `third` added — so
/// the delta carries one Removed, one Modified, and one Added row, which
/// the reporter scenarios assert all three kinds are rendered.
const DRIFT_CUR_SRC: &str = "\
// extra leading line to shift line numbers
pub fn second(x: i32) -> i32 {
    if x > 0 {
        if x > 5 { 1 } else { 2 }
    } else {
        3
    }
}
pub fn third() -> i32 { 42 }
";

const DRIFT_CUR_LCOV: &str = "\
SF:lib.rs
DA:2,0
DA:3,0
DA:4,0
DA:5,0
DA:6,0
DA:7,0
DA:8,0
DA:9,1
end_of_record
";

/// A complexity-4 function. Fully covered it scores CRAP `4.0`; fully
/// uncovered it scores `4² + 4 = 20.0`. Reused byte-identically between
/// the epsilon baseline and current snapshots so only coverage moves —
/// driving the CRAP score across a threshold of 12, with both readings
/// inside a ±10 border band.
const CLASSIFY_SRC: &str = "\
pub fn classify(x: i32) -> i32 {
    if x > 0 {
        if x > 5 { 1 } else { 2 }
    } else {
        3
    }
}
";

/// LCOV for [`CLASSIFY_SRC`] with every line covered (`hits = 1`) or
/// uncovered (`hits = 0`).
fn classify_lcov(hits: u32) -> String {
    let mut s = String::from("SF:lib.rs\n");
    for line in 1..=7 {
        s.push_str(&format!("DA:{line},{hits}\n"));
    }
    s.push_str("end_of_record\n");
    s
}

/// A branchy, fully-uncovered function — CRAP `c² + c`, comfortably over
/// a threshold of 5 wherever it lives. Reused byte-identically across two
/// files so the relocation pass pairs it as a single `Renamed` change.
const RELOCATED_FN: &str = "\
pub fn process(x: i32) -> i32 {
    if x > 0 {
        if x > 5 { 1 } else { 2 }
    } else {
        3
    }
}
";

/// LCOV marking every line of [`RELOCATED_FN`] uncovered for `file`, so
/// the function scores identically (and over threshold) wherever it lives.
fn relocated_lcov(file: &str) -> String {
    format!("SF:{file}\nDA:1,0\nDA:2,0\nDA:3,0\nDA:4,0\nDA:5,0\nDA:6,0\nDA:7,0\nend_of_record\n")
}

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

/// Navigate a dotted object path (e.g. `delta.summary.passed`) into a
/// JSON value. Panics naming the missing key if the path does not
/// resolve, so a renamed envelope field surfaces as a clear failure.
fn json_at<'a>(root: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    let mut cur = root;
    for key in path.split('.') {
        cur = cur.get(key).unwrap_or_else(|| {
            panic!("JSON path {path:?} missing at key {key:?}; envelope:\n{root:#}")
        });
    }
    cur
}

fn write_fixture(dir: &Path, src: &str, lcov: &str) {
    std::fs::create_dir_all(dir.join("src")).expect("create src dir");
    std::fs::write(dir.join("src/lib.rs"), src).expect("write src/lib.rs");
    std::fs::write(dir.join("lcov.info"), lcov).expect("write lcov.info");
}

fn setup_project(world: &mut CliWorld, src: &str, lcov: &str) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    write_fixture(&path, src, lcov);
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

/// Overwrite the current project's source + coverage in place (the
/// "current" snapshot after a baseline has been captured).
fn mutate_project(world: &mut CliWorld, src: &str, lcov: &str) {
    let dir = world.require_dir().to_path_buf();
    write_fixture(&dir, src, lcov);
}

/// Write a single named source file (plus `lcov.info`) into the project's
/// `src/` dir — for relocation fixtures where the file name itself is the
/// identity that moves.
fn write_single_fn(dir: &Path, file_name: &str, src: &str, lcov: &str) {
    std::fs::create_dir_all(dir.join("src")).expect("create src dir");
    std::fs::write(dir.join("src").join(file_name), src).expect("write src file");
    std::fs::write(dir.join("lcov.info"), lcov).expect("write lcov.info");
}

/// Run the binary at the captured-baseline threshold and persist its JSON
/// stdout as `baseline.json` in the project dir. `--no-fail` keeps the
/// capture exit code 0 regardless of the baseline's own violations;
/// `verbose` adds `--verbose` so the baseline envelope carries a
/// `diagnostics` block (for the propagation scenario).
fn capture_baseline_inner(world: &mut CliWorld, threshold: &str, verbose: bool) {
    let dir = world.require_dir();
    let mut args = vec![
        "--coverage",
        "lcov.info",
        "--src",
        "src",
        "--no-gitignore",
        "--format",
        "json",
        "--threshold",
        threshold,
        "--no-fail",
    ];
    if verbose {
        args.push("--verbose");
    }
    let output = Command::new(BINARY)
        .current_dir(dir)
        .args(&args)
        .output()
        .expect("failed to run crap4rs to capture baseline");
    // Fail fast on a non-zero capture: `--no-fail` keeps a violation-bearing
    // baseline at exit 0, so any non-zero status here is a genuine setup
    // failure (a panic, or an exit-2 validation error). Writing its stdout
    // to baseline.json anyway would surface later as a confusing JSON-parse
    // or empty-baseline error in the scenario's `--baseline` run.
    assert!(
        output.status.success(),
        "baseline capture failed (status {:?})\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::write(dir.join("baseline.json"), &output.stdout).expect("write baseline.json");
}

/// Capture a baseline envelope (no `--verbose`) — the common case.
fn capture_baseline(world: &mut CliWorld, threshold: &str) {
    capture_baseline_inner(world, threshold, false);
}

/// Parse a backtick-wrapped `crap4rs ...` command into the args vec
/// (drops the binary name). Returns args for `Command::args(&args)`.
fn parse_command(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().skip(1).map(str::to_string).collect()
}

// ── Given steps ──────────────────────────────────────────────────────

#[given(regex = r#"^a baseline of three passing functions captured at threshold (\d+)$"#)]
fn given_baseline_three_passing(world: &mut CliWorld, threshold: String) {
    setup_project(world, THREE_PASSING_SRC, THREE_PASSING_LCOV);
    capture_baseline(world, &threshold);
}

#[given(regex = r#"^a baseline of six functions \(three exceeding\) captured at threshold (\d+)$"#)]
fn given_baseline_six(world: &mut CliWorld, threshold: String) {
    setup_project(world, SIX_MIXED_SRC, SIX_MIXED_LCOV);
    capture_baseline(world, &threshold);
}

#[given(regex = r#"^a baseline of two functions captured at threshold (\d+)$"#)]
fn given_baseline_two(world: &mut CliWorld, threshold: String) {
    setup_project(world, DRIFT_BASE_SRC, DRIFT_BASE_LCOV);
    capture_baseline(world, &threshold);
}

#[given(regex = r#"^a baseline where one covered function scores below threshold (\d+)$"#)]
fn given_baseline_classify(world: &mut CliWorld, threshold: String) {
    setup_project(world, CLASSIFY_SRC, &classify_lcov(1));
    capture_baseline(world, &threshold);
}

#[given("a synthetic project with six functions and no baseline")]
fn given_six_no_baseline(world: &mut CliWorld) {
    setup_project(world, SIX_MIXED_SRC, SIX_MIXED_LCOV);
}

#[given("the project then adds the three exceeding functions")]
fn given_current_adds_failing(world: &mut CliWorld) {
    mutate_project(world, SIX_MIXED_SRC, SIX_MIXED_LCOV);
}

#[given("the project is left unchanged")]
fn given_current_unchanged(_world: &mut CliWorld) {
    // The baseline was captured from the current source; no mutation.
}

#[given("the project drops one function, modifies another, and adds a third")]
fn given_current_drift(world: &mut CliWorld) {
    mutate_project(world, DRIFT_CUR_SRC, DRIFT_CUR_LCOV);
}

#[given("that function then becomes fully uncovered")]
fn given_current_uncovered(world: &mut CliWorld) {
    mutate_project(world, CLASSIFY_SRC, &classify_lcov(0));
}

#[given(regex = r#"^a baseline captured with diagnostics at threshold (\d+)$"#)]
fn given_baseline_with_diagnostics(world: &mut CliWorld, threshold: String) {
    setup_project(world, DRIFT_BASE_SRC, DRIFT_BASE_LCOV);
    capture_baseline_inner(world, &threshold, true);
}

#[given("a project with a malformed baseline file present")]
fn given_malformed_baseline(world: &mut CliWorld) {
    setup_project(world, SIX_MIXED_SRC, SIX_MIXED_LCOV);
    let dir = world.require_dir().to_path_buf();
    std::fs::write(dir.join("bad.json"), "not json{").expect("write bad.json");
}

#[given("a project with a baseline declaring an unsupported schema_version")]
fn given_unsupported_schema_baseline(world: &mut CliWorld) {
    setup_project(world, SIX_MIXED_SRC, SIX_MIXED_LCOV);
    let dir = world.require_dir().to_path_buf();
    std::fs::write(
        dir.join("future.json"),
        r#"{
            "schema_version": 99,
            "result": {
                "functions": [],
                "summary": {
                    "total_functions": 0, "total_files": 0,
                    "exceeding_threshold": 0,
                    "average_crap": 0.0, "median_crap": 0.0,
                    "max_crap": null, "worst_function": null,
                    "distribution": {"low":0,"acceptable":0,"moderate":0,"high":0}
                },
                "passed": true
            }
        }"#,
    )
    .expect("write future.json");
}

#[given("the baseline metric field is then stripped")]
fn given_strip_baseline_metric(world: &mut CliWorld) {
    let path = world.require_dir().join("baseline.json");
    let stripped = std::fs::read_to_string(&path)
        .expect("read baseline.json")
        .replace("\"metric\": \"cognitive\",", "");
    assert!(
        !stripped.contains("\"metric\""),
        "fixture must actually omit the metric key after stripping"
    );
    std::fs::write(&path, stripped).expect("rewrite baseline.json without metric");
}

#[given(regex = r#"^a baseline with one function in old_mod\.rs captured at threshold (\d+)$"#)]
fn given_baseline_relocated(world: &mut CliWorld, threshold: String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    write_single_fn(
        &path,
        "old_mod.rs",
        RELOCATED_FN,
        &relocated_lcov("old_mod.rs"),
    );
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
    capture_baseline(world, &threshold);
}

#[given("the function relocates to new_mod.rs")]
fn given_current_relocates(world: &mut CliWorld) {
    let dir = world.require_dir().to_path_buf();
    std::fs::remove_file(dir.join("src").join("old_mod.rs")).expect("remove old_mod.rs");
    write_single_fn(
        &dir,
        "new_mod.rs",
        RELOCATED_FN,
        &relocated_lcov("new_mod.rs"),
    );
}

// ── When step ────────────────────────────────────────────────────────

#[when(regex = r#"^the operator runs `([^`]+)`$"#)]
fn when_run(world: &mut CliWorld, cmd: String) {
    let args = parse_command(&cmd);
    let needs_project = cmd.contains("--coverage") && !cmd.contains("--help");

    let mut command = Command::new(BINARY);
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

#[then(regex = r#"^stdout starts with "([^"]+)"$"#)]
fn then_starts_with(world: &mut CliWorld, prefix: String) {
    let stdout = world.stdout();
    assert!(
        stdout.starts_with(&prefix),
        "stdout did not start with {prefix:?}:\nstdout:\n{stdout}"
    );
}

#[then(regex = r#"^stderr contains "([^"]+)"$"#)]
fn then_stderr_contains(world: &mut CliWorld, needle: String) {
    let stderr = world.stderr();
    assert!(
        stderr.contains(&needle),
        "stderr did not contain {needle:?}:\nstderr:\n{stderr}"
    );
}

#[then(regex = r#"^stderr does not contain "([^"]+)"$"#)]
fn then_stderr_not_contains(world: &mut CliWorld, needle: String) {
    let stderr = world.stderr();
    assert!(
        !stderr.contains(&needle),
        "stderr unexpectedly contained {needle:?}:\nstderr:\n{stderr}"
    );
}

/// Assert a scalar JSON envelope field. The expected token is parsed as a
/// JSON literal so `1`, `2`, `null`, `true`, and `false` all compare by
/// value — e.g. `delta.summary.passed is false`, `schema_version is 2`.
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

/// Assert a path resolves to a JSON object (presence + shape). Used for
/// `delta`, `delta.summary`, `delta.spec` — the additive blocks whose
/// mere presence is the contract.
#[then(regex = r#"^the JSON envelope has a "([^"]+)" object$"#)]
fn then_envelope_object(world: &mut CliWorld, path: String) {
    let root = world.json();
    let node = json_at(&root, &path);
    assert!(
        node.is_object(),
        "JSON path {path:?} is not an object; got {node}"
    );
}

/// Assert a top-level key is absent from the envelope — the additive
/// contract that a run without `--baseline` omits the `delta` key
/// entirely (rather than emitting an empty / null block).
#[then(regex = r#"^the JSON envelope has no top-level "([^"]+)" key$"#)]
fn then_envelope_no_top_level(world: &mut CliWorld, key: String) {
    let root = world.json();
    assert!(
        root.get(&key).is_none(),
        "top-level key {key:?} unexpectedly present; envelope:\n{root:#}"
    );
}

/// Assert a path resolves to a non-empty JSON string — for the baseline
/// provenance fields (`delta.baseline_tool_version` / `_timestamp`),
/// whose exact value is environment-dependent but whose presence and
/// type are the contract.
#[then(regex = r#"^the JSON envelope at "([^"]+)" holds a non-empty string$"#)]
fn then_envelope_non_empty_string(world: &mut CliWorld, path: String) {
    let root = world.json();
    let node = json_at(&root, &path);
    let s = node
        .as_str()
        .unwrap_or_else(|| panic!("JSON path {path:?} is not a string; got {node}"));
    assert!(!s.is_empty(), "JSON path {path:?} is an empty string");
}

#[then(regex = r"^the exit code is (\d+)$")]
fn then_exit_code(world: &mut CliWorld, expected: i32) {
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

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // `writer::Libtest::or_basic()` emits libtest-compatible JSON under
    // nextest (which probes `--list`) and falls back to the basic writer
    // for plain `cargo test`. Matches `cli_ergonomics_cucumber`.
    //
    // `filter_run_and_exit` loads `delta.feature` but executes only
    // `@wired`-tagged scenarios; `@unwired` scenarios are aspirational
    // specs tracked via the umbrella issue (see `AGENTS.md` § BDD
    // hygiene). Tags inside `sc.tags` are stored without the `@` prefix.
    //
    // `run_and_exit` (vs `run`) panics on scenario failure, propagating a
    // non-zero exit to CI — see memory `cucumber-run-vs-run-and-exit`.
    CliWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit("tests/features/delta.feature", |_, _, scenario| {
            scenario.tags.iter().any(|t| t == "wired")
        })
        .await;
}
