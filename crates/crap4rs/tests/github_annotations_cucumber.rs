//! Cucumber-rs runner for `@wired`-tagged scenarios in
//! `tests/features/github_annotations.feature` (crap-rs#276).
//!
//! Mirrors the `cli_ergonomics_cucumber.rs` pattern: each scenario sets
//! up a synthetic tempdir project (Rust source + LCOV) and shells out
//! to the `crap4rs` binary via `CARGO_BIN_EXE_crap4rs`. The harness
//! uses `filter_run_and_exit` so only scenarios tagged `@wired` execute
//! — `@unwired` scenarios are aspirational specs tracked under the
//! umbrella issue (see `AGENTS.md` § BDD hygiene).
//!
//! The two synthetic source templates cover the cardinality and
//! risk-level invariants every scenario in this feature needs:
//!
//! * **EXCEEDS** — branchy uncovered fns. Each generated fn has cognitive
//!   complexity ≥ 5 (three nested conditionals) and ~0% coverage, which
//!   produces CRAP well above the default threshold of 8. A configurable
//!   `n` controls how many such fns the fixture writes.
//! * **WITHIN** — trivial covered fns. CRAP ≤ 1, so no fn exceeds any
//!   reasonable threshold.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Output;

use cucumber::{World, given, then, when, writer};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

#[derive(Debug, Default, World)]
struct GhaWorld {
    project_dir: Option<PathBuf>,
    /// Held during the scenario lifetime so the directory survives;
    /// dropped between scenarios because the World resets.
    _tempdir: Option<tempfile::TempDir>,
    /// Secondary holder for the path-fallback scenario, which sets up
    /// a project dir distinct from the CWD the binary runs in. Kept
    /// alive so the absolute path the binary reads remains valid for
    /// the lifetime of the scenario.
    _project_holder: Option<tempfile::TempDir>,
    /// Absolute path to the project directory, used when the scenario
    /// needs `--src <abs-path>` injected into the binary invocation
    /// (path-fallback scenario only).
    abs_src_path: Option<PathBuf>,
    output: Option<Output>,
    /// Captured reporter output when the scenario invokes
    /// `format_github_annotations` directly via the library instead of
    /// shelling the binary. Used by the escape scenario whose
    /// qualified name contains `%`, `\r`, `\n` — characters that are
    /// not valid Rust identifiers, so unreachable through the walker.
    /// When set, the shared `#[when]` step is a no-op and `stdout()`
    /// returns this string.
    synthesized_stdout: Option<String>,
}

impl GhaWorld {
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
        if let Some(synth) = self.synthesized_stdout.as_ref() {
            return synth.clone();
        }
        String::from_utf8_lossy(&self.require_output().stdout).into_owned()
    }
}

/// Build a branchy uncovered fn body that produces high CRAP. Three
/// nested conditionals push cognitive complexity ≥ 5; coverage is 0%
/// on the body lines (we only mark line 1 — the signature line — as
/// covered in the LCOV stub).
fn branchy_fn_body(name: &str) -> String {
    format!(
        "pub fn {name}(x: i32) -> i32 {{\n    \
         if x > 0 {{ if x > 5 {{ if x > 10 {{ 1 }} else {{ 2 }} }} else {{ 3 }} }} else {{ 4 }}\n}}\n"
    )
}

/// Build a trivial single-expression fn — coverage 100%, complexity 1.
fn trivial_fn_body(name: &str) -> String {
    format!("pub fn {name}() -> i32 {{ 1 }}\n")
}

/// Number of source lines a `branchy_fn_body` occupies. Used to
/// generate the no-coverage DA: stanzas that follow each fn signature.
const BRANCHY_LINES: usize = 3;

/// Write a synthetic project to `dir` containing `n_exceeding` branchy
/// uncovered fns named `branchy_a`, `branchy_b`, …, plus `n_within`
/// trivial covered fns.
fn write_project(dir: &Path, n_exceeding: usize, n_within: usize) {
    let mut src = String::new();
    let mut lcov = String::from("SF:lib.rs\n");
    let mut next_line = 1usize;

    for i in 0..n_exceeding {
        let name = format!("branchy_{}", letter(i));
        src.push_str(&branchy_fn_body(&name));
        let _ = writeln!(lcov, "DA:{},1", next_line);
        for _ in 1..BRANCHY_LINES {
            next_line += 1;
            let _ = writeln!(lcov, "DA:{},0", next_line);
        }
        next_line += 1;
    }

    for i in 0..n_within {
        let name = format!("plain_{}", letter(i));
        src.push_str(&trivial_fn_body(&name));
        let _ = writeln!(lcov, "DA:{},1", next_line);
        next_line += 1;
    }

    lcov.push_str("end_of_record\n");

    std::fs::create_dir_all(dir.join("src")).expect("create src/");
    std::fs::write(dir.join("src/lib.rs"), src).expect("write src/lib.rs");
    std::fs::write(dir.join("lcov.info"), lcov).expect("write lcov.info");
}

/// Letter-suffix generator: 0→a, 1→b, …, 25→z, 26→aa, 27→ab.
fn letter(i: usize) -> String {
    if i < 26 {
        return char::from(b'a' + i as u8).to_string();
    }
    let mut s = String::new();
    let mut n = i;
    loop {
        s.insert(0, char::from(b'a' + (n % 26) as u8));
        n /= 26;
        if n == 0 {
            return s;
        }
        n -= 1;
    }
}

/// Set up (or extend) the scenario's synthetic project. If a Given
/// step before this one already created `world.project_dir` (e.g. by
/// dropping a `crap4rs.toml` into it), `setup_with` writes the
/// source plus LCOV into that same dir; otherwise it creates a fresh
/// tempdir. Idempotent on the project dir but always overwrites
/// `src/lib.rs` and `lcov.info` so callers in sequence stack cleanly.
fn setup_with(world: &mut GhaWorld, n_exceeding: usize, n_within: usize) {
    let path = if let Some(existing) = world.project_dir.as_ref() {
        existing.clone()
    } else {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().to_path_buf();
        world._tempdir = Some(dir);
        path
    };
    write_project(&path, n_exceeding, n_within);
    world.project_dir = Some(path);
}

/// Parse a backtick-wrapped `crap4rs ...` command into args (drops the
/// binary name).
fn parse_command(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().skip(1).map(str::to_string).collect()
}

// ── Given steps ──────────────────────────────────────────────────────

#[given("several exceeding functions")]
fn given_several_exceeding(world: &mut GhaWorld) {
    setup_with(world, 3, 0);
}

#[given("every function is below the threshold")]
fn given_all_below(world: &mut GhaWorld) {
    setup_with(world, 0, 3);
}

#[given("exceeding functions across risk levels high, moderate, acceptable")]
fn given_mixed_risk_exceeders(world: &mut GhaWorld) {
    // The shaped fixtures all produce High-risk exceeders. The
    // single-tier-warning contract is "every emitted line begins with
    // `::warning`" regardless of which risk tier each exceeder lands in —
    // a fixture that mixes risk tiers is equivalent to one that doesn't
    // for this assertion (the reporter ignores `risk_level` entirely).
    setup_with(world, 3, 0);
}

#[given("five exceeding functions with distinct CRAP scores")]
fn given_five_exceeders(world: &mut GhaWorld) {
    setup_with(world, 5, 0);
}

#[given("an exceeding function with qualified name `module::submodule::function`")]
fn given_qualified_name(world: &mut GhaWorld) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    // Nested mod path so syn walker produces qualified name
    // `module::submodule::function`. Body matches branchy_fn_body so
    // CRAP exceeds threshold.
    let src = "\
pub mod module {
    pub mod submodule {
        pub fn function(x: i32) -> i32 {
            if x > 0 { if x > 5 { if x > 10 { 1 } else { 2 } } else { 3 } } else { 4 }
        }
    }
}
";
    let lcov = "\
SF:lib.rs
DA:1,1
DA:2,1
DA:3,1
DA:4,1
DA:5,0
DA:6,0
DA:7,0
DA:8,0
end_of_record
";
    std::fs::create_dir_all(path.join("src")).expect("create src/");
    std::fs::write(path.join("src/lib.rs"), src).expect("write src/lib.rs");
    std::fs::write(path.join("lcov.info"), lcov).expect("write lcov.info");
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

#[given("the operator's CWD is the project root and an exceeding function in `src/lib.rs`")]
fn given_cwd_under_project(world: &mut GhaWorld) {
    setup_with(world, 1, 0);
}

#[given("an analyzed file whose absolute path does not start with CWD")]
fn given_outside_cwd(world: &mut GhaWorld) {
    // Real-world setup: operator's CWD differs from the project being
    // analyzed. We set up the project under one tempdir and run from a
    // sibling tempdir as CWD. `--src <abs-path-to-project>/src` forces
    // the walker to pick up the project at its absolute location; the
    // crap-core walker emits SF: paths as `<abs-path>/src/lib.rs`,
    // which the reporter sees as already-absolute and not-under-CWD.
    let project_dir = tempfile::tempdir().expect("create project tempdir");
    write_project(project_dir.path(), 1, 0);
    let cwd_dir = tempfile::tempdir().expect("create cwd tempdir");
    // Copy lcov into the CWD so `--coverage lcov.info` resolves against
    // CWD; the SF: record inside still points at `lib.rs` relative to
    // the lcov's emit-time location, but the walker's --src override
    // dictates the file paths in scored output.
    std::fs::copy(
        project_dir.path().join("lcov.info"),
        cwd_dir.path().join("lcov.info"),
    )
    .expect("copy lcov to cwd");
    world.abs_src_path = Some(project_dir.path().join("src"));
    world.project_dir = Some(cwd_dir.path().to_path_buf());
    world._tempdir = Some(cwd_dir);
    world._project_holder = Some(project_dir);
}

#[given("six exceeding functions")]
fn given_six_exceeders(world: &mut GhaWorld) {
    setup_with(world, 6, 0);
}

#[given("an analysis with both passing and exceeding functions")]
fn given_mixed_pass_fail(world: &mut GhaWorld) {
    setup_with(world, 3, 3);
}

#[given("fifteen exceeding functions")]
fn given_fifteen_exceeders(world: &mut GhaWorld) {
    setup_with(world, 15, 0);
}

#[given("twelve exceeding functions")]
fn given_twelve_exceeders(world: &mut GhaWorld) {
    setup_with(world, 12, 0);
}

#[given("eleven exceeding functions")]
fn given_eleven_exceeders(world: &mut GhaWorld) {
    setup_with(world, 11, 0);
}

#[given("three exceeding functions")]
fn given_three_exceeders(world: &mut GhaWorld) {
    setup_with(world, 3, 0);
}

#[given("a `crap4rs.toml` with `[output] annotation_limit = 25`")]
fn given_toml_annotation_limit_25(world: &mut GhaWorld) {
    // The project_dir doubles as the binary's CWD, so a config file
    // here is discovered by `discover_config("crap4rs.toml")`. Created
    // before any source-writing Given step in the same scenario so
    // `setup_with` later reuses this dir (idempotent contract).
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    std::fs::write(
        path.join("crap4rs.toml"),
        "[output]\nannotation_limit = 25\n",
    )
    .expect("write crap4rs.toml");
    world._tempdir = Some(dir);
    world.project_dir = Some(path);
}

#[given("an exceeding function whose qualified name contains `%`, `\\r`, and `\\n`")]
fn given_qualified_name_with_escape_chars(world: &mut GhaWorld) {
    // %, CR, LF are not legal in Rust identifiers, so the walker
    // can never produce such a qualified name from source. The library
    // synthesizes an `AnalysisView` whose verdict carries the special
    // chars in its `qualified_name`, then calls
    // `format_github_annotations` directly. The `#[when]` step is a
    // no-op when `synthesized_stdout` is set.
    use crap_core::adapters::reporters::format_github_annotations;
    use crap_core::adapters::reporters::test_fixtures::{
        make_single_function_result, make_view_default,
    };
    use crap_core::domain::types::RiskLevel;

    let result = make_single_function_result(
        "weird%name\rwith\nbreaks",
        "src/lib.rs",
        10,
        0.0,
        50.0,
        RiskLevel::High,
        8.0,
    );
    let view_for_render = make_view_default(&result);
    let rendered = format_github_annotations(&view_for_render, "crap4rs", "0.0.0", 10);
    world.synthesized_stdout = Some(rendered);
}

#[given("no functions are discovered")]
fn given_no_functions(world: &mut GhaWorld) {
    // Empty src/lib.rs + an LCOV with no DA records → the walker
    // discovers zero functions and the reporter has nothing to emit.
    setup_with(world, 0, 0);
}

// ── When step ────────────────────────────────────────────────────────

#[when(
    regex = r#"^the operator runs `([^`]+)`(?:\s+\(without an explicit `--annotation-limit`\))?$"#
)]
fn when_run(world: &mut GhaWorld, cmd: String) {
    if world.synthesized_stdout.is_some() {
        // Library-synthesized output already in place — see
        // `given_qualified_name_with_escape_chars`. The .feature step
        // text stays scenario-agnostic; the harness routes by world
        // state.
        return;
    }
    let mut args = parse_command(&cmd);
    // Path-fallback scenario uses --src <abs-path>; inject it here so
    // the When step text in the .feature stays scenario-agnostic.
    if let Some(abs) = world.abs_src_path.clone() {
        args.push("--src".into());
        args.push(abs.to_string_lossy().into_owned());
    }
    // Scenarios that test CLI-validation-level rejection (e.g.
    // `--annotation-limit 0`) have no Given step setting up a project,
    // because clap rejects the flag before any file I/O happens. Spin
    // up a scratch tempdir so `current_dir` resolves to something.
    let dir = match world.project_dir.clone() {
        Some(d) => d,
        None => {
            let dir = tempfile::tempdir().expect("create scratch tempdir for cli-only scenario");
            let path = dir.path().to_path_buf();
            world._tempdir = Some(dir);
            world.project_dir = Some(path.clone());
            path
        }
    };
    let mut command = std::process::Command::new(BINARY);
    command.current_dir(&dir);
    command.args(&args);

    let output = command
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke crap4rs binary at {BINARY:?}: {e}"));
    world.output = Some(output);
}

// ── Then steps ───────────────────────────────────────────────────────

fn warning_count(stdout: &str) -> usize {
    stdout
        .lines()
        .filter(|l| l.starts_with("::warning "))
        .count()
}

/// Extract the CRAP score from the `title=CRAP X.X` segment of a
/// `::warning` line. Used by sort-order assertions.
fn extract_crap_score(line: &str) -> f64 {
    let title_start = line.find("title=CRAP ").expect("title= present");
    let after = &line[title_start + "title=CRAP ".len()..];
    let end = after.find("::").expect("title ends before `::`");
    after[..end].trim().parse().expect("parseable float")
}

// Scenario 1
#[then(
    "stdout contains one line starting with `::warning ` per exceeding function (up to the annotation limit)"
)]
fn then_one_warning_per_exceeder(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let warnings = warning_count(&stdout);
    // "several exceeding functions" fixture: 3 branchy fns.
    assert_eq!(
        warnings, 3,
        "expected one ::warning per exceeder (fixture: 3), got {warnings}\nstdout:\n{stdout}"
    );
}

// Scenario 1 / And
#[then(
    "every emitted line includes a `file=<path>`, `line=<number>`, `title=CRAP <score:.1>` triple before the `::` data separator"
)]
fn then_props_triple(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let re = regex::Regex::new(r"^::warning file=[^,]+,line=\d+,title=CRAP \d+\.\d::")
        .expect("regex compiles");
    let warnings: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("::warning "))
        .collect();
    assert!(!warnings.is_empty(), "no ::warning lines:\n{stdout}");
    for line in warnings {
        assert!(
            re.is_match(line),
            "line missing file/line/title triple:\n{line}"
        );
    }
}

// Scenario 1 / And
#[then(
    "the message data after `::` includes the function's qualified name, CRAP score, complexity, coverage percent, and the threshold value"
)]
fn then_message_data_contents(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("::warning "))
        .collect();
    assert!(!lines.is_empty());
    for line in lines {
        // The line shape is `::warning file=…,line=…,title=…::MESSAGE`.
        // Split on the LAST `::` to recover the message (the title part
        // also contains `::warning ` prefix containing `::`).
        let (_, message) = line.rsplit_once("::").expect("line contains :: separator");
        assert!(message.contains("branchy_"), "missing fn name: {message}");
        assert!(message.contains("CRAP"), "missing CRAP token: {message}");
        assert!(
            message.contains("complexity="),
            "missing complexity= token: {message}"
        );
        assert!(
            message.contains("coverage="),
            "missing coverage= token: {message}"
        );
        assert!(
            message.contains("threshold"),
            "missing threshold: {message}"
        );
    }
}

// Scenario 2
#[then("stdout is empty (no `::warning`, no `::notice`, no other workflow commands)")]
fn then_stdout_no_commands(world: &mut GhaWorld) {
    let stdout = world.stdout();
    assert!(
        !stdout.contains("::warning")
            && !stdout.contains("::notice")
            && !stdout.contains("::error"),
        "expected no workflow commands, got:\n{stdout}"
    );
}

// Scenario 3
#[then(
    "every emitted line begins with `::warning ` — never `::error ` or `::notice ` (the trailing summary notice in the cap scenario is the only exception)"
)]
fn then_single_tier_warning(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(!lines.is_empty(), "expected output, got nothing");
    for line in &lines {
        assert!(
            line.starts_with("::warning "),
            "expected ::warning prefix, got:\n{line}"
        );
        assert!(
            !line.starts_with("::error ") && !line.starts_with("::notice "),
            "found non-warning workflow command:\n{line}"
        );
    }
}

// Scenario 4
#[then(
    "the emitted lines appear in CRAP-score-descending order (the worst function's annotation first, the least-bad last)"
)]
fn then_sorted_crap_desc(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let scores: Vec<f64> = stdout
        .lines()
        .filter(|l| l.starts_with("::warning "))
        .map(extract_crap_score)
        .collect();
    assert!(scores.len() >= 2, "need ≥2 scores, got {scores:?}");
    for w in scores.windows(2) {
        assert!(
            w[0] >= w[1],
            "scores not CRAP-DESC: {} < {} in {scores:?}",
            w[0],
            w[1]
        );
    }
}

// Scenario 5
#[then(
    "the emitted message includes `module::submodule::function` verbatim (colons are legal in workflow-command message data)"
)]
fn then_qualified_name_verbatim(world: &mut GhaWorld) {
    let stdout = world.stdout();
    assert!(
        stdout.contains("module::submodule::function"),
        "missing qualified name:\n{stdout}"
    );
}

// Scenario 6
#[then("the annotation's `file=` value is `src/lib.rs` (relative), not the absolute path")]
fn then_file_relative(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let warnings: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("::warning "))
        .collect();
    assert!(!warnings.is_empty(), "no ::warning lines:\n{stdout}");
    for line in warnings {
        let after = line.split_once("file=").expect("file= present").1;
        let path = after.split(',').next().unwrap();
        assert!(
            !path.starts_with('/'),
            "file= path should be relative, got `{path}` in:\n{line}"
        );
    }
}

// Scenario 7
#[then("the annotation's `file=` value is the absolute path")]
fn then_file_absolute(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let warnings: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("::warning "))
        .collect();
    assert!(!warnings.is_empty(), "no ::warning lines:\n{stdout}");
    for line in warnings {
        let after = line.split_once("file=").expect("file= present").1;
        let path = after.split(',').next().unwrap();
        assert!(
            path.starts_with('/'),
            "file= should be absolute when path not under CWD, got `{path}`"
        );
    }
}

// Scenario 8
#[then(
    "six `::warning` lines are emitted (the `--top` view shaping is independent of the annotation cap; the cap is the GitHub UI limit, not a display knob)"
)]
fn then_six_warnings_unaffected_by_top(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let warnings = warning_count(&stdout);
    assert_eq!(
        warnings, 6,
        "expected 6 ::warning lines despite --top 2; got {warnings}\nstdout:\n{stdout}"
    );
}

// Scenario 9
#[then(
    "the annotation set is identical to the run without `--only-failing` (the reporter already filters to `exceeds == true`)"
)]
fn then_only_failing_no_op(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let warnings = warning_count(&stdout);
    // Fixture: 3 exceeding + 3 within → 3 ::warning lines (only exceeders).
    assert_eq!(
        warnings, 3,
        "expected 3 ::warnings from 3 exceeders, got {warnings}\nstdout:\n{stdout}"
    );
}

// Scenario 10
#[then(
    "the emitted lines remain CRAP-score-descending (the reporter's own ordering invariant — the View's sort key does not leak through)"
)]
fn then_sort_by_no_op(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let scores: Vec<f64> = stdout
        .lines()
        .filter(|l| l.starts_with("::warning "))
        .map(extract_crap_score)
        .collect();
    assert!(scores.len() >= 2, "need ≥2 scores, got {scores:?}");
    for w in scores.windows(2) {
        assert!(
            w[0] >= w[1],
            "scores not CRAP-DESC despite --sort-by coverage: {scores:?}"
        );
    }
}

fn notice_count(stdout: &str) -> usize {
    stdout.lines().filter(|l| l.starts_with("::notice")).count()
}

// Scenario 11
#[then("exactly ten `::warning` lines are emitted (the ten with the highest CRAP)")]
fn then_exactly_ten_warnings(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let warnings = warning_count(&stdout);
    assert_eq!(
        warnings, 10,
        "expected ten ::warnings, got {warnings}\nstdout:\n{stdout}"
    );
}

// Scenario 11 / And
#[then(
    "exactly one trailing `::notice::` line is emitted whose message names the remaining count: `5 more functions exceed threshold; see scorecard for the full list`"
)]
fn then_truncation_notice_5(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let notices: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("::notice"))
        .collect();
    assert_eq!(
        notices.len(),
        1,
        "expected one ::notice, got {notices:?}\nstdout:\n{stdout}"
    );
    assert_eq!(
        notices[0], "::notice::5 more functions exceed threshold; see scorecard for the full list",
        "unexpected notice text"
    );
}

// Scenario 12
#[then("three `::warning` lines are emitted and no `::notice` line follows")]
fn then_three_warnings_no_notice(world: &mut GhaWorld) {
    let stdout = world.stdout();
    assert_eq!(warning_count(&stdout), 3, "stdout:\n{stdout}");
    assert_eq!(notice_count(&stdout), 0, "stdout:\n{stdout}");
}

// Scenario 13
#[then("ten `::warning` lines and one trailing `::notice::` line are emitted")]
fn then_ten_warnings_one_notice(world: &mut GhaWorld) {
    let stdout = world.stdout();
    assert_eq!(
        warning_count(&stdout),
        10,
        "expected 10 ::warnings (default cap), got\n{stdout}"
    );
    assert_eq!(
        notice_count(&stdout),
        1,
        "expected one trailing notice, got\n{stdout}"
    );
}

// Scenario 14
#[then("all eleven `::warning` lines are emitted and no `::notice` line follows")]
fn then_eleven_warnings_no_notice(world: &mut GhaWorld) {
    let stdout = world.stdout();
    assert_eq!(
        warning_count(&stdout),
        11,
        "expected 11 ::warnings (config cap=25 allows all), got\n{stdout}"
    );
    assert_eq!(notice_count(&stdout), 0, "stdout:\n{stdout}");
}

// Scenario 15
#[then("exactly five `::warning` lines are emitted (the CLI flag wins)")]
fn then_exactly_five_warnings(world: &mut GhaWorld) {
    let stdout = world.stdout();
    assert_eq!(
        warning_count(&stdout),
        5,
        "expected 5 ::warnings (CLI cap=5 overrides config cap=25), got\n{stdout}"
    );
}

// Scenario 16
#[then("the emitted message replaces `%` with `%25`, `\\r` with `%0D`, `\\n` with `%0A`")]
fn then_escape_chars_replaced(world: &mut GhaWorld) {
    let stdout = world.stdout();
    // Synthesized qualified_name is "weird%name\rwith\nbreaks"; after
    // gha_escape it must become "weird%25name%0Dwith%0Abreaks" in the
    // emitted message data.
    assert!(
        stdout.contains("weird%25name%0Dwith%0Abreaks"),
        "expected escaped qualified name in stdout, got:\n{stdout}"
    );
    // The raw control chars must NOT survive into the output.
    assert!(
        !stdout.contains("weird%name"),
        "raw `%` must be escaped, got:\n{stdout}"
    );
    let (_, message) = stdout
        .lines()
        .find(|l| l.starts_with("::warning "))
        .expect("at least one ::warning line")
        .rsplit_once("::")
        .expect(":: separator present");
    assert!(
        !message.contains('\r') && !message.contains('\n'),
        "raw CR/LF leaked into message data: {message:?}"
    );
}

// Scenario 16 / And
#[then(
    "the `file=`, `line=`, and `title=` property values are not modified (no dynamic data lands in property fields, so delimiter escaping is unnecessary)"
)]
fn then_property_values_unmodified(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let line = stdout
        .lines()
        .find(|l| l.starts_with("::warning "))
        .expect("at least one ::warning line");
    // file= must be a clean path (no %0D / %0A / %25 escape sequences;
    // the only legal escapes in property fields are , and : per the
    // GH Actions spec, and those don't appear in deterministic data).
    let after_file = line.split_once("file=").expect("file= present").1;
    let file_value = after_file.split(',').next().unwrap();
    assert!(
        !file_value.contains("%0D") && !file_value.contains("%0A") && !file_value.contains("%25"),
        "file= property value must not be escaped, got: {file_value}"
    );
    // title=CRAP <score> — same invariant: no escape sequences.
    let after_title = line.split_once("title=").expect("title= present").1;
    let title_value = after_title.split("::").next().unwrap();
    assert!(
        !title_value.contains("%0D")
            && !title_value.contains("%0A")
            && !title_value.contains("%25"),
        "title= property value must not be escaped, got: {title_value}"
    );
}

// Scenario 17
#[then("`scorecard.md` is created with the markdown reporter's output")]
fn then_scorecard_md_created(world: &mut GhaWorld) {
    let dir = world.require_dir();
    let path = dir.join("scorecard.md");
    assert!(path.exists(), "scorecard.md missing at {}", path.display());
    let content = std::fs::read_to_string(&path).expect("read scorecard.md");
    // Markdown reporter emits a table — at minimum some pipe-delimited
    // structure must appear. A more strict check would couple us to a
    // specific markdown layout the reporter may rework.
    assert!(
        content.contains('|'),
        "scorecard.md doesn't look like markdown table output:\n{content}"
    );
}

// Scenario 17 / And
#[then("stdout contains the `::warning` lines from the annotation reporter")]
fn then_stdout_has_annotation_warnings(world: &mut GhaWorld) {
    let stdout = world.stdout();
    assert!(
        warning_count(&stdout) > 0,
        "expected at least one ::warning in stdout, got:\n{stdout}"
    );
}

// Scenario 17 / And
#[then(
    "the two reporters produce consistent function counts (the annotation cap may truncate but the markdown is full-fidelity)"
)]
fn then_consistent_counts(world: &mut GhaWorld) {
    let stdout = world.stdout();
    let warnings = warning_count(&stdout);
    // Fixture: 3 branchy_X fns, default cap = 10 → cap doesn't fire,
    // both reporters cover the full set.
    assert_eq!(warnings, 3, "expected 3 ::warnings, got\n{stdout}");
    let dir = world.require_dir();
    let md = std::fs::read_to_string(dir.join("scorecard.md")).expect("read scorecard.md");
    for letter in ["a", "b", "c"] {
        let fname = format!("branchy_{letter}");
        assert!(md.contains(&fname), "markdown missing {fname}:\n{md}");
    }
}

// Scenario 18
#[then("stdout is empty")]
fn then_stdout_empty(world: &mut GhaWorld) {
    let stdout = world.stdout();
    assert!(stdout.is_empty(), "expected empty stdout, got:\n{stdout:?}");
}

// Scenario 19
#[then(
    "the CLI exits non-zero with a clap error explaining the value must be ≥ 1 (the per-step display cap is meaningless at zero; the user almost certainly meant to use the default)"
)]
fn then_zero_rejected(world: &mut GhaWorld) {
    let output = world.require_output();
    assert!(
        !output.status.success(),
        "expected non-zero exit, got status: {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    // clap's value_parser!(u32).range(1..=100) error contains "1" and
    // some "not in" / "out of range" framing; assert on the flag name
    // + the lower bound rather than the exact phrasing so the assertion
    // survives a clap version bump.
    assert!(
        stderr.contains("--annotation-limit") || stderr.contains("annotation-limit"),
        "stderr must mention --annotation-limit, got:\n{stderr}"
    );
    assert!(
        stderr.contains('1'),
        "stderr must reference the lower bound (1), got:\n{stderr}"
    );
}

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    GhaWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit(
            "tests/features/github_annotations.feature",
            |_, _, scenario| scenario.tags.iter().any(|t| t == "wired"),
        )
        .await;
}
