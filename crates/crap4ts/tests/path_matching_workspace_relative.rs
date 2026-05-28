//! Integration tests for issue #331 — Istanbul coverage matching must
//! work when `path` keys are **workspace-relative** (e.g.
//! `crates/foo/ts/bar.ts`), regardless of whether `--src` is a
//! bare-relative or absolute path.
//!
//! Pre-fix, `IstanbulCoverage::normalize_path` arm 1 lexically joined a
//! relative `path` under the canonical `effective_src` and stripped the
//! prefix back off, yielding the workspace-relative path verbatim — a
//! string that doesn't resolve to a real file and never matches the
//! walker's src-relative key. Arm 1 returned `Some(invalid)` *before*
//! arm 2's filesystem-validated suffix match could fire, so every
//! function reported `coverage_percent: 0`.
//!
//! These fixtures place a real `.ts` file on disk and use
//! workspace-relative `path` keys, so the suffix-match fallback is
//! actually exercised.

use std::path::{Path, PathBuf};

use assert_cmd::Command;

/// A single-line, fully-covered TS function. Its body occupies line 1,
/// which the coverage fixture below marks hit — so the matched function
/// must score exactly 100.0%.
const ADD_TS: &str = "export function add(a: number, b: number): number { return a + b; }\n";

/// Workspace-relative Istanbul coverage for `pkg/ts/add.ts`. The top
/// level key AND the `path` field are workspace-relative — the natural
/// shape a coverage tool emits when run from the workspace root.
const COVERAGE_JSON: &str = r#"{
    "pkg/ts/add.ts": {
        "path": "pkg/ts/add.ts",
        "statementMap": {
            "0": { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 68 } }
        },
        "s": { "0": 3 },
        "branchMap": {},
        "b": {},
        "fnMap": {},
        "f": {}
    }
}"#;

/// Lay out a workspace-shaped tree under a canonicalised tempdir:
///   - `pkg/ts/add.ts` carrying `ADD_TS`.
///   - `coverage-final.json` with workspace-relative `path` keys.
///
/// Returns `(TempDir, canonical_root)`. The binary is invoked with
/// `current_dir(canonical_root)`, so the root plays the role of the
/// workspace root that relative paths resolve against.
fn setup_dir() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Canonicalize: macOS /tmp redirects to /private/tmp, and the
    // orchestrator canonicalizes `effective_src`. Without this the
    // suffix-match anchors against a different prefix than the walker.
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");

    let ts_dir = canonical.join("pkg").join("ts");
    std::fs::create_dir_all(&ts_dir).expect("create pkg/ts");
    std::fs::write(ts_dir.join("add.ts"), ADD_TS).expect("write add.ts");
    std::fs::write(canonical.join("coverage-final.json"), COVERAGE_JSON)
        .expect("write coverage-final.json");

    (tmp, canonical)
}

/// Run `crap4ts --src <src_arg> --coverage coverage-final.json` with
/// `current_dir(root)` and return the matched function's coverage
/// percent for `add`.
fn add_coverage_percent(root: &Path, src_arg: &str) -> f64 {
    let out = Command::cargo_bin("crap4ts")
        .expect("crap4ts binary discoverable")
        .current_dir(root)
        .args([
            "--src",
            src_arg,
            "--coverage",
            "coverage-final.json",
            "--threshold",
            "16",
            "--no-fail",
            "--format",
            "json",
        ])
        .output()
        .expect("crap4ts executes");

    assert!(
        out.status.success(),
        "crap4ts exited non-zero: stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let value: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("crap4ts --format json emits valid JSON");
    let functions = value["result"]["functions"]
        .as_array()
        .expect("envelope must carry a result.functions array");
    let add = functions
        .iter()
        .find(|f| f["scored"]["identity"]["qualified_name"] == "add")
        .unwrap_or_else(|| panic!("expected an `add` function in the envelope; got {value:#}"));
    add["scored"]["coverage_percent"]
        .as_f64()
        .expect("coverage_percent is a number")
}

#[test]
fn bare_relative_src_with_workspace_relative_coverage_matches() {
    let (_tmp, root) = setup_dir();
    // Bare-relative `--src` + workspace-relative `path` keys. `add`
    // occupies a single fully-hit line, so it must score 100.0%.
    assert_eq!(
        add_coverage_percent(&root, "pkg/ts"),
        100.0,
        "workspace-relative coverage paths must match the walker's \
         src-relative key under a bare-relative --src (see #331)"
    );
}

#[test]
fn absolute_src_with_workspace_relative_coverage_matches() {
    let (_tmp, root) = setup_dir();
    // Absolute `--src` + workspace-relative `path` keys — only reachable
    // through the filesystem suffix-match fallback (arm 1's lexical
    // strip produces a nonexistent path here).
    let abs_src = root.join("pkg").join("ts");
    assert_eq!(
        add_coverage_percent(&root, &abs_src.to_string_lossy()),
        100.0,
        "workspace-relative coverage paths must match under an absolute --src too (see #331)"
    );
}
