//! End-to-end smoke tests for the `crap4ts` binary.
//!
//! Spawns the `crap4ts` binary via `assert_cmd` against the W1.1 jest
//! fixture (with `{SRC_ROOT}` template substituted at test time) plus
//! the W1.2 walker. The four scenarios cover (1) the default table
//! reporter, (2) `--format json` envelope parse-ability, (3) the
//! markdown reporter header, and (4) the parse-failure-continues
//! behavior where `broken.ts` is silently skipped while `simple.ts`
//! still surfaces in the report.
//!
//! Owns the **wired binary canary**: if `cargo build --bin crap4ts`
//! succeeds but the binary can't actually parse + walk + report against
//! a real jest fixture, these tests fail.
//!
//! Together with `wire_envelope_crap4ts.rs` (envelope shape lock) and
//! the existing `istanbul_smoke.rs` + `walker_smoke.rs` (unit-level
//! contracts) these complete the W1 walking-skeleton verification.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

const FIXTURE_TEMPLATE: &str = include_str!("fixtures/istanbul-jest/coverage-final.json");

/// Build a canonicalised tempdir, copy the five W1.1 jest fixture TS
/// files into it, and substitute `{SRC_ROOT}` in the templated
/// `coverage-final.json` with the canonical tempdir path. Returns
/// the `TempDir` (keep it alive for the lifetime of the test — the
/// directory is removed on drop) plus the canonical path.
///
/// Mirrors `istanbul_smoke::build_fixture`'s pattern but materializes
/// the substituted `coverage-final.json` on disk so the binary can
/// load it via `--coverage <path>`. The two helpers stay separate by
/// design: editing the W1.1 helper to dual-purpose it would invite
/// drift between unit-level and end-to-end fixture loading.
fn build_jest_fixture() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Canonicalize because macOS's /tmp redirects to /private/tmp;
    // without canonicalization the parser sees a different prefix
    // than the JSON's substituted paths and emits PathUnresolved
    // for every entry (verified empirically during W1.3 eyeball).
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");

    for (name, content) in [
        ("simple.ts", include_str!("fixtures/ts-fixtures/simple.ts")),
        ("arrow.ts", include_str!("fixtures/ts-fixtures/arrow.ts")),
        (
            "Button.tsx",
            include_str!("fixtures/ts-fixtures/Button.tsx"),
        ),
        ("map.ts", include_str!("fixtures/ts-fixtures/map.ts")),
        ("mixed.ts", include_str!("fixtures/ts-fixtures/mixed.ts")),
    ] {
        std::fs::write(canonical.join(name), content).expect("write fixture");
    }

    // Normalize path separators before string-substituting into the
    // JSON template — Windows backslashes would land as invalid `\p`-
    // style escape sequences in the resulting JSON. Forward slashes
    // are valid path separators in JSON string values on every
    // platform. No-op on macOS/linux.
    let payload = FIXTURE_TEMPLATE.replace(
        "{SRC_ROOT}",
        &canonical.to_string_lossy().replace('\\', "/"),
    );
    std::fs::write(canonical.join("coverage-final.json"), payload)
        .expect("write coverage-final.json");

    (tmp, canonical)
}

/// Spawn `crap4ts` with the given `--src` + `--coverage` + extra args
/// and assert the binary exists. Returns the full `std::process::Output`
/// so callers can assert on stdout/stderr/status independently.
fn run_crap4ts(src: &Path, coverage: &Path, extra: &[&str]) -> std::process::Output {
    let mut cmd = Command::cargo_bin("crap4ts").expect("crap4ts binary discoverable");
    cmd.arg("--src")
        .arg(src)
        .arg("--coverage")
        .arg(coverage)
        .arg("--threshold")
        .arg("16")
        .arg("--no-fail")
        .args(extra);
    cmd.output().expect("crap4ts executes")
}

// ── 1. Happy-path table: default reporter renders function names ──────

#[test]
fn happy_path_table_lists_simple_add_function() {
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");
    let out = run_crap4ts(&root, &coverage, &[]);

    assert!(
        out.status.success(),
        "crap4ts exited non-zero: stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // simple.ts contains `export function add(a, b)`. The table
    // reporter renders `| simple.ts | add |` (column-padded).
    assert!(
        stdout.contains("simple.ts") && stdout.contains("add"),
        "table output missing add/simple.ts; stdout=\n{stdout}"
    );
}

// ── 2. JSON envelope parses + reports at least one analyzed function ──

#[test]
fn json_envelope_parses_and_reports_functions() {
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");
    let out = run_crap4ts(&root, &coverage, &["--format", "json"]);

    assert!(
        out.status.success(),
        "crap4ts exited non-zero: stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let value: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("crap4ts --format json emits valid JSON");

    // The crap-core envelope's summary block exposes `total_functions`
    // (the count of functions the walker emitted that joined against
    // ≥1 coverage record). The plan-of-record asserted
    // `analysis_output.summary.functions_analyzed`; the actual field
    // is `result.summary.total_functions` — surfaced as a W1.3 plan
    // deviation in the PR body alongside the markdown header and
    // shared-helper layout deviations.
    let total_functions = value
        .pointer("/result/summary/total_functions")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| {
            panic!("envelope missing /result/summary/total_functions; envelope={value:#}")
        });
    assert!(
        total_functions >= 1,
        "expected >=1 functions analyzed; got {total_functions}"
    );
}

// ── 3. Markdown reporter emits the `# {tool_name} v{version}` header ──
//
// Plan-of-record deviation surfaced during W1.3: the orchestrator prompt
// asserted `# CRAP Scorecard` as the markdown header, but the crap-core
// reporter at `crates/crap-core/src/adapters/reporters/markdown.rs:86-88`
// emits `# {tool_name} v{tool_version} — CRAP Score Analysis`. The
// `## CRAP Scorecard` section is only appended when a delta baseline is
// present. We assert against the actual header shape — version-prefix
// substring keeps the test alive across alpha → rc → 2.0.0 without
// churn. See PR body "Deviations from plan-of-record" for the full
// reconciliation.

#[test]
fn markdown_reporter_emits_crap4ts_title_header() {
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");
    let out = run_crap4ts(&root, &coverage, &["--format", "markdown"]);

    assert!(
        out.status.success(),
        "crap4ts exited non-zero: stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("# crap4ts v"),
        "markdown body should start with `# crap4ts v…`; got first line:\n{}",
        stdout.lines().next().unwrap_or("(empty)")
    );
    assert!(
        stdout.contains("— CRAP Score Analysis"),
        "markdown body should contain `— CRAP Score Analysis`; got:\n{stdout}"
    );
}

// ── 4. Parse-failure-with-other-files: broken.ts skipped, simple.ts kept ──
//
// Per `crates/crap-core/src/core/mod.rs:286-310`, when the walker
// returns `Err(CrapError::SourceParse(_))` for one file, the
// orchestrator emits a `warning: skipping <path>` and increments
// `files_unparseable`, then continues with the rest of the discovered
// files. This test exercises that with a hand-rolled mini-fixture:
//   - tempdir contains `simple.ts` (parseable) + `broken.ts` (the W1.2
//     malformed fixture)
//   - coverage-final.json references only simple.ts (broken.ts has no
//     coverage to emit; the walker discovers it through directory
//     traversal and fails parse independently)

#[test]
fn parse_failure_continues_other_files_are_analyzed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");

    // Copy the two TS fixtures into the tempdir.
    std::fs::write(
        canonical.join("simple.ts"),
        include_str!("fixtures/ts-fixtures/simple.ts"),
    )
    .expect("write simple.ts");
    std::fs::write(
        canonical.join("broken.ts"),
        include_str!("fixtures/ts-fixtures/broken.ts"),
    )
    .expect("write broken.ts");

    // Minimal coverage-final.json referencing only simple.ts — the
    // walker discovers broken.ts independently through file-system
    // traversal and fails parse on it. Mirrors the per-file shape from
    // `crates/crap4ts/tests/fixtures/istanbul-jest/coverage-final.json`
    // but trimmed to a single entry to keep this test's coupling to
    // the W1.1 fixture loose.
    let coverage_template = r#"{
        "{SRC_ROOT}/simple.ts": {
            "path": "{SRC_ROOT}/simple.ts",
            "statementMap": {
                "0": { "start": { "line": 4, "column": 2 }, "end": { "line": 4, "column": 15 } }
            },
            "s": { "0": 3 },
            "branchMap": {},
            "b": {},
            "fnMap": {
                "0": {
                    "name": "add",
                    "decl": { "start": { "line": 3, "column": 16 }, "end": { "line": 3, "column": 19 } },
                    "loc": { "start": { "line": 3, "column": 51 }, "end": { "line": 5, "column": 1 } },
                    "line": 3
                }
            },
            "f": { "0": 3 }
        }
    }"#;
    let coverage = canonical.join("coverage-final.json");
    // Same forward-slash normalization rationale as `build_jest_fixture`:
    // Windows backslashes break JSON parsing of the substituted
    // template. No-op on macOS/linux.
    std::fs::write(
        &coverage,
        coverage_template.replace(
            "{SRC_ROOT}",
            &canonical.to_string_lossy().replace('\\', "/"),
        ),
    )
    .expect("write coverage-final.json");

    let out = run_crap4ts(&canonical, &coverage, &[]);

    assert!(
        out.status.success(),
        "crap4ts exited non-zero (broken.ts should be skipped, not abort): stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // (a) simple.ts's `add` still surfaces in the table.
    assert!(
        stdout.contains("simple.ts") && stdout.contains("add"),
        "expected simple.ts/add to survive broken.ts's parse failure; stdout=\n{stdout}"
    );
    // (b) stderr emits the per-file `warning: skipping` for broken.ts.
    assert!(
        stderr.contains("warning: skipping"),
        "expected stderr to contain `warning: skipping`; got:\n{stderr}"
    );
    assert!(
        stderr.contains("broken.ts"),
        "expected stderr to name broken.ts in the skip warning; got:\n{stderr}"
    );
}
