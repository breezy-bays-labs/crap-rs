//! Cucumber-rs runner for `@wired`-tagged scenarios in
//! `tests/features/multi_lang_html.feature` (crap-rs#315).
//!
//! The harness exercises the `crap-render` binary (which ships in
//! crap-core) plus the composite scorecard action's documented gate
//! behavior. Each scenario sets up tempdir-backed JSON envelopes,
//! invokes crap-render via `CARGO_BIN_EXE_crap-render`, and asserts
//! on the rendered HTML or the binary's exit code + error message.
//!
//! Per AGENTS.md § BDD hygiene rule 5, the harness filters scenarios
//! by the `@wired` tag — `@unwired` scenarios (carrying a
//! `# tracked: crap-rs#<n>` comment) are skipped by design until
//! their step definitions land.
//!
//! The library-synthesis pattern: rather than producing real
//! envelopes by shelling out to crap4rs/crap4ts (which would require
//! per-platform coverage fixtures), each Given step writes a
//! hand-crafted minimal envelope JSON to a tempdir. Same approach as
//! the `crap_render_cli.rs` integration test in crap-core.
//!
//! The composite-action scenario (`single-language composite-action
//! mode emits no unified-render artifact`) is verified by reading
//! `action.yml` for the documented `if:` gate condition rather than
//! actually running a workflow — that gate is the contract.

use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use cucumber::{World, given, then, when, writer};

#[derive(Debug, Default, World)]
struct MultiLangWorld {
    /// Tempdir holding any envelope JSON files the scenario builds.
    /// Held so the directory survives the scenario's lifetime.
    _tempdir: Option<tempfile::TempDir>,
    /// Per-language envelope paths the scenario has produced. Keyed
    /// by language so duplicates can be intentionally tested.
    envelopes: Vec<(String, PathBuf)>,
    /// Per-language baseline envelope paths (View axis scenarios).
    /// Keyed by language; same dedup discipline as `envelopes`.
    baselines: Vec<(String, PathBuf)>,
    /// Output of the last crap-render invocation. None until the
    /// scenario's When step has run.
    output: Option<Output>,
    /// Loaded contents of `.github/actions/scorecard/action.yml` for
    /// the composite-action scenario. Set by the Given step that
    /// declares a workspace configured with a single language adapter.
    /// Captured during Given so the When step has executable work to
    /// do (parse + validate structural fitness) and the Then steps
    /// share a single read.
    action_yml: Option<String>,
}

impl MultiLangWorld {
    fn require_output(&self) -> &Output {
        self.output
            .as_ref()
            .expect("scenario did not run crap-render")
    }

    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.require_output().stdout).into_owned()
    }

    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.require_output().stderr).into_owned()
    }
}

/// Build a minimal v2 envelope JSON for the given language, with one
/// function carrying the supplied complexity/coverage/CRAP/risk so
/// the ranked-table sort scenarios have predictable rows.
///
/// `#[allow(clippy::too_many_arguments)]`: each parameter drives a
/// distinct cell of the synthesized JSON (identity, file, metric,
/// scoring, classification, complexity, coverage); collapsing them
/// into a struct would just rename the noise without adding
/// type-safety value for a test helper called from a handful of
/// scenarios. Keep them positional for readability at call sites.
#[allow(clippy::too_many_arguments)]
fn synth_envelope(
    language: &str,
    qualified_name: &str,
    file_path: &str,
    metric: &str,
    crap: f64,
    threshold: f64,
    risk: &str,
    complexity: u32,
    coverage: f64,
) -> String {
    let exceeds = crap > threshold;
    let risk_distribution = match risk {
        "low" => r#"{"low":1,"acceptable":0,"moderate":0,"high":0}"#,
        "acceptable" => r#"{"low":0,"acceptable":1,"moderate":0,"high":0}"#,
        "moderate" => r#"{"low":0,"acceptable":0,"moderate":1,"high":0}"#,
        "high" => r#"{"low":0,"acceptable":0,"moderate":0,"high":1}"#,
        other => panic!("unknown risk level '{other}'"),
    };
    format!(
        r#"{{
  "schema_version": 2,
  "tool_version": "0.0.0-test",
  "language": "{language}",
  "timestamp": "2026-05-25T12:00:00Z",
  "metric": "{metric}",
  "threshold": {threshold},
  "diff_ref": null,
  "result": {{
    "functions": [
      {{
        "scored": {{
          "identity": {{
            "file_path": "{file_path}",
            "qualified_name": "{qualified_name}",
            "span": {{"start_line": 1, "end_line": 10, "start_column": 0, "end_column": 0}}
          }},
          "complexity": {complexity},
          "complexity_metric": "{metric}",
          "coverage_percent": {coverage},
          "crap": {{"value": {crap}, "risk_level": "{risk}"}},
          "contributors": []
        }},
        "threshold": {threshold},
        "exceeds": {exceeds}
      }}
    ],
    "summary": {{
      "total_functions": 1,
      "total_files": 1,
      "exceeding_threshold": {exceeding},
      "average_crap": {crap},
      "median_crap": {crap},
      "max_crap": {{"value": {crap}, "risk_level": "{risk}"}},
      "worst_function": null,
      "distribution": {risk_distribution}
    }},
    "passed": {passed}
  }},
  "view": {{
    "spec": {{"filters": {{}}, "sort": "crap-desc", "limit": null, "group_by": null}},
    "eligible_count": 1,
    "truncated": false,
    "shown_summary": {{
      "total_functions": 1,
      "total_files": 1,
      "exceeding_threshold": {exceeding},
      "average_crap": {crap},
      "median_crap": {crap},
      "max_crap": {{"value": {crap}, "risk_level": "{risk}"}},
      "worst_function": null,
      "distribution": {risk_distribution}
    }},
    "grouped": null
  }}
}}
"#,
        exceeding = if exceeds { 1 } else { 0 },
        passed = !exceeds,
    )
}

fn fresh_tempdir(world: &mut MultiLangWorld) -> PathBuf {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    world._tempdir = Some(dir);
    path
}

fn write_envelope(world: &mut MultiLangWorld, language: &str, filename: &str, body: &str) {
    let dir = match world._tempdir.as_ref() {
        Some(d) => d.path().to_path_buf(),
        None => fresh_tempdir(world),
    };
    let path = dir.join(filename);
    std::fs::write(&path, body).expect("write envelope");
    world.envelopes.push((language.to_string(), path));
}

fn write_baseline(world: &mut MultiLangWorld, language: &str, filename: &str, body: &str) {
    let dir = match world._tempdir.as_ref() {
        Some(d) => d.path().to_path_buf(),
        None => fresh_tempdir(world),
    };
    let path = dir.join(filename);
    std::fs::write(&path, body).expect("write baseline envelope");
    world.baselines.push((language.to_string(), path));
}

// ── Given steps ──────────────────────────────────────────────────────

#[given("a crap4rs JSON envelope from a representative workspace")]
fn given_crap4rs_envelope_representative(world: &mut MultiLangWorld) {
    let body = synth_envelope(
        "rust",
        "compute_crap",
        "src/lib.rs",
        "cognitive",
        5.16,
        8.0,
        "low",
        5,
        80.0,
    );
    write_envelope(world, "rust", "crap4rs.json", &body);
}

#[given("a crap4rs JSON envelope and a crap4ts JSON envelope from one workspace")]
fn given_two_language_envelopes(world: &mut MultiLangWorld) {
    let rs = synth_envelope(
        "rust",
        "compute_crap",
        "src/lib.rs",
        "cognitive",
        5.16,
        8.0,
        "low",
        5,
        80.0,
    );
    let ts = synth_envelope(
        "typescript",
        "parseInvoice",
        "src/parse.ts",
        "cyclomatic",
        12.0,
        8.0,
        "moderate",
        7,
        65.0,
    );
    write_envelope(world, "rust", "crap4rs.json", &rs);
    write_envelope(world, "typescript", "crap4ts.json", &ts);
}

#[given("a crap4rs envelope with one High-risk function at CRAP/threshold ratio 5.7")]
fn given_crap4rs_high_risk_envelope(world: &mut MultiLangWorld) {
    // ratio = 5.7 = 45.6 / 8.0
    let body = synth_envelope(
        "rust",
        "view::analyze_view",
        "src/domain/view.rs",
        "cognitive",
        45.6,
        8.0,
        "high",
        20,
        30.0,
    );
    write_envelope(world, "rust", "crap4rs.json", &body);
}

#[given("a crap4ts envelope with one Moderate-risk function at CRAP/threshold ratio 2.5")]
fn given_crap4ts_moderate_envelope(world: &mut MultiLangWorld) {
    // ratio = 2.5 = 20.0 / 8.0
    let body = synth_envelope(
        "typescript",
        "parseInvoice",
        "src/parse.ts",
        "cyclomatic",
        20.0,
        8.0,
        "moderate",
        10,
        60.0,
    );
    write_envelope(world, "typescript", "crap4ts.json", &body);
}

#[given("two JSON envelopes carrying different schema_version values")]
fn given_mismatched_schema_envelopes(world: &mut MultiLangWorld) {
    let good = synth_envelope(
        "rust",
        "ok",
        "src/lib.rs",
        "cognitive",
        4.0,
        8.0,
        "low",
        3,
        90.0,
    );
    // Replace schema_version with an unsupported value (99).
    let bad = synth_envelope(
        "typescript",
        "bad",
        "src/lib.ts",
        "cyclomatic",
        4.0,
        8.0,
        "low",
        3,
        90.0,
    )
    .replace(r#""schema_version": 2"#, r#""schema_version": 99"#);
    write_envelope(world, "rust", "good.json", &good);
    write_envelope(world, "typescript", "bad.json", &bad);
}

#[given("two crap4rs JSON envelopes from the same workspace")]
fn given_two_crap4rs_envelopes(world: &mut MultiLangWorld) {
    let body = synth_envelope(
        "rust",
        "compute_crap",
        "src/lib.rs",
        "cognitive",
        5.16,
        8.0,
        "low",
        5,
        80.0,
    );
    // Both envelopes labeled 'rust' so the duplicate-language guard
    // trips; the synthesized content is identical.
    write_envelope(world, "rust", "a.json", &body);
    write_envelope(world, "rust", "b.json", &body);
}

#[given("a crap4rs envelope and a crap4ts envelope with matching baselines for each language")]
fn given_two_lang_with_both_baselines(world: &mut MultiLangWorld) {
    // Current envelopes — same shape as the existing two-language
    // scenario so the rest of the structural assertions hold.
    let rs_current = synth_envelope(
        "rust",
        "compute_crap",
        "src/lib.rs",
        "cognitive",
        12.0,
        8.0,
        "moderate",
        7,
        65.0,
    );
    let ts_current = synth_envelope(
        "typescript",
        "parseInvoice",
        "src/parse.ts",
        "cyclomatic",
        14.0,
        8.0,
        "moderate",
        8,
        55.0,
    );
    write_envelope(world, "rust", "crap4rs.json", &rs_current);
    write_envelope(world, "typescript", "crap4ts.json", &ts_current);

    // Baselines — earlier snapshots of the same identities so the
    // delta computer pairs them as Modified rather than Added.
    let rs_baseline = synth_envelope(
        "rust",
        "compute_crap",
        "src/lib.rs",
        "cognitive",
        6.0,
        8.0,
        "acceptable",
        4,
        75.0,
    );
    let ts_baseline = synth_envelope(
        "typescript",
        "parseInvoice",
        "src/parse.ts",
        "cyclomatic",
        8.0,
        8.0,
        "acceptable",
        5,
        70.0,
    );
    write_baseline(world, "rust", "crap4rs-baseline.json", &rs_baseline);
    write_baseline(world, "typescript", "crap4ts-baseline.json", &ts_baseline);
}

#[given("a crap4rs envelope with a matching baseline and a crap4ts envelope without a baseline")]
fn given_mismatched_baselines(world: &mut MultiLangWorld) {
    let rs_current = synth_envelope(
        "rust",
        "compute_crap",
        "src/lib.rs",
        "cognitive",
        10.0,
        8.0,
        "acceptable",
        6,
        70.0,
    );
    let ts_current = synth_envelope(
        "typescript",
        "parseInvoice",
        "src/parse.ts",
        "cyclomatic",
        12.0,
        8.0,
        "moderate",
        7,
        65.0,
    );
    write_envelope(world, "rust", "crap4rs.json", &rs_current);
    write_envelope(world, "typescript", "crap4ts.json", &ts_current);

    let rs_baseline = synth_envelope(
        "rust",
        "compute_crap",
        "src/lib.rs",
        "cognitive",
        5.0,
        8.0,
        "low",
        3,
        85.0,
    );
    write_baseline(world, "rust", "crap4rs-baseline.json", &rs_baseline);
    // TypeScript: NO baseline supplied — the disabled-tab path under
    // test.
}

#[given("a Rust baseline plus a Rust current with one High-risk regression at ratio 5.7")]
fn given_rust_high_risk_regression(world: &mut MultiLangWorld) {
    let rs_baseline = synth_envelope(
        "rust",
        "view::analyze_view",
        "src/domain/view.rs",
        "cognitive",
        6.0,
        8.0,
        "acceptable",
        4,
        80.0,
    );
    let rs_current = synth_envelope(
        "rust",
        "view::analyze_view",
        "src/domain/view.rs",
        "cognitive",
        45.6,
        8.0,
        "high",
        20,
        30.0,
    );
    write_baseline(world, "rust", "crap4rs-baseline.json", &rs_baseline);
    write_envelope(world, "rust", "crap4rs.json", &rs_current);
}

#[given(
    "a TypeScript baseline plus a TypeScript current with one Moderate-risk regression at ratio 2.5"
)]
fn given_typescript_moderate_regression(world: &mut MultiLangWorld) {
    let ts_baseline = synth_envelope(
        "typescript",
        "parseInvoice",
        "src/parse.ts",
        "cyclomatic",
        6.0,
        8.0,
        "acceptable",
        4,
        80.0,
    );
    let ts_current = synth_envelope(
        "typescript",
        "parseInvoice",
        "src/parse.ts",
        "cyclomatic",
        20.0,
        8.0,
        "moderate",
        10,
        60.0,
    );
    write_baseline(world, "typescript", "crap4ts-baseline.json", &ts_baseline);
    write_envelope(world, "typescript", "crap4ts.json", &ts_current);
}

#[given("two language envelopes with baselines")]
fn given_two_language_envelopes_with_baselines(world: &mut MultiLangWorld) {
    // Routed through the same helper as the explicit "matching
    // baselines" Given so URL-hash scenarios share fixture data.
    given_two_lang_with_both_baselines(world);
}

#[given("a workspace configured with a single language adapter")]
fn given_workspace_single_language(world: &mut MultiLangWorld) {
    // Composite-action scenario: the contract under test is the
    // declarative gating in `.github/actions/scorecard/action.yml`,
    // not a runtime workflow execution. We capture the action.yml
    // contents here so subsequent steps have shared, scenario-owned
    // state rather than re-reading the file from each Then step.
    let yml = std::fs::read_to_string(action_yml_path())
        .expect("read .github/actions/scorecard/action.yml");
    world.action_yml = Some(yml);
}

// ── When steps ───────────────────────────────────────────────────────

#[when("crap-render is invoked with that single envelope and --format html")]
fn when_run_single_envelope(world: &mut MultiLangWorld) {
    let (lang, path) = world
        .envelopes
        .first()
        .expect("scenario should have built one envelope")
        .clone();
    let mut cmd = Command::cargo_bin("crap-render").expect("crap-render bin discoverable");
    cmd.arg("--input")
        .arg(format!("{}={}", lang, path.display()))
        .arg("--format")
        .arg("html");
    let output = cmd.output().expect("run crap-render");
    world.output = Some(output);
}

#[when("crap-render is invoked with both envelopes and --format html")]
fn when_run_both_envelopes(world: &mut MultiLangWorld) {
    let mut cmd = Command::cargo_bin("crap-render").expect("crap-render bin discoverable");
    for (lang, path) in &world.envelopes {
        cmd.arg("--input")
            .arg(format!("{}={}", lang, path.display()));
    }
    cmd.arg("--format").arg("html");
    let output = cmd.output().expect("run crap-render");
    world.output = Some(output);
}

#[when("crap-render is invoked with both envelopes")]
fn when_run_both_envelopes_default_format(world: &mut MultiLangWorld) {
    let mut cmd = Command::cargo_bin("crap-render").expect("crap-render bin discoverable");
    for (lang, path) in &world.envelopes {
        cmd.arg("--input")
            .arg(format!("{}={}", lang, path.display()));
    }
    let output = cmd.output().expect("run crap-render");
    world.output = Some(output);
}

#[when("crap-render is invoked with both current envelopes plus both baseline envelopes")]
fn when_run_both_envelopes_with_both_baselines(world: &mut MultiLangWorld) {
    let mut cmd = Command::cargo_bin("crap-render").expect("crap-render bin discoverable");
    for (lang, path) in &world.envelopes {
        cmd.arg("--input")
            .arg(format!("{}={}", lang, path.display()));
    }
    for (lang, path) in &world.baselines {
        cmd.arg("--baseline")
            .arg(format!("{}={}", lang, path.display()));
    }
    cmd.arg("--format").arg("html");
    let output = cmd.output().expect("run crap-render");
    world.output = Some(output);
}

#[when("crap-render is invoked with both current envelopes and the Rust baseline only")]
fn when_run_both_envelopes_with_rust_baseline_only(world: &mut MultiLangWorld) {
    // The fixture Given step writes only the Rust baseline, so this
    // is byte-identical in behavior to the "both baselines"
    // invocation — but expressing it as a distinct step keeps the
    // .feature file declarative about intent.
    when_run_both_envelopes_with_both_baselines(world);
}

#[when("crap-render is invoked with both pairs of envelopes")]
fn when_run_both_pairs_of_envelopes(world: &mut MultiLangWorld) {
    when_run_both_envelopes_with_both_baselines(world);
}

#[when("crap-render renders the unified HTML report")]
fn when_renders_unified_html(world: &mut MultiLangWorld) {
    when_run_both_envelopes_with_both_baselines(world);
}

#[when("the composite scorecard action runs with html-report set true and one language")]
fn when_action_single_language(world: &mut MultiLangWorld) {
    // The "run" under test is a contract check, not a workflow
    // execution: assert the captured action.yml is structurally
    // intact (non-empty + contains the top-level `steps:` keyword)
    // so the Then steps' substring assertions run against a real
    // composite-action document rather than a malformed file. The
    // Given step has already loaded the contents; this step performs
    // the contract precondition.
    let yml = world
        .action_yml
        .as_ref()
        .expect("Given step should have loaded action.yml");
    assert!(
        !yml.trim().is_empty(),
        "action.yml must not be empty for the composite-action scenario"
    );
    assert!(
        yml.contains("\nruns:") && yml.contains("\n  steps:"),
        "action.yml must be a composite action with `runs:` + `steps:`; got truncated content"
    );
}

// ── Then steps ───────────────────────────────────────────────────────

#[then("the HTML output is byte-identical to crap4rs --format html on the same workspace")]
fn then_byte_identical_passthrough(world: &mut MultiLangWorld) {
    let out = world.stdout();
    // The single-language passthrough invariant is structurally
    // guaranteed by `format_html_multi`'s short-circuit on
    // `multi.languages.len() == 1` → delegate to `format_html`. We
    // assert the structural marker (no multi-lang chrome) here as
    // the BDD-level proof; the byte-level lock lives in the
    // dedicated `multi_lang_passthrough.rs` smoke test which uses
    // the in-process `format_html` for a true byte comparison.
    assert!(out.starts_with("<!doctype html>"));
    assert!(
        !out.contains("data-multi-lang"),
        "single-language passthrough must produce no multi-lang body marker"
    );
    assert!(
        !out.contains("lang-nav"),
        "single-language passthrough must produce no Language nav"
    );
}

#[then("the output contains no `<nav class=\"segmented\"` markup")]
fn then_output_no_segmented(world: &mut MultiLangWorld) {
    let out = world.stdout();
    assert!(
        !out.contains("<nav class=\"segmented\""),
        "single-language output must not render a segmented nav"
    );
}

#[then("the output contains no Combined panel")]
fn then_output_no_combined(world: &mut MultiLangWorld) {
    let out = world.stdout();
    assert!(
        !out.contains(r#"data-lang="combined""#),
        "single-language output must not render a Combined panel"
    );
}

#[then("the HTML output contains exactly one `<nav class=\"segmented\"` element")]
fn then_output_one_segmented(world: &mut MultiLangWorld) {
    let out = world.stdout();
    let count = out.matches("<nav class=\"lang-nav segmented\"").count();
    assert_eq!(
        count,
        1,
        "expected exactly one Language nav; got {count}\nrendered HTML:\n{}",
        &out[..out.len().min(2000)]
    );
}

#[then("the segmented nav has buttons with data-lang \"rust\", \"typescript\", and \"combined\"")]
fn then_segmented_has_three_buttons(world: &mut MultiLangWorld) {
    let out = world.stdout();
    assert!(out.contains(r#"data-lang="rust""#));
    assert!(out.contains(r#"data-lang="typescript""#));
    assert!(out.contains(r#"data-lang="combined""#));
}

#[then("the Combined panel button is rendered active by default")]
fn then_combined_default_active(world: &mut MultiLangWorld) {
    let out = world.stdout();
    // The Combined button carries `class="seg is-active"` and
    // `aria-pressed="true"`. We assert the joined markers because
    // the template emits them atomically.
    assert!(
        out.contains(r#"<div class="lang-panel" data-lang="combined" data-active>"#),
        "Combined panel must carry data-active marker for default visibility"
    );
}

#[then("the document footer contains an Adapters provenance grid listing both languages")]
fn then_footer_lists_both_languages(world: &mut MultiLangWorld) {
    let out = world.stdout();
    assert!(out.contains("class=\"footer-adapters\""));
    assert!(out.contains(">Rust</"));
    assert!(out.contains(">TypeScript</"));
}

#[then(
    "the Combined panel ranked table lists the Rust High-risk function before the TypeScript Moderate-risk function"
)]
fn then_high_risk_before_moderate(world: &mut MultiLangWorld) {
    let out = world.stdout();
    let rs_pos = out
        .find("view::analyze_view")
        .expect("Rust function should appear");
    let ts_pos = out.find("parseInvoice").expect("TS function should appear");
    assert!(
        rs_pos < ts_pos,
        "expected High-risk Rust row before Moderate-risk TS row (D2d sort)"
    );
}

#[then("each row carries an adapter badge identifying its source language")]
fn then_rows_carry_badges(world: &mut MultiLangWorld) {
    let out = world.stdout();
    assert!(
        out.contains(r#"<span class="adapter-badge""#),
        "ranked rows must carry adapter-badge markup"
    );
}

#[then("crap-render exits with non-zero status")]
fn then_nonzero_exit(world: &mut MultiLangWorld) {
    let status = world.require_output().status;
    assert!(!status.success(), "expected non-zero exit; got {status:?}");
}

#[then(
    "the error message names the offending envelope path and the unsupported schema_version value"
)]
fn then_error_names_schema(world: &mut MultiLangWorld) {
    let stderr = world.stderr();
    assert!(
        stderr.contains("schema_version 99"),
        "stderr should name the offending schema_version value; got: {stderr}"
    );
    assert!(
        stderr.contains("bad.json"),
        "stderr should name the offending envelope path; got: {stderr}"
    );
}

#[then("the error message names the duplicate language")]
fn then_error_names_duplicate(world: &mut MultiLangWorld) {
    let stderr = world.stderr();
    assert!(
        stderr.contains("duplicate input for language 'rust'"),
        "stderr should name the duplicate language key; got: {stderr}"
    );
}

#[then("the workflow produces exactly one HTML artifact named after the adapter")]
fn then_action_one_artifact_named_after_adapter(world: &mut MultiLangWorld) {
    // Verified by reading action.yml — single-language mode
    // produces either `crap4rs-report-*` or `crap4ts-report-*` (not
    // `crap-scorecard-report-*`), guarded by the resolved language.
    let action_yml = world
        .action_yml
        .as_ref()
        .expect("Given step should have loaded action.yml");
    assert!(
        action_yml.contains("crap4rs-report${{ inputs.html-artifact-name-suffix"),
        "action.yml should declare per-language crap4rs artifact upload"
    );
    assert!(
        action_yml.contains("crap4ts-report${{ inputs.html-artifact-name-suffix"),
        "action.yml should declare per-language crap4ts artifact upload"
    );
    assert!(
        action_yml
            .contains("if: inputs.html-report == 'true' && steps.lang.outputs.language == 'rust'"),
        "Upload Rust HTML report must be gated to single-language Rust"
    );
    assert!(
        action_yml.contains(
            "if: inputs.html-report == 'true' && steps.lang.outputs.language == 'typescript'"
        ),
        "Upload TypeScript HTML report must be gated to single-language TypeScript"
    );
}

#[then("the unified HTML render step does not execute")]
fn then_unified_render_skipped(world: &mut MultiLangWorld) {
    let action_yml = world
        .action_yml
        .as_ref()
        .expect("Given step should have loaded action.yml");
    assert!(
        action_yml.contains(
            "if: inputs.html-report == 'true' && steps.presets.outputs.is_multi == 'true'"
        ),
        "Render unified HTML step must be gated on is_multi == 'true'"
    );
}

// ── View axis Then steps ────────────────────────────────────────────

#[then("the HTML output contains a `<nav class=\"tabs\"` element inside the Combined panel")]
fn then_combined_has_tabs_nav(world: &mut MultiLangWorld) {
    let out = world.stdout();
    assert!(
        out.contains(r#"<nav class="tabs" role="tablist" aria-label="Combined views">"#),
        "Combined panel must carry View axis tabs when at least one language has a baseline"
    );
}

#[then(
    "both per-language panels contain a `<nav class=\"tabs\"` element with Current and Delta tabs"
)]
fn then_both_lang_panels_have_tabs(world: &mut MultiLangWorld) {
    let out = world.stdout();
    assert!(
        out.contains(r#"<nav class="tabs" role="tablist" aria-label="Rust views">"#),
        "Rust panel must carry View axis tabs"
    );
    assert!(
        out.contains(r#"<nav class="tabs" role="tablist" aria-label="TypeScript views">"#),
        "TypeScript panel must carry View axis tabs"
    );
}

#[then("no panel renders a disabled Delta tab when its language has a baseline")]
fn then_no_disabled_delta_tab(world: &mut MultiLangWorld) {
    let out = world.stdout();
    assert!(
        !out.contains(r#"title="no baseline available"#),
        "no panel should render the no-baseline disabled tooltip when all languages have baselines"
    );
}

#[then("the TypeScript panel renders the Delta tab with the disabled attribute")]
fn then_ts_delta_tab_disabled(world: &mut MultiLangWorld) {
    let out = world.stdout();
    let ts_nav_start = out
        .find(r#"aria-label="TypeScript views""#)
        .expect("TypeScript tabs nav present");
    let next_close = out[ts_nav_start..].find("</nav>").unwrap();
    let ts_nav = &out[ts_nav_start..ts_nav_start + next_close];
    assert!(
        ts_nav.contains("disabled"),
        "TypeScript Delta tab must be disabled when TypeScript has no baseline; got: {ts_nav}"
    );
}

#[then("the TypeScript Delta tab carries the no-baseline tooltip text")]
fn then_ts_delta_tab_no_baseline_tooltip(world: &mut MultiLangWorld) {
    let out = world.stdout();
    let ts_nav_start = out
        .find(r#"aria-label="TypeScript views""#)
        .expect("TypeScript tabs nav present");
    let next_close = out[ts_nav_start..].find("</nav>").unwrap();
    let ts_nav = &out[ts_nav_start..ts_nav_start + next_close];
    assert!(
        ts_nav.contains(r#"title="no baseline available for TypeScript""#),
        "Disabled Delta tab must carry the no-baseline tooltip; got: {ts_nav}"
    );
}

#[then("the Rust panel renders the Delta tab without the disabled attribute")]
fn then_rust_delta_tab_enabled(world: &mut MultiLangWorld) {
    let out = world.stdout();
    let rs_nav_start = out
        .find(r#"aria-label="Rust views""#)
        .expect("Rust tabs nav present");
    let next_close = out[rs_nav_start..].find("</nav>").unwrap();
    let rs_nav = &out[rs_nav_start..rs_nav_start + next_close];
    assert!(
        !rs_nav.contains("disabled"),
        "Rust Delta tab must be enabled when Rust has a baseline; got: {rs_nav}"
    );
}

#[then("the Combined Delta scope-banner names TypeScript as a language missing a baseline")]
fn then_combined_delta_missing_baseline_note(world: &mut MultiLangWorld) {
    let out = world.stdout();
    assert!(
        out.contains(r#"class="missing-baseline-note""#),
        "Combined Delta hero must render the missing-baseline note"
    );
    assert!(
        out.contains("<strong>TypeScript</strong>") && out.contains("has no baseline yet"),
        "missing-baseline-note must name TypeScript: {out}"
    );
}

#[then(
    "the Combined Delta tab panel lists the Rust High-risk regression before the TypeScript Moderate-risk regression"
)]
fn then_combined_delta_ranks_rust_before_typescript(world: &mut MultiLangWorld) {
    let out = world.stdout();
    let combined_delta_start = out
        .find(r#"data-tab="delta" role="tabpanel""#)
        .expect("Combined Delta tab-panel must render");
    let combined_delta_section = &out[combined_delta_start..];
    let rs_pos = combined_delta_section
        .find("view::analyze_view")
        .expect("Rust High-risk regression must surface in Combined Delta");
    let ts_pos = combined_delta_section
        .find("parseInvoice")
        .expect("TypeScript Moderate-risk regression must surface in Combined Delta");
    assert!(
        rs_pos < ts_pos,
        "Rust High-risk regression must rank ahead of TypeScript Moderate-risk regression in Combined Delta (D2d sort: risk band desc, ratio desc within band)"
    );
}

#[then("each Combined Delta row carries an adapter badge identifying its source language")]
fn then_combined_delta_rows_carry_badges(world: &mut MultiLangWorld) {
    let out = world.stdout();
    let combined_delta_start = out
        .find(r#"data-tab="delta" role="tabpanel""#)
        .expect("Combined Delta tab-panel must render");
    // Take everything between the opening of the Combined Delta tab
    // panel and the next `</div>` that closes a tab-panel (which is
    // our own).
    let combined_delta_section = &out[combined_delta_start..];
    assert!(
        combined_delta_section.contains(r#"<span class="adapter-badge""#),
        "Combined Delta rows must carry adapter-badge markup"
    );
}

#[then("the rendered JS parses URL hashes of the shape `#<lang>:<view>`")]
fn then_js_parses_two_axis_hash(world: &mut MultiLangWorld) {
    let out = world.stdout();
    // The JS does parts = rawHash.split(':') and reads parts[0] +
    // parts[1] for lang and view. Verify the structural marker so
    // we don't lock the exact JS text (which may evolve).
    assert!(
        out.contains("rawHash.split(':')"),
        "rendered JS must split the URL hash on ':' to extract <lang> and <view> axes"
    );
}

#[then("the rendered JS falls back to `#combined:current` when the URL carries no hash")]
fn then_js_default_hash_is_combined_current(world: &mut MultiLangWorld) {
    let out = world.stdout();
    // Default fallback: `var lang = parts[0] || 'combined';` and
    // `var view = parts[1] || 'current';`. Verify both literals so
    // the default-target invariant is locked.
    assert!(
        out.contains("parts[0] || 'combined'"),
        "rendered JS must default lang axis to 'combined' when URL hash is absent"
    );
    assert!(
        out.contains("parts[1] || 'current'"),
        "rendered JS must default view axis to 'current' when URL hash is absent"
    );
}

// ── No-baseline View axis Then steps ────────────────────────────────

#[then("the HTML output contains exactly three `<nav class=\"tabs\"` elements")]
fn then_three_view_navs(world: &mut MultiLangWorld) {
    let out = world.stdout();
    let count = out.matches(r#"<nav class="tabs""#).count();
    assert_eq!(
        count, 3,
        "expected exactly 3 View navs in unified HTML (Combined + Rust + TypeScript); got {count}"
    );
}

#[then("the Combined panel Delta tab is disabled with the cross-adapter no-baselines tooltip")]
fn then_combined_delta_disabled_no_baselines(world: &mut MultiLangWorld) {
    let out = world.stdout();
    let nav_start = out
        .find(r#"aria-label="Combined views""#)
        .expect("Combined tabs nav present");
    let close_off = out[nav_start..]
        .find("</nav>")
        .expect("Combined nav has closing tag");
    let nav = &out[nav_start..nav_start + close_off];
    assert!(
        nav.contains("disabled") && nav.contains(r#"aria-disabled="true""#),
        "Combined Delta tab must be disabled when no language supplied a baseline; got: {nav}"
    );
    // The Combined tooltip uses distinctive plural wording so it
    // cannot collide with the per-language `title="no baseline
    // available for <lang>"` literal asserted elsewhere in this
    // file. Drift between Combined and per-language wording is
    // caught by the assertion below in
    // `then_both_lang_delta_disabled_no_baseline_tooltip`.
    assert!(
        nav.contains(
            r#"title="no baselines provided — pass --baseline to enable cross-adapter delta""#
        ),
        "Combined Delta disabled tooltip must name the no-baselines cause; got: {nav}"
    );
}

#[then("both per-language Delta tabs are disabled with their per-language no-baseline tooltip")]
fn then_both_lang_delta_disabled_no_baseline_tooltip(world: &mut MultiLangWorld) {
    let out = world.stdout();
    for lang_label in ["Rust", "TypeScript"] {
        let aria = format!(r#"aria-label="{lang_label} views""#);
        let nav_start = out
            .find(&aria)
            .unwrap_or_else(|| panic!("{lang_label} tabs nav present"));
        let close_off = out[nav_start..]
            .find("</nav>")
            .expect("per-language nav has closing tag");
        let nav = &out[nav_start..nav_start + close_off];
        assert!(
            nav.contains("disabled") && nav.contains(r#"aria-disabled="true""#),
            "{lang_label} Delta tab must be disabled when {lang_label} has no baseline; got: {nav}"
        );
        let expected_tooltip = format!(r#"title="no baseline available for {lang_label}""#);
        assert!(
            nav.contains(&expected_tooltip),
            "{lang_label} Delta disabled tooltip must name the language ({expected_tooltip}); got: {nav}"
        );
    }
}

fn action_yml_path() -> PathBuf {
    // Walk up from CARGO_MANIFEST_DIR (= crates/crap4rs) to the
    // workspace root, then point at the composite action's yml.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    workspace.join(".github/actions/scorecard/action.yml")
}

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    MultiLangWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit(
            "tests/features/multi_lang_html.feature",
            |_, _, scenario| scenario.tags.iter().any(|t| t == "wired"),
        )
        .await;
}
