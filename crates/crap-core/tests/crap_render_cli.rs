//! Integration test for the `crap-render` binary CLI.
//!
//! Exercises:
//! * `--help` renders cleanly (clap doesn't panic on derived help).
//! * Missing `--input` argument errors out actionably.
//! * Invalid `--input` spec (missing `=`) errors actionably.
//! * Schema-version mismatch produces an actionable error naming
//!   the envelope path and the unsupported version.
//! * Duplicate language key errors actionably.
//! * Valid two-envelope input produces a unified HTML document on
//!   stdout (smoke check; the rendered shape is exercised by the
//!   reporter snapshot test).
//! * Single-envelope input produces a single-language HTML document
//!   byte-identical to the underlying `format_html` path (the
//!   passthrough smoke; covered more comprehensively by
//!   `multi_lang_passthrough.rs`).
//!
//! cargo-mutants note: tests in this file shell out to the
//! `crap-render` binary via `assert_cmd`. The repo-wide
//! `.cargo/mutants.toml` carries a `--skip crap_render` token so
//! scoped `cargo mutants --package crap-core` runs don't trip on
//! the `CARGO_BIN_EXE_crap-render` env var (which only exists
//! inside the bin's own crate's test build).

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

const MINIMAL_ENVELOPE_V2: &str = r#"{
  "schema_version": 2,
  "tool_version": "0.6.0",
  "language": "rust",
  "timestamp": "2026-05-25T12:00:00Z",
  "metric": "cognitive",
  "threshold": 8.0,
  "diff_ref": null,
  "result": {
    "functions": [
      {
        "scored": {
          "identity": {
            "file_path": "src/lib.rs",
            "qualified_name": "compute_crap",
            "span": {
              "start_line": 1,
              "end_line": 10,
              "start_column": 0,
              "end_column": 0
            }
          },
          "complexity": 5,
          "complexity_metric": "cognitive",
          "coverage_percent": 80.0,
          "crap": {
            "value": 5.16,
            "risk_level": "low"
          },
          "contributors": []
        },
        "threshold": 8.0,
        "exceeds": false
      }
    ],
    "summary": {
      "total_functions": 1,
      "total_files": 1,
      "exceeding_threshold": 0,
      "average_crap": 5.16,
      "median_crap": 5.16,
      "max_crap": {
        "value": 5.16,
        "risk_level": "low"
      },
      "worst_function": {
        "file_path": "src/lib.rs",
        "qualified_name": "compute_crap",
        "span": {
          "start_line": 1,
          "end_line": 10,
          "start_column": 0,
          "end_column": 0
        }
      },
      "distribution": {
        "low": 1,
        "acceptable": 0,
        "moderate": 0,
        "high": 0
      }
    },
    "passed": true
  },
  "view": {
    "spec": {
      "filters": {},
      "sort": "crap-desc",
      "limit": null,
      "group_by": null
    },
    "eligible_count": 1,
    "truncated": false,
    "shown_summary": {
      "total_functions": 1,
      "total_files": 1,
      "exceeding_threshold": 0,
      "average_crap": 5.16,
      "median_crap": 5.16,
      "max_crap": {
        "value": 5.16,
        "risk_level": "low"
      },
      "worst_function": null,
      "distribution": {
        "low": 1,
        "acceptable": 0,
        "moderate": 0,
        "high": 0
      }
    },
    "grouped": null
  }
}
"#;

const MINIMAL_ENVELOPE_TS_V2: &str = r#"{
  "schema_version": 2,
  "tool_version": "2.0.0",
  "language": "typescript",
  "timestamp": "2026-05-25T12:00:00Z",
  "metric": "cyclomatic",
  "threshold": 8.0,
  "diff_ref": null,
  "result": {
    "functions": [
      {
        "scored": {
          "identity": {
            "file_path": "src/index.ts",
            "qualified_name": "parseInvoice",
            "span": {
              "start_line": 1,
              "end_line": 20,
              "start_column": 0,
              "end_column": 0
            }
          },
          "complexity": 7,
          "complexity_metric": "cyclomatic",
          "coverage_percent": 65.0,
          "crap": {
            "value": 12.5,
            "risk_level": "moderate"
          },
          "contributors": []
        },
        "threshold": 8.0,
        "exceeds": true
      }
    ],
    "summary": {
      "total_functions": 1,
      "total_files": 1,
      "exceeding_threshold": 1,
      "average_crap": 12.5,
      "median_crap": 12.5,
      "max_crap": {
        "value": 12.5,
        "risk_level": "moderate"
      },
      "worst_function": null,
      "distribution": {
        "low": 0,
        "acceptable": 0,
        "moderate": 1,
        "high": 0
      }
    },
    "passed": false
  },
  "view": {
    "spec": {
      "filters": {},
      "sort": "crap-desc",
      "limit": null,
      "group_by": null
    },
    "eligible_count": 1,
    "truncated": false,
    "shown_summary": {
      "total_functions": 1,
      "total_files": 1,
      "exceeding_threshold": 1,
      "average_crap": 12.5,
      "median_crap": 12.5,
      "max_crap": {
        "value": 12.5,
        "risk_level": "moderate"
      },
      "worst_function": null,
      "distribution": {
        "low": 0,
        "acceptable": 0,
        "moderate": 1,
        "high": 0
      }
    },
    "grouped": null
  }
}
"#;

#[test]
fn crap_render_help_includes_usage() {
    let out = Command::cargo_bin("crap-render")
        .unwrap()
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success(), "--help should exit 0");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("--input"));
    assert!(stdout.contains("--format"));
    assert!(stdout.contains("--output"));
}

#[test]
fn crap_render_requires_at_least_one_input() {
    let out = Command::cargo_bin("crap-render").unwrap().output().unwrap();
    assert!(!out.status.success(), "no --input → non-zero exit");
    let stderr = String::from_utf8(out.stderr).unwrap();
    // clap's required-args error message convention.
    assert!(
        stderr.contains("--input") || stderr.contains("required"),
        "stderr should mention the missing required argument; got: {stderr}"
    );
}

#[test]
fn crap_render_rejects_invalid_input_spec() {
    let out = Command::cargo_bin("crap-render")
        .unwrap()
        .arg("--input")
        .arg("crap4rs.json")
        .output()
        .unwrap();
    assert!(!out.status.success(), "no '=' in spec → non-zero exit");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("<LANG>=<FILE>"),
        "stderr should mention the expected spec shape; got: {stderr}"
    );
}

#[test]
fn crap_render_rejects_schema_version_mismatch() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("bad-schema.json");
    let bad_envelope =
        MINIMAL_ENVELOPE_V2.replace(r#""schema_version": 2"#, r#""schema_version": 99"#);
    fs::write(&path, bad_envelope).unwrap();

    let out = Command::cargo_bin("crap-render")
        .unwrap()
        .arg("--input")
        .arg(format!("rust={}", path.display()))
        .output()
        .unwrap();
    assert!(!out.status.success(), "schema mismatch → non-zero exit");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("schema_version 99"),
        "stderr should name the bad version; got: {stderr}"
    );
    assert!(
        stderr.contains(path.file_name().unwrap().to_str().unwrap()),
        "stderr should name the offending envelope path; got: {stderr}"
    );
}

#[test]
fn crap_render_rejects_duplicate_language() {
    let tmp = TempDir::new().unwrap();
    let path_a = tmp.path().join("a.json");
    let path_b = tmp.path().join("b.json");
    fs::write(&path_a, MINIMAL_ENVELOPE_V2).unwrap();
    fs::write(&path_b, MINIMAL_ENVELOPE_V2).unwrap();

    let out = Command::cargo_bin("crap-render")
        .unwrap()
        .arg("--input")
        .arg(format!("rust={}", path_a.display()))
        .arg("--input")
        .arg(format!("rust={}", path_b.display()))
        .output()
        .unwrap();
    assert!(!out.status.success(), "duplicate language → non-zero exit");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("duplicate input for language 'rust'"),
        "stderr should mention the duplicate language; got: {stderr}"
    );
}

#[test]
fn crap_render_renders_two_language_unified_html() {
    let tmp = TempDir::new().unwrap();
    let rs_path = tmp.path().join("rs.json");
    let ts_path = tmp.path().join("ts.json");
    fs::write(&rs_path, MINIMAL_ENVELOPE_V2).unwrap();
    fs::write(&ts_path, MINIMAL_ENVELOPE_TS_V2).unwrap();

    let out = Command::cargo_bin("crap-render")
        .unwrap()
        .arg("--input")
        .arg(format!("rust={}", rs_path.display()))
        .arg("--input")
        .arg(format!("typescript={}", ts_path.display()))
        .arg("--format")
        .arg("html")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "valid 2-envelope render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.starts_with("<!doctype html>"),
        "rendered HTML should start with doctype"
    );
    assert!(
        stdout.contains("data-multi-lang"),
        "multi-lang body marker should be present"
    );
    assert!(
        stdout.contains("data-lang=\"rust\""),
        "rust language nav button should render"
    );
    assert!(
        stdout.contains("data-lang=\"typescript\""),
        "typescript language nav button should render"
    );
    assert!(
        stdout.contains("data-lang=\"combined\""),
        "combined panel should render"
    );
    assert!(
        stdout.contains("parseInvoice"),
        "TS function should appear in the output"
    );
    assert!(
        stdout.contains("compute_crap"),
        "Rust function should appear in the output"
    );
}

#[test]
fn crap_render_writes_to_output_file() {
    let tmp = TempDir::new().unwrap();
    let rs_path = tmp.path().join("rs.json");
    let out_path = tmp.path().join("unified.html");
    fs::write(&rs_path, MINIMAL_ENVELOPE_V2).unwrap();

    let out = Command::cargo_bin("crap-render")
        .unwrap()
        .arg("--input")
        .arg(format!("rust={}", rs_path.display()))
        .arg("--output")
        .arg(out_path.to_str().unwrap())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "writing to file should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out_path.exists(), "output file should be created");
    let content = fs::read_to_string(&out_path).unwrap();
    assert!(content.starts_with("<!doctype html>"));
    // Single-language passthrough: no multi-lang chrome.
    assert!(!content.contains("data-multi-lang"));
}

#[test]
fn crap_render_accepts_schema_version_1_for_legacy_envelopes() {
    // The baseline loader accepts v1 too (per
    // adapters::baseline::SUPPORTED_SCHEMA_VERSIONS); crap-render
    // mirrors that range so a workspace mid-upgrade (one adapter on
    // v1, one on v2) still composes.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("v1.json");
    let v1_envelope =
        MINIMAL_ENVELOPE_V2.replace(r#""schema_version": 2"#, r#""schema_version": 1"#);
    fs::write(&path, v1_envelope).unwrap();

    let out = Command::cargo_bin("crap-render")
        .unwrap()
        .arg("--input")
        .arg(format!("rust={}", path.display()))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "schema v1 should be accepted; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn crap_render_rejects_baseline_without_matching_input() {
    // Characterization (crap-rs#336): a `--baseline` whose language key
    // has no matching `--input` is an operator error. Asserts the
    // validation stays actionable now that `run`'s checks live in an
    // extracted helper.
    let tmp = TempDir::new().unwrap();
    let rs_path = tmp.path().join("rs.json");
    let ts_baseline = tmp.path().join("ts-baseline.json");
    fs::write(&rs_path, MINIMAL_ENVELOPE_V2).unwrap();
    fs::write(&ts_baseline, MINIMAL_ENVELOPE_TS_V2).unwrap();

    let out = Command::cargo_bin("crap-render")
        .unwrap()
        .arg("--input")
        .arg(format!("rust={}", rs_path.display()))
        .arg("--baseline")
        .arg(format!("typescript={}", ts_baseline.display()))
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "baseline with no matching input → non-zero exit"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("has no matching --input"),
        "stderr should name the unmatched baseline; got: {stderr}"
    );
}

#[test]
fn crap_render_rejects_duplicate_baseline() {
    // Characterization (crap-rs#336): two `--baseline` for the same
    // language key is an operator error. Asserts this stays an operator
    // error now that `run`'s validation lives in an extracted helper.
    let tmp = TempDir::new().unwrap();
    let rs_path = tmp.path().join("rs.json");
    let base_a = tmp.path().join("base-a.json");
    let base_b = tmp.path().join("base-b.json");
    fs::write(&rs_path, MINIMAL_ENVELOPE_V2).unwrap();
    fs::write(&base_a, MINIMAL_ENVELOPE_V2).unwrap();
    fs::write(&base_b, MINIMAL_ENVELOPE_V2).unwrap();

    let out = Command::cargo_bin("crap-render")
        .unwrap()
        .arg("--input")
        .arg(format!("rust={}", rs_path.display()))
        .arg("--baseline")
        .arg(format!("rust={}", base_a.display()))
        .arg("--baseline")
        .arg(format!("rust={}", base_b.display()))
        .output()
        .unwrap();
    assert!(!out.status.success(), "duplicate baseline → non-zero exit");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("duplicate baseline for language 'rust'"),
        "stderr should name the duplicate baseline; got: {stderr}"
    );
}

/// crap-rs#379: the effective `epsilon` travels on the input envelope, so
/// `crap-render`'s recomputed delta carries the active band and the rendered
/// report shows the border-jitter line even when nothing was suppressed.
/// Without epsilon on the envelope the line stays hidden — a byte-level
/// contrast proving the signal comes from the wire, not a default.
#[test]
fn crap_render_shows_border_jitter_line_when_envelope_carries_epsilon() {
    let tmp = TempDir::new().unwrap();

    // Input envelope WITH an active band; the baseline pairs by language so a
    // delta is recomputed. Identical results on both sides → zero suppressed,
    // band active → the line must still render (the #379 case).
    let input_with_eps = MINIMAL_ENVELOPE_V2.replace(
        "\"schema_version\": 2,",
        "\"schema_version\": 2,\n  \"epsilon\": 0.5,",
    );
    let in_path = tmp.path().join("input.json");
    let base_path = tmp.path().join("baseline.json");
    fs::write(&in_path, &input_with_eps).unwrap();
    fs::write(&base_path, MINIMAL_ENVELOPE_V2).unwrap();

    let out = Command::cargo_bin("crap-render")
        .unwrap()
        .arg("--input")
        .arg(format!("rust={}", in_path.display()))
        .arg("--baseline")
        .arg(format!("rust={}", base_path.display()))
        .arg("--format")
        .arg("html")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let html = String::from_utf8(out.stdout).unwrap();
    assert!(
        html.contains("border-jitter suppressed"),
        "an envelope-carried active epsilon must surface the border-jitter line"
    );

    // Contrast: no epsilon on the envelope → band off → line hidden.
    let in_no_eps = tmp.path().join("input_no_eps.json");
    fs::write(&in_no_eps, MINIMAL_ENVELOPE_V2).unwrap();
    let out0 = Command::cargo_bin("crap-render")
        .unwrap()
        .arg("--input")
        .arg(format!("rust={}", in_no_eps.display()))
        .arg("--baseline")
        .arg(format!("rust={}", base_path.display()))
        .arg("--format")
        .arg("html")
        .output()
        .unwrap();
    assert!(out0.status.success());
    let html0 = String::from_utf8(out0.stdout).unwrap();
    assert!(
        !html0.contains("border-jitter suppressed"),
        "no envelope epsilon → the border-jitter line stays hidden"
    );
}

/// crap-rs#379 on its literal surface: TWO languages through `crap-render`
/// with an active epsilon band must produce a **combined** panel that shows
/// the border-jitter line even at zero suppressed. The unit tests cover each
/// link (epsilon transport for one language; the in-memory combined render);
/// this guards the exact composition — two `--input` + two `--baseline` +
/// combined panel — that was the reported bug.
#[test]
fn crap_render_combined_panel_shows_border_jitter_band_across_languages() {
    let tmp = TempDir::new().unwrap();
    // Inputs carry an active band; baselines are identical → zero suppressed,
    // band active → every panel (both per-language + the combined one) shows
    // the line. The combined render is the one #379 was about.
    let with_eps = |env: &str| {
        env.replace(
            "\"schema_version\": 2,",
            "\"schema_version\": 2,\n  \"epsilon\": 0.5,",
        )
    };
    let rs_in = tmp.path().join("rs_in.json");
    let ts_in = tmp.path().join("ts_in.json");
    let rs_base = tmp.path().join("rs_base.json");
    let ts_base = tmp.path().join("ts_base.json");
    fs::write(&rs_in, with_eps(MINIMAL_ENVELOPE_V2)).unwrap();
    fs::write(&ts_in, with_eps(MINIMAL_ENVELOPE_TS_V2)).unwrap();
    fs::write(&rs_base, MINIMAL_ENVELOPE_V2).unwrap();
    fs::write(&ts_base, MINIMAL_ENVELOPE_TS_V2).unwrap();

    let out = Command::cargo_bin("crap-render")
        .unwrap()
        .args([
            "--input",
            &format!("rust={}", rs_in.display()),
            "--input",
            &format!("typescript={}", ts_in.display()),
            "--baseline",
            &format!("rust={}", rs_base.display()),
            "--baseline",
            &format!("typescript={}", ts_base.display()),
            "--format",
            "html",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "two-language render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let html = String::from_utf8(out.stdout).unwrap();
    // Scope the assertion to the COMBINED panel specifically — its verdict
    // span carries aria-label "Combined Delta verdict", and the border-jitter
    // text lives in the sibling `<span class="meta">` inside the same `<p>`
    // (closed by the next `</p>`). A whole-document count could be satisfied
    // by the per-language panels alone; this pins the combined one.
    let combined_start = html
        .find("Combined Delta verdict")
        .expect("a two-language render must include the combined panel");
    let combined_meta = &html[combined_start..];
    let combined_meta = combined_meta
        .find("</p>")
        .map_or(combined_meta, |end| &combined_meta[..end]);
    assert!(
        combined_meta.contains("border-jitter suppressed"),
        "the COMBINED panel must show the border-jitter line at an active band; combined block was: {combined_meta}"
    );
}
