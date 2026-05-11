//! Integration tests for issue #150 — LcovParser path-strip must honor
//! the effective source root *after* CLI/config-file merge.
//!
//! The bug fixture sets `src` only in `crap4rs.toml` (not on the CLI).
//! Pre-fix, `crates/crap4rs/src/main.rs` constructed `LcovParser::new`
//! from `cli.input.src` *before* config merging, so the parser stripped
//! the default `src/` prefix from absolute `SF:` records emitted by
//! `cargo llvm-cov`. That left coverage-map keys as absolute paths, the
//! walker emitted src-relative keys, and `match_functions` found no
//! overlap → coverage silently dropped to 0.
//!
//! Absolute `SF:` paths are mandatory here. Relative-path LCOV fixtures
//! short-circuit `strip_prefix(...).unwrap_or(p)` and look like they
//! "work" regardless of which root the parser uses — they don't surface
//! the strip-prefix-mismatch bug.

use std::path::Path;
use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

const FIXTURE_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn passing_c() -> i32 { 3 }
";

/// Custom source-directory name. NOT `src` — using a non-default name
/// is what forces `cli.input.src` to be `None` and the effective `src`
/// to come exclusively from `crap4rs.toml`.
const CUSTOM_SRC_DIR: &str = "myproject_source";

/// Set up the fixture in `dir`:
///   - Create `<CUSTOM_SRC_DIR>/lib.rs` with `FIXTURE_SRC`.
///   - Write `crap4rs.toml` with `src = "<CUSTOM_SRC_DIR>"`.
///   - Write `lcov.info` with **absolute** `SF:` paths pointing into
///     the canonicalized custom source dir — this is what
///     `cargo llvm-cov` emits in CI.
///
/// Returns the canonicalized custom source dir for assertions.
fn setup_dir(dir: &Path) -> std::path::PathBuf {
    let src = dir.join(CUSTOM_SRC_DIR);
    std::fs::create_dir_all(&src).expect("create custom src dir");
    std::fs::write(src.join("lib.rs"), FIXTURE_SRC).expect("write lib.rs fixture");

    let src_canonical = src.canonicalize().expect("canonicalize custom src dir");
    let lib_rs_absolute = src_canonical.join("lib.rs");

    // `cargo-llvm-cov` emits forward-slash `SF:` paths even on
    // Windows; normalize here so the fixture stays portable if
    // Windows joins the CI matrix later.
    let lib_rs_sf = lib_rs_absolute.to_string_lossy().replace('\\', "/");
    let lcov = format!("SF:{lib_rs_sf}\nDA:1,1\nDA:2,1\nDA:3,1\nend_of_record\n");
    std::fs::write(dir.join("lcov.info"), lcov).expect("write lcov.info fixture");

    let toml = format!("src = \"{CUSTOM_SRC_DIR}\"\n");
    std::fs::write(dir.join("crap4rs.toml"), toml).expect("write crap4rs.toml fixture");

    src_canonical
}

fn run_without_src_flag(dir: &Path) -> std::process::Output {
    Command::new(BINARY)
        .current_dir(dir)
        // Deliberately omit `--src` so the effective source root comes
        // from `crap4rs.toml` alone (the path that triggered #150).
        .args([
            "--coverage",
            "lcov.info",
            "--no-gitignore",
            "--threshold",
            "5",
            "--no-fail",
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run crap4rs binary")
}

#[test]
fn config_only_src_strips_lcov_path_prefix() {
    // #150 acceptance criterion 2:
    //   "Add a config-file integration test where `[src] = ".../foo"`
    //    only appears in `crap4rs.toml`; assert the LCOV parser strips
    //    that prefix."
    //
    // After the fix, the parser strips the canonical custom-src prefix
    // from the absolute `SF:` path, leaving `lib.rs` as the coverage
    // key. That matches the walker's `lib.rs` and the scored result
    // contains a function with non-zero coverage.
    let dir = tempfile::tempdir().expect("create tempdir");
    let _src_canonical = setup_dir(dir.path());

    let output = run_without_src_flag(dir.path());

    assert!(
        output.status.success(),
        "binary exited non-zero (stderr=\n{}\n stdout=\n{})",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nraw stdout:\n{stdout}"));

    let functions = json["result"]["functions"]
        .as_array()
        .expect("envelope must carry a result.functions array");

    assert!(
        !functions.is_empty(),
        "expected at least one scored function; envelope = {json:#?}"
    );

    // The walker emits src-relative file_path = `lib.rs`. Pre-fix, the
    // parser kept the absolute `SF:` path in its coverage map, so the
    // `lib.rs` complexity entries had no matching coverage row and the
    // scored `coverage_percent` was 0.0 across the board. After the fix
    // the parser strips the canonical custom-src prefix and the
    // coverage rows align with the walker keys.
    //
    // The fixture's LCOV marks lines 1–3 as fully hit (`DA:N,1`) and
    // the three functions each occupy a single line, so every function
    // must score exactly 100.0. Asserting on `> 0.0` alone would
    // tolerate a partial-strip regression (some keys matched, others
    // mis-keyed), which the per-function check catches.
    let coverage_percents: Vec<f64> = functions
        .iter()
        .map(|f| f["scored"]["coverage_percent"].as_f64().unwrap_or(-1.0))
        .collect();

    assert_eq!(
        coverage_percents,
        vec![100.0, 100.0, 100.0],
        "every function should score 100.0% with the fixture's full \
         line hits; LCOV path-strip likely did not honor the \
         config-file `src` (see #150). functions = {functions:#?}"
    );
}
