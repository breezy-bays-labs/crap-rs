//! Cucumber-rs runner for `tests/features/istanbul_parser.feature`.
//!
//! Wires the Istanbul JSON coverage-parsing contract directly against
//! `IstanbulCoverage` — the same library entry point `istanbul_smoke.rs`
//! unit-tests. The parser is pure (coverage JSON in → `ParseOutput`
//! out), so the spec is executable without the full CLI: the feature's
//! `crap4ts --coverage … --src …` line is the *spec's* narration of
//! "parse this coverage file"; the executable form is a direct
//! `IstanbulCoverage::parse` / `validate` call. This keeps the BDD
//! layer inside the public adapter contract, mirroring
//! `cyclomatic_walker_cucumber.rs`.
//!
//! One exception: the `validate()` empty-coverage scenario also shells
//! the `crap4ts` binary. `validate` returns the bare
//! `"no statement coverage records"` string; the actionable
//! "regenerate coverage" hint is added by the CLI layer (via
//! `AdapterMeta::coverage_hint`), so verifying the *user-facing* error
//! truthfully needs the binary.
//!
//! Named `*_cucumber` (suffix) so `.config/nextest.toml`'s
//! `binary(/.*_cucumber$/)` filter excludes it from nextest probing.

use std::path::PathBuf;
use std::process::Output;

use crap_core::domain::types::CrapError;
use crap_core::ports::{CoveragePort, ParseOutput};
use crap4ts::adapters::coverage::IstanbulCoverage;
use crap4ts::parse_diagnostic::{IstanbulDiagnosticKind, IstanbulParseDiagnostic};
use cucumber::{World, given, then, when, writer};
use tempfile::TempDir;

const JEST_FIXTURE: &str = include_str!("fixtures/istanbul-jest/coverage-final.json");
const VITEST_FIXTURE: &str = include_str!("fixtures/istanbul-vitest/coverage-final.json");
const NYC_FIXTURE: &str = include_str!("fixtures/istanbul-nyc/coverage-final.json");
const ORPHAN_BRANCH_FIXTURE: &str =
    include_str!("fixtures/istanbul-broken/coverage-with-orphan-branch.json");

type IstanbulParse = Result<ParseOutput<IstanbulParseDiagnostic>, CrapError>;

/// Write `content` to `root/name`.
fn write_file(root: &std::path::Path, name: &str, content: &str) {
    std::fs::write(root.join(name), content).expect("write fixture file");
}

/// Forward-slash-normalize a tempdir path before embedding it into a
/// JSON fixture. Windows paths use `\` which would need JSON-string
/// escaping; forward-slash is the canonical form Istanbul emitters use
/// anyway and `normalize_path` strip-prefix works either way. Mirrors
/// the pattern in `metric_unsupported_cucumber.rs` and
/// `istanbul_smoke.rs::build_fixture`.
fn json_root(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// A canonicalized tempdir (macOS routes `/tmp` → `/private/tmp`, so the
/// parser must see the same prefix the `{SRC_ROOT}`-substituted payload
/// carries) with the named source files written into it.
fn tempdir_with(files: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");
    for (name, content) in files {
        write_file(&canonical, name, content);
    }
    (tmp, canonical)
}

const TS_SIMPLE: &str = include_str!("fixtures/ts-fixtures/simple.ts");
const TS_ARROW: &str = include_str!("fixtures/ts-fixtures/arrow.ts");
const TS_MAP: &str = include_str!("fixtures/ts-fixtures/map.ts");
const TS_BUTTON: &str = include_str!("fixtures/ts-fixtures/Button.tsx");
const TS_MIXED: &str = include_str!("fixtures/ts-fixtures/mixed.ts");
const TS_BRANCH_HEAVY: &str = include_str!("fixtures/ts-fixtures/branch-heavy.ts");

/// What the shared `crap4ts --coverage coverage-final.json` When step
/// should do — set by each scenario's Given.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Op {
    #[default]
    Parse,
    Validate,
}

/// State for one scenario. The Given materializes `root` + `payload`
/// (and for the validate scenario, `cov_path`); the When produces
/// `parsed` (lib parse) and/or `validate_err` + `binary` (validate +
/// shelled binary).
#[derive(Debug, Default, World)]
struct IstanbulWorld {
    fixture: Option<(TempDir, PathBuf)>,
    payload: Option<String>,
    cov_path: Option<PathBuf>,
    op: Op,
    parsed: Option<IstanbulParse>,
    validate_err: Option<String>,
    binary: Option<Output>,
}

impl IstanbulWorld {
    fn root(&self) -> PathBuf {
        self.fixture
            .as_ref()
            .map(|(_, p)| p.clone())
            .unwrap_or_else(|| PathBuf::from("/tmp"))
    }

    /// Parse `payload` under `root` and stash the outcome.
    fn run_parse(&mut self) {
        let payload = self.payload.clone().expect("a Given must set the payload");
        let parser = IstanbulCoverage::new(self.root());
        self.parsed = Some(parser.parse(&payload));
    }

    fn ok(&self) -> &ParseOutput<IstanbulParseDiagnostic> {
        match self.parsed.as_ref().expect("a When step must parse first") {
            Ok(out) => out,
            Err(e) => panic!("expected a successful parse, got {e:?}"),
        }
    }

    /// All diagnostics of the given kind.
    fn diagnostics_of(&self, kind: IstanbulDiagnosticKind) -> Vec<&IstanbulParseDiagnostic> {
        self.ok()
            .diagnostics
            .iter()
            .filter(|d| d.kind == kind)
            .collect()
    }
}

// ── Given ────────────────────────────────────────────────────────────

#[given("a jest-emitted Istanbul `coverage-final.json` covering 3 source files")]
fn given_jest(world: &mut IstanbulWorld) {
    // The committed jest fixture covers five files (simple/arrow/Button/
    // map/mixed); the spec's "3" predates the fixture growing. The
    // binding contract is "every covered file gets line coverage", not
    // the literal count — see `then_attributes_line_coverage`.
    let (tmp, root) = tempdir_with(&[
        ("simple.ts", TS_SIMPLE),
        ("arrow.ts", TS_ARROW),
        ("Button.tsx", TS_BUTTON),
        ("map.ts", TS_MAP),
        ("mixed.ts", TS_MIXED),
    ]);
    world.payload = Some(JEST_FIXTURE.replace("{SRC_ROOT}", &json_root(&root)));
    world.fixture = Some((tmp, root));
}

#[given("the report's source root resolves all 3 file paths to discovered sources")]
fn given_root_resolves(_world: &mut IstanbulWorld) {
    // The jest Given already wrote every covered file under the
    // canonical root, so all entry paths strip-prefix cleanly.
}

#[given("a vitest-emitted Istanbul `coverage-final.json` covering 3 source files")]
fn given_vitest(world: &mut IstanbulWorld) {
    let (tmp, root) = tempdir_with(&[
        ("simple.ts", TS_SIMPLE),
        ("arrow.ts", TS_ARROW),
        ("map.ts", TS_MAP),
    ]);
    world.payload = Some(VITEST_FIXTURE.replace("{SRC_ROOT}", &json_root(&root)));
    world.fixture = Some((tmp, root));
}

#[given("an nyc-emitted Istanbul `coverage-final.json` covering 3 source files")]
fn given_nyc(world: &mut IstanbulWorld) {
    let (tmp, root) = tempdir_with(&[
        ("simple.ts", TS_SIMPLE),
        ("arrow.ts", TS_ARROW),
        ("map.ts", TS_MAP),
    ]);
    world.payload = Some(NYC_FIXTURE.replace("{SRC_ROOT}", &json_root(&root)));
    world.fixture = Some((tmp, root));
}

#[given(
    "an Istanbul `coverage-final.json` with one entry pointing at `/private/build/transpiled/foo.js`"
)]
fn given_one_orphan_entry(world: &mut IstanbulWorld) {
    let (tmp, root) = tempdir_with(&[("simple.ts", TS_SIMPLE)]);
    // One in-tree entry + one entry whose path sits outside `--src`.
    let payload = format!(
        r#"{{
          "{root}/simple.ts": {{ "path": "{root}/simple.ts", "s": {{ "0": 1 }},
            "statementMap": {{ "0": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 5 }} }} }} }},
          "/private/build/transpiled/foo.js": {{ "path": "/private/build/transpiled/foo.js", "s": {{ "0": 1 }},
            "statementMap": {{ "0": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 5 }} }} }} }}
        }}"#,
        root = json_root(&root),
    );
    world.payload = Some(payload);
    world.fixture = Some((tmp, root));
}

#[given("no source file resolves to that path under `--src src`")]
fn given_no_source_resolves(_world: &mut IstanbulWorld) {
    // `/private/build/transpiled/foo.js` is outside the tempdir root by
    // construction — nothing to set up.
}

#[given(r#"a `coverage.json` whose top-level shape is `{ "foo": "bar" }` (not Istanbul)"#)]
fn given_non_istanbul_json(world: &mut IstanbulWorld) {
    // Write the bad JSON to disk so the When step can shell the binary
    // alongside the lib parse — the spec's "exits non-zero" assertion is
    // verified against the shipped binary, not just narrated against
    // `parse()`'s `coverage.is_empty()` proxy.
    let payload = r#"{ "foo": "bar" }"#;
    let (tmp, root) = tempdir_with(&[]);
    let cov_path = root.join("coverage.json");
    std::fs::write(&cov_path, payload).expect("write coverage.json");
    world.payload = Some(payload.to_string());
    world.cov_path = Some(cov_path);
    world.fixture = Some((tmp, root));
}

#[given("a `coverage-final.json` whose `b` record references branchId `42`")]
fn given_orphan_branch(world: &mut IstanbulWorld) {
    let (tmp, root) = tempdir_with(&[("branch-heavy.ts", TS_BRANCH_HEAVY)]);
    world.payload = Some(ORPHAN_BRANCH_FIXTURE.replace("{SRC_ROOT}", &json_root(&root)));
    world.fixture = Some((tmp, root));
}

#[given("`branchMap` contains no entry for branchId `42`")]
fn given_branchmap_omits_42(_world: &mut IstanbulWorld) {
    // The orphan-branch fixture is built so branchId 42 has no
    // `branchMap` entry — nothing to set up.
}

#[given(
    "a `coverage-final.json` that decodes as Istanbul JSON but every entry's `statementMap` is empty"
)]
fn given_empty_statement_maps(world: &mut IstanbulWorld) {
    let (tmp, root) = tempdir_with(&[("simple.ts", TS_SIMPLE)]);
    let payload = format!(
        r#"{{ "{root}/simple.ts": {{ "path": "{root}/simple.ts", "s": {{}}, "statementMap": {{}} }} }}"#,
        root = json_root(&root),
    );
    let cov_path = root.join("coverage-final.json");
    std::fs::write(&cov_path, &payload).expect("write coverage file");
    world.payload = Some(payload);
    world.cov_path = Some(cov_path);
    world.op = Op::Validate;
    world.fixture = Some((tmp, root));
}

#[given("a jest-emitted `coverage-final.json` with `hash` and `contentHash` fields")]
fn given_extra_fields(world: &mut IstanbulWorld) {
    let (tmp, root) = tempdir_with(&[("simple.ts", TS_SIMPLE)]);
    // A minimal jest-shaped entry carrying two unknown top-level fields.
    let payload = format!(
        r#"{{ "{root}/simple.ts": {{ "path": "{root}/simple.ts",
          "hash": "deadbeef", "contentHash": "cafef00d",
          "s": {{ "0": 3 }},
          "statementMap": {{ "0": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 9 }} }} }} }} }}"#,
        root = json_root(&root),
    );
    world.payload = Some(payload);
    world.fixture = Some((tmp, root));
}

#[given("no other deviation from the expected schema")]
fn given_no_other_deviation(_world: &mut IstanbulWorld) {}

#[given("an Istanbul `coverage-final.json` whose entries use relative paths like `src/foo.ts`")]
fn given_relative_paths(world: &mut IstanbulWorld) {
    let (tmp, root) = tempdir_with(&[("simple.ts", TS_SIMPLE)]);
    // The entry path is `simple.ts` relative to `--src`; the suffix
    // fallback resolves it against the discovered source tree.
    let payload = r#"{ "simple.ts": { "path": "simple.ts", "s": { "0": 1 },
      "statementMap": { "0": { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 5 } } } } }"#
        .to_string();
    world.payload = Some(payload);
    world.fixture = Some((tmp, root));
}

#[given("the operator invokes `crap4ts --coverage coverage-final.json --src /home/me/project/src`")]
fn given_operator_invokes_with_src(_world: &mut IstanbulWorld) {
    // Narration: the `--src` is the canonical tempdir the relative-path
    // Given already created; the spec's `/home/me/project/src` stands
    // in for "the operator's project root".
}

// ── When ─────────────────────────────────────────────────────────────

/// Shared by the parse-path scenarios (jest/vitest/nyc/orphan-path/
/// branch-mismatch) and the validate scenario. Dispatches on `op` set
/// by the Given.
#[when("the operator runs `crap4ts --coverage coverage-final.json --src src`")]
fn when_run_coverage_final(world: &mut IstanbulWorld) {
    match world.op {
        Op::Parse => world.run_parse(),
        Op::Validate => {
            let root = world.root();
            let cov = world
                .cov_path
                .clone()
                .expect("validate Given sets cov_path");
            world.validate_err = IstanbulCoverage::new(root.clone()).validate(&cov).err();
            world.binary = Some(
                assert_cmd::Command::cargo_bin("crap4ts")
                    .expect("crap4ts binary discoverable")
                    .args(["--coverage".as_ref(), cov.as_os_str()])
                    .args(["--src".as_ref(), root.as_os_str()])
                    .output()
                    .expect("crap4ts executes"),
            );
        }
    }
}

#[when("the operator runs `crap4ts --coverage coverage.json --src src`")]
fn when_run_coverage_json(world: &mut IstanbulWorld) {
    // Lib-level: parse() returns Ok(SchemaUnrecognized diagnostic).
    world.run_parse();
    // Binary-level: shell crap4ts so `then_exits_non_zero_schema` can
    // assert the spec's exit-code claim against shipped behavior.
    let root = world.root();
    let cov = world
        .cov_path
        .clone()
        .expect("schema-unrecognized Given writes the coverage file");
    world.binary = Some(
        assert_cmd::Command::cargo_bin("crap4ts")
            .expect("crap4ts binary discoverable")
            .args(["--coverage".as_ref(), cov.as_os_str()])
            .args(["--src".as_ref(), root.as_os_str()])
            .output()
            .expect("crap4ts executes"),
    );
}

#[when("the parser normalizes the entry paths")]
fn when_parser_normalizes(world: &mut IstanbulWorld) {
    world.run_parse();
}

// ── Then ─────────────────────────────────────────────────────────────

#[then("the report attributes line coverage to all 3 files")]
fn then_attributes_line_coverage(world: &mut IstanbulWorld) {
    let out = world.ok();
    assert!(
        !out.coverage.is_empty(),
        "expected line coverage for the fixture's files; got an empty map",
    );
    for (file, lines) in &out.coverage {
        assert!(!lines.is_empty(), "no line records for `{file}`");
    }
}

#[then("no warnings or diagnostics are emitted for the coverage input")]
fn then_no_diagnostics(world: &mut IstanbulWorld) {
    let out = world.ok();
    assert!(
        out.diagnostics.is_empty(),
        "expected a clean parse; got diagnostics: {:?}",
        out.diagnostics,
    );
}

#[then("the diagnostics section of the report contains one entry for that unresolved path")]
fn then_one_path_unresolved(world: &mut IstanbulWorld) {
    let unresolved = world.diagnostics_of(IstanbulDiagnosticKind::PathUnresolved);
    assert_eq!(
        unresolved.len(),
        1,
        "expected exactly one path-unresolved diagnostic; got {:?}",
        world.ok().diagnostics,
    );
}

#[then("the diagnostic's kind is `path-unresolved`")]
fn then_kind_path_unresolved(world: &mut IstanbulWorld) {
    assert_eq!(
        world
            .diagnostics_of(IstanbulDiagnosticKind::PathUnresolved)
            .len(),
        1
    );
}

#[then("the diagnostic's message mentions the unresolved path")]
fn then_message_mentions_path(world: &mut IstanbulWorld) {
    let d = world.diagnostics_of(IstanbulDiagnosticKind::PathUnresolved)[0];
    assert!(
        d.message.contains("/private/build/transpiled/foo.js"),
        "message should name the unresolved path; got: {}",
        d.message,
    );
}

#[then(
    "the scorecard still produces line coverage for the OTHER entries (never abort first-record)"
)]
fn then_other_entries_survive(world: &mut IstanbulWorld) {
    let out = world.ok();
    assert!(
        out.coverage.contains_key("simple.ts"),
        "the in-tree entry must still parse; coverage keys: {:?}",
        out.coverage.keys().collect::<Vec<_>>(),
    );
}

#[then("`crap4ts` exits with a non-zero status")]
fn then_exits_non_zero_schema(world: &mut IstanbulWorld) {
    // Lib-level: a non-Istanbul shape yields zero coverage (the
    // downstream gate for the CLI's non-zero exit).
    let out = world.ok();
    assert!(
        out.coverage.is_empty(),
        "a non-Istanbul shape must yield no coverage",
    );
    // Binary-level: verify the spec's exit-code claim directly against
    // the shipped binary, not just the lib-level proxy above.
    let bin = world
        .binary
        .as_ref()
        .expect("schema-unrecognized When shells the binary");
    assert!(
        !bin.status.success(),
        "crap4ts must exit non-zero on an unrecognized coverage shape; stderr=\n{}",
        String::from_utf8_lossy(&bin.stderr),
    );
}

#[then(r#"the user-facing error message names the problem ("top-level shape not recognized as Istanbul")"#)]
fn then_error_names_problem(world: &mut IstanbulWorld) {
    let d = &world.diagnostics_of(IstanbulDiagnosticKind::SchemaUnrecognized)[0];
    assert!(
        d.message
            .contains("top-level shape not recognized as Istanbul"),
        "got: {}",
        d.message,
    );
}

#[then("the message hints at the expected shape `{[path]: { path, s, statementMap, … }}`")]
fn then_message_hints_shape(world: &mut IstanbulWorld) {
    let d = &world.diagnostics_of(IstanbulDiagnosticKind::SchemaUnrecognized)[0];
    assert!(
        d.message.contains("path, s, statementMap"),
        "message should hint at the expected Istanbul shape; got: {}",
        d.message,
    );
}

#[then("the JSON envelope's diagnostic record carries kind `schema-unrecognized`")]
fn then_carries_schema_unrecognized(world: &mut IstanbulWorld) {
    let schema = world.diagnostics_of(IstanbulDiagnosticKind::SchemaUnrecognized);
    assert_eq!(schema.len(), 1, "{:?}", world.ok().diagnostics);
}

#[then("the diagnostics section of the report contains one entry for that branch")]
fn then_one_branch_diagnostic(world: &mut IstanbulWorld) {
    let mismatches = world.diagnostics_of(IstanbulDiagnosticKind::BranchMismatch);
    assert_eq!(
        mismatches.len(),
        1,
        "expected exactly one branch-mismatch diagnostic; got {:?}",
        world.ok().diagnostics,
    );
}

#[then("the diagnostic's kind is `branch-mismatch`")]
fn then_kind_branch_mismatch(world: &mut IstanbulWorld) {
    assert_eq!(
        world
            .diagnostics_of(IstanbulDiagnosticKind::BranchMismatch)
            .len(),
        1
    );
}

#[then(r#"the diagnostic's message redirects the user to "the coverage tool's issue tracker""#)]
fn then_message_redirects(world: &mut IstanbulWorld) {
    let d = world.diagnostics_of(IstanbulDiagnosticKind::BranchMismatch)[0];
    assert!(
        d.message.contains("coverage tool's issue tracker"),
        "got: {}",
        d.message,
    );
}

#[then("`crap4ts` exits with a non-zero status before reaching the parse pass")]
fn then_validate_fails_preflight(world: &mut IstanbulWorld) {
    assert!(
        world.validate_err.is_some(),
        "validate() should reject the empty-statementMap file pre-flight",
    );
    let out = world
        .binary
        .as_ref()
        .expect("validate When shells the binary");
    assert!(
        !out.status.success(),
        "crap4ts must exit non-zero on an empty coverage file",
    );
}

#[then(r#"the user-facing error explains "no statement coverage records""#)]
fn then_error_explains_no_records(world: &mut IstanbulWorld) {
    let err = world.validate_err.as_ref().expect("validate err");
    assert!(err.contains("no statement coverage records"), "got: {err}");
}

#[then(r#"the error message tells the user how to regenerate coverage (e.g., "run jest with --coverage")"#)]
fn then_error_tells_regenerate(world: &mut IstanbulWorld) {
    // The actionable regenerate hint is added by the CLI layer (the
    // adapter's `validate` returns only the bare reason), so this is
    // verified against the shelled binary's stderr.
    let out = world
        .binary
        .as_ref()
        .expect("validate When shells the binary");
    let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(
        stderr.contains("coverage")
            && (stderr.contains("regenerate") || stderr.contains("--coverage")),
        "stderr should tell the user how to regenerate coverage; got:\n{}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[then("the parser produces a `ParseOutput` containing line coverage for the entries")]
fn then_parseoutput_has_coverage(world: &mut IstanbulWorld) {
    let out = world.ok();
    assert!(
        out.coverage.contains_key("simple.ts"),
        "expected line coverage for simple.ts; keys: {:?}",
        out.coverage.keys().collect::<Vec<_>>(),
    );
}

#[then("no `ParseDiagnostic` records are emitted for the unknown fields")]
fn then_no_diagnostics_for_unknown(world: &mut IstanbulWorld) {
    assert!(
        world.ok().diagnostics.is_empty(),
        "unknown fields must be tolerated silently; got: {:?}",
        world.ok().diagnostics,
    );
}

#[then("the paths resolve against `/home/me/project/src`")]
fn then_paths_resolve(world: &mut IstanbulWorld) {
    let out = world.ok();
    assert!(
        out.diagnostics.is_empty(),
        "relative paths should resolve cleanly; got: {:?}",
        out.diagnostics,
    );
    assert!(
        !out.coverage.is_empty(),
        "relative-path entries should produce coverage",
    );
}

#[then("the normalized paths in the output are workspace-relative")]
fn then_normalized_workspace_relative(world: &mut IstanbulWorld) {
    for key in world.ok().coverage.keys() {
        assert!(
            !key.starts_with('/') && !key.contains(':'),
            "coverage key `{key}` should be workspace-relative, not absolute",
        );
    }
}

// ── Runner ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // `@wired`-only filter (AGENTS.md rule 5). `with_default_cli()`
    // skips argv parsing so the `--skip <name>` libtest args that
    // `cargo mutants --package crap4ts` injects do not abort cucumber's
    // strict clap CLI (the crap-rs#224 gate-zeroing class).
    IstanbulWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .with_default_cli()
        .filter_run_and_exit("tests/features/istanbul_parser.feature", |_, _, sc| {
            sc.tags.iter().any(|t| t == "wired")
        })
        .await;
}
