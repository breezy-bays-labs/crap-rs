//! Cucumber-rs runner for `tests/features/file_extensions.feature`.
//!
//! File discovery, `crap4ts.toml` `exclude` globs, unrecognized-
//! extension skipping, and continue-on-parse-failure are all binary-
//! level concerns (the filesystem walker + config loading + exit
//! codes), so this harness shells the `crap4ts` binary with
//! `--format json` and reads the per-function `file_path` set out of
//! the envelope — mirroring `metric_unsupported_cucumber.rs`.
//!
//! Scenario "A .d.ts file is skipped by default" is `@wired` post-
//! crap-rs#253: `AdapterMeta::forced_excludes` carries `**/*.d.ts` for
//! crap4ts, so declaration files are dropped by the source-discovery
//! walker before they reach the AST walker.
//!
//! Named `*_cucumber` (suffix) so `.config/nextest.toml`'s
//! `binary(/.*_cucumber$/)` filter excludes it from nextest probing.

use std::path::PathBuf;

use cucumber::{World, given, then, when, writer};
use serde::Deserialize;
use tempfile::TempDir;

/// Minimal projection of the `crap4ts --format json` envelope.
#[derive(Debug, Deserialize)]
struct Envelope {
    result: EnvelopeResult,
}

#[derive(Debug, Deserialize)]
struct EnvelopeResult {
    functions: Vec<EnvelopeFn>,
}

#[derive(Debug, Deserialize)]
struct EnvelopeFn {
    scored: EnvelopeScored,
}

#[derive(Debug, Deserialize)]
struct EnvelopeScored {
    identity: EnvelopeIdentity,
}

#[derive(Debug, Deserialize)]
struct EnvelopeIdentity {
    file_path: String,
}

/// Minimal valid single-line function body for a given extension —
/// matching the dialect of `file_extensions.feature`'s Examples table
/// (`.cjs` is CommonJS, `.tsx`/`.jsx` carry JSX). One line so the
/// coverage statement at line 1 joins.
fn canned_source(ext: &str) -> &'static str {
    match ext {
        ".ts" => "export function greet(name: string): string { return name; }\n",
        ".tsx" => "export const Greet = ({name}: {name: string}) => <span>hi {name}</span>;\n",
        ".js" => "export function greet(name) { return 'hello ' + name; }\n",
        ".jsx" => "export const Greet = ({name}) => <span>hi {name}</span>;\n",
        ".mjs" => "export function greet(name) { return 'hello ' + name; }\n",
        ".cjs" => "module.exports.greet = function(name) { return 'hello ' + name; };\n",
        other => panic!("no canned source for extension {other:?}"),
    }
}

/// Write a `coverage-final.json` under `root` covering each named file
/// with a single statement at line 1, so `validate()`'s pre-flight
/// (at least one non-empty `statementMap`) passes. Returns its path.
fn write_coverage(root: &std::path::Path, files: &[&str]) -> PathBuf {
    let entries: Vec<String> = files
        .iter()
        .map(|f| {
            let abs = root.join(f).to_string_lossy().replace('\\', "/");
            format!(
                r#""{abs}": {{ "path": "{abs}", "s": {{ "0": 1 }},
                  "statementMap": {{ "0": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 5 }} }} }} }}"#
            )
        })
        .collect();
    let payload = format!("{{ {} }}", entries.join(", "));
    let path = root.join("coverage-final.json");
    std::fs::write(&path, payload).expect("write coverage-final.json");
    path
}

/// State for one scenario. The Given materializes the source tree +
/// coverage file (and, for the exclude scenario, a `crap4ts.toml`); the
/// When shells `crap4ts` and records the envelope, exit code, stderr.
#[derive(Debug, Default, World)]
struct FileExtWorld {
    fixture: Option<(TempDir, PathBuf)>,
    cov_path: Option<PathBuf>,
    config: Option<PathBuf>,
    functions: Vec<EnvelopeFn>,
    exit_code: Option<i32>,
    stderr: String,
}

impl FileExtWorld {
    /// Create the canonicalized tempdir (lazily) and return its root.
    fn root(&mut self) -> PathBuf {
        if self.fixture.is_none() {
            let tmp = tempfile::tempdir().expect("tempdir");
            let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");
            self.fixture = Some((tmp, canonical));
        }
        self.fixture.as_ref().unwrap().1.clone()
    }

    fn write(&mut self, name: &str, content: &str) {
        let root = self.root();
        std::fs::write(root.join(name), content).expect("write source file");
    }

    /// The set of file paths that contributed at least one function.
    fn files_with_functions(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .functions
            .iter()
            .map(|f| f.scored.identity.file_path.as_str())
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

// ── Given ────────────────────────────────────────────────────────────

#[given(
    regex = r"^a source tree under .src/. containing a single file .example(\.\w+). with contents .+$"
)]
fn given_single_extension_file(world: &mut FileExtWorld, ext: String) {
    let name = format!("example{ext}");
    world.write(&name, canned_source(&ext));
    let root = world.root();
    world.cov_path = Some(write_coverage(&root, &[&name]));
}

#[given("a valid Istanbul `coverage-final.json` covering that file")]
fn given_coverage_covers_file(_world: &mut FileExtWorld) {
    // The single-extension Given already wrote the coverage file.
}

#[given("a source tree under `src/` containing `app.ts` and `app.test.ts`")]
fn given_app_and_test(world: &mut FileExtWorld) {
    world.write("app.ts", "export function app() { return 1; }\n");
    world.write("app.test.ts", "export function spec() { return 2; }\n");
    let root = world.root();
    world.cov_path = Some(write_coverage(&root, &["app.ts", "app.test.ts"]));
}

#[given("the operator's `crap4ts.toml` has no exclusion for `.test.ts`")]
fn given_no_test_exclusion(_world: &mut FileExtWorld) {
    // No config file is written, so nothing excludes `.test.ts`.
}

#[given(r#"the operator's `crap4ts.toml` has `exclude = ["**/*.test.ts"]`"#)]
fn given_test_exclusion(world: &mut FileExtWorld) {
    let root = world.root();
    let config = root.join("crap4ts.toml");
    std::fs::write(&config, "exclude = [\"**/*.test.ts\"]\n").expect("write crap4ts.toml");
    world.config = Some(config);
}

#[given("a source tree under `src/` containing `app.ts` and `notes.txt`")]
fn given_app_and_notes(world: &mut FileExtWorld) {
    world.write("app.ts", "export function app() { return 1; }\n");
    world.write("notes.txt", "just some prose, not source\n");
    let root = world.root();
    world.cov_path = Some(write_coverage(&root, &["app.ts"]));
}

#[given("a source tree under `src/` containing `types.d.ts` and `app.ts`")]
fn given_dts_and_app(world: &mut FileExtWorld) {
    // `types.d.ts` is a declaration file (ambient types only); `app.ts`
    // carries an executable function so we can assert positive
    // discovery alongside the negative-discovery assertion on
    // `types.d.ts`. The walker can syntactically parse declaration
    // files (oxc accepts `export declare function`); the
    // `forced_excludes` skip happens at filesystem discovery so the
    // AST walker never sees them.
    world.write("types.d.ts", "export declare function ambient(): number;\n");
    world.write("app.ts", "export function app() { return 1; }\n");
    let root = world.root();
    // Coverage covers `app.ts` only — Istanbul never emits coverage
    // for `.d.ts` (no statements to instrument), so excluding it from
    // the fixture mirrors what a real jest run produces.
    world.cov_path = Some(write_coverage(&root, &["app.ts"]));
}

#[given("the operator's `crap4ts.toml` does NOT explicitly include `.d.ts`")]
fn given_no_dts_include_in_config(_world: &mut FileExtWorld) {
    // No `crap4ts.toml` is written; the default skip applies. There
    // is no `include` flag for `.d.ts` today (crap-rs#253) — the skip
    // is unconditional. This Given is narration that pins the
    // contract for future readers.
}

#[given(
    "a source tree under `src/` containing `good.ts` (parses cleanly) and `broken.ts` (syntactically invalid)"
)]
fn given_good_and_broken(world: &mut FileExtWorld) {
    world.write("good.ts", "export function good() { return 1; }\n");
    world.write("broken.ts", "export function broken( {\n");
    let root = world.root();
    world.cov_path = Some(write_coverage(&root, &["good.ts"]));
}

// ── When ─────────────────────────────────────────────────────────────

#[when("the operator runs `crap4ts --coverage coverage-final.json --src src`")]
fn when_run_crap4ts(world: &mut FileExtWorld) {
    let root = world.root();
    let cov = world
        .cov_path
        .clone()
        .expect("a Given must write the coverage file");
    let mut cmd = assert_cmd::Command::cargo_bin("crap4ts").expect("crap4ts binary discoverable");
    cmd.arg("--coverage")
        .arg(&cov)
        .arg("--src")
        .arg(&root)
        .args(["--format", "json", "--no-fail"]);
    if let Some(config) = &world.config {
        cmd.arg("--config").arg(config);
    }
    let out = cmd.output().expect("crap4ts executes");
    world.exit_code = out.status.code();
    world.stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    // Under `--no-fail` the run produces an envelope even when a file
    // failed to parse; a JSON-parse failure here is loud (silent
    // swallow would let "doesn't include notes.txt" pass trivially on
    // an empty Vec).
    let envelope: Envelope = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "crap4ts --format json must emit a valid envelope: {e}\nstderr=\n{}",
            String::from_utf8_lossy(&out.stderr),
        )
    });
    world.functions = envelope.result.functions;
}

// ── Then ─────────────────────────────────────────────────────────────

#[then("`crap4ts` exits with status 0 (no parse errors)")]
fn then_exits_zero(world: &mut FileExtWorld) {
    assert_eq!(
        world.exit_code,
        Some(0),
        "expected exit 0; stderr=\n{}",
        world.stderr,
    );
}

#[then(regex = r"^the report includes at least one function from .example(\.\w+).$")]
fn then_report_includes_extension(world: &mut FileExtWorld, ext: String) {
    let name = format!("example{ext}");
    assert!(
        world.files_with_functions().contains(&name.as_str()),
        "expected a function from `{name}`; discovered: {:?}",
        world.files_with_functions(),
    );
}

#[then("the report includes functions from both `app.ts` and `app.test.ts`")]
fn then_includes_both(world: &mut FileExtWorld) {
    let files = world.files_with_functions();
    assert!(files.contains(&"app.ts"), "app.ts missing; got {files:?}");
    assert!(
        files.contains(&"app.test.ts"),
        "app.test.ts should be included by default; got {files:?}",
    );
}

#[then("the report includes functions from `app.ts`")]
fn then_includes_app(world: &mut FileExtWorld) {
    assert!(
        world.files_with_functions().contains(&"app.ts"),
        "app.ts missing; got {:?}",
        world.files_with_functions(),
    );
}

#[then("the report does NOT include entries from `app.test.ts`")]
fn then_excludes_test(world: &mut FileExtWorld) {
    assert!(
        !world.files_with_functions().contains(&"app.test.ts"),
        "app.test.ts should be excluded by the crap4ts.toml glob; got {:?}",
        world.files_with_functions(),
    );
}

#[then("the report does NOT include entries from `types.d.ts`")]
fn then_excludes_dts_entries(world: &mut FileExtWorld) {
    // The discovery walker drops `*.d.ts` via `forced_excludes`
    // before the AST walker runs, so neither file paths nor function
    // entries from declaration files reach the envelope.
    let files = world.files_with_functions();
    assert!(
        !files.iter().any(|f| f.ends_with(".d.ts")),
        "no .d.ts file should contribute a function entry; got {files:?}",
    );
    assert!(
        !files.contains(&"types.d.ts"),
        "`types.d.ts` must not appear in the report; got {files:?}",
    );
}

#[then("the report does NOT mention `notes.txt`")]
fn then_no_notes(world: &mut FileExtWorld) {
    assert!(
        !world.files_with_functions().contains(&"notes.txt"),
        "notes.txt is not a source file and must not appear; got {:?}",
        world.files_with_functions(),
    );
}

#[then("no diagnostic is emitted about `notes.txt`")]
fn then_no_notes_diagnostic(world: &mut FileExtWorld) {
    assert!(
        !world.stderr.contains("notes.txt"),
        "an unrecognized extension must be skipped silently; stderr=\n{}",
        world.stderr,
    );
}

#[then("`crap4ts` still produces a scorecard for functions in `good.ts`")]
fn then_good_survives(world: &mut FileExtWorld) {
    assert!(
        world.files_with_functions().contains(&"good.ts"),
        "good.ts must still be scored despite broken.ts; got {:?}",
        world.files_with_functions(),
    );
}

#[then("the diagnostics section reports `broken.ts` as unparseable")]
fn then_broken_reported(world: &mut FileExtWorld) {
    // crap4ts surfaces per-file parse failures as `warning:` lines on
    // stderr (the `--format json` envelope carries no diagnostics
    // section).
    assert!(
        world.stderr.contains("broken.ts"),
        "stderr should name the unparseable file; got=\n{}",
        world.stderr,
    );
}

#[then("`AnalysisDiagnostics.files_unparseable` equals 1")]
fn then_files_unparseable_one(world: &mut FileExtWorld) {
    // The internal `files_unparseable` count surfaces to the user as
    // the "N source file(s) could not be parsed" stderr summary.
    assert!(
        world
            .stderr
            .contains("1 source file(s) could not be parsed"),
        "stderr should report exactly one unparseable file; got=\n{}",
        world.stderr,
    );
}

#[then(
    "the run exits with non-zero status ONLY if threshold violations gate it (not because of parse failure alone)"
)]
fn then_parse_failure_alone_does_not_gate(world: &mut FileExtWorld) {
    // The run used `--no-fail`, so threshold violations never gate;
    // a parse failure alone must therefore leave the exit code 0.
    assert_eq!(
        world.exit_code,
        Some(0),
        "a parse failure alone must not force a non-zero exit; stderr=\n{}",
        world.stderr,
    );
}

// ── Runner ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // `@wired`-only filter (AGENTS.md rule 5) — every scenario in this
    // feature is wired post-crap-rs#253 (the `.d.ts` skip closed the
    // last `@unwired` here), so the filter is currently a no-op pass-
    // through. Kept for shape consistency with other crap4ts cucumber
    // harnesses + so a future `@unwired` regression is mechanically
    // gated out rather than silently passing as a skipped scenario.
    // `with_default_cli()` skips argv parsing so the `--skip` libtest
    // args `cargo mutants --package crap4ts` injects do not abort
    // cucumber's strict clap CLI (the crap-rs#224 gate-zeroing class).
    FileExtWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .with_default_cli()
        .filter_run_and_exit("tests/features/file_extensions.feature", |_, _, sc| {
            sc.tags.iter().any(|t| t == "wired")
        })
        .await;
}
