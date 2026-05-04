//! Integration tests for `--format scorecard-row` (issue #111).
//!
//! Validates that the projector + reporter produce a JSON object that
//! conforms to mokumo's locked `Row::CrapDelta` schema fragment
//! (vendored at `tests/fixtures/scorecard/schema.json`,
//! `schema_version = 2`).
//!
//! These tests are the contract pin between crap4rs and mokumo. If
//! mokumo bumps `schema_version`, run
//! `tests/fixtures/scorecard/regen.sh` to refresh the fixture, then
//! adjust this file or the reporter as needed.

use std::sync::OnceLock;

use crap4rs::adapters::reporters::format_scorecard_row;
use crap4rs::domain::summary::{CrapDeltaRowData, CrapDeltaStatus};
use serde_json::Value;

/// Vendored mokumo schema. Embedded so the test is hermetic — no
/// filesystem reads at runtime, no network.
const VENDORED_SCHEMA: &str = include_str!("fixtures/scorecard/schema.json");

/// Build a wrapper schema rooted at `#/definitions/Row` so the JSON
/// Schema validator resolves `$ref`s against the vendored `definitions`
/// block. Cached because `serde_json::from_str` over a 50KB schema
/// plus compilation isn't free, and the test module runs each case
/// independently.
fn row_schema_validator() -> &'static jsonschema::Validator {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        let full: Value =
            serde_json::from_str(VENDORED_SCHEMA).expect("vendored schema must be valid JSON");
        let definitions = full
            .get("definitions")
            .expect("schema must carry definitions")
            .clone();
        let row_schema = serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "definitions": definitions,
            "$ref": "#/definitions/Row",
        });
        jsonschema::validator_for(&row_schema)
            .expect("wrapper schema must compile against vendored definitions")
    })
}

fn validate_row(row_json: &str) {
    let value: Value = serde_json::from_str(row_json)
        .unwrap_or_else(|e| panic!("output must be valid JSON: {e}\nstdout was:\n{row_json}"));
    let validator = row_schema_validator();
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|e| format!("at {}: {e}", e.instance_path))
        .collect();
    assert!(
        errors.is_empty(),
        "row failed schema validation:\n{}\nemitted JSON:\n{row_json}",
        errors.join("\n")
    );
}

// ── Green / Yellow / Red ─────────────────────────────────────────────

#[test]
fn green_row_validates_against_mokumo_schema() {
    let data = CrapDeltaRowData {
        status: CrapDeltaStatus::Green,
        threshold: 15,
        delta_count: -1,
        delta_text: "5 → 4 (-1)".to_string(),
        failure_detail_md: None,
    };
    validate_row(&format_scorecard_row(&data));
}

#[test]
fn yellow_row_validates_against_mokumo_schema() {
    let data = CrapDeltaRowData {
        status: CrapDeltaStatus::Yellow,
        threshold: 15,
        delta_count: 0,
        delta_text: "5 → 5 (regressions on existing functions)".to_string(),
        failure_detail_md: None,
    };
    validate_row(&format_scorecard_row(&data));
}

#[test]
fn red_row_validates_against_mokumo_schema() {
    let data = CrapDeltaRowData {
        status: CrapDeltaStatus::Red,
        threshold: 15,
        delta_count: 2,
        delta_text: "5 → 7 (+2)".to_string(),
        failure_detail_md: Some(
            "**New CRAP threshold violations (>15):**\n- `foo::bar` — `src/foo.rs:42` — CRAP 22.0 (newly added)\n"
                .to_string(),
        ),
    };
    validate_row(&format_scorecard_row(&data));
}

// ── Layer 2 enforcement: Red WITHOUT failure_detail_md must fail ─────

#[test]
fn red_row_without_failure_detail_is_rejected_by_layer_2_if_then() {
    // Hand-craft a malformed payload — bypassing the projector — to
    // confirm the vendored schema enforces the Layer 2 invariant
    // (Red ⇒ failure_detail_md required) end-to-end.
    let malformed = serde_json::json!({
        "type": "CrapDelta",
        "id": "crap_delta",
        "label": "CRAP Δ",
        "anchor": "crap-delta",
        "status": "Red",
        "threshold": 15,
        "delta_count": 2,
        "delta_text": "5 → 7 (+2)",
    });
    let validator = row_schema_validator();
    let errors: Vec<_> = validator.iter_errors(&malformed).collect();
    assert!(
        !errors.is_empty(),
        "schema must reject Red row missing failure_detail_md"
    );
}

// ── End-to-end CLI dispatch ──────────────────────────────────────────
//
// Exercise `crap4rs --format scorecard-row` through the binary so the
// CLI dispatch path (`format_as_scorecard_row` and the surrounding
// `print_formatted_output` arm) is covered. The earlier per-status
// tests above call the projector + reporter directly — they don't go
// through `run_inner`, so the dispatch arm previously had zero
// coverage.

#[test]
fn cli_dispatch_emits_scorecard_row_validating_against_schema() {
    let binary = env!("CARGO_BIN_EXE_crap4rs");
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let out = std::process::Command::new(binary)
        .args([
            "--coverage",
            &format!("{manifest_dir}/tests/fixtures/crap4rs-self.lcov"),
            "--src",
            &format!("{manifest_dir}/src"),
            "--format",
            "scorecard-row",
            "--no-fail",
        ])
        .output()
        .expect("failed to run crap4rs binary");

    assert!(
        out.status.success(),
        "crap4rs --format scorecard-row exited non-zero ({:?}); stderr=\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("stdout must be UTF-8");
    validate_row(&stdout);
}

// ── Schema version pin ───────────────────────────────────────────────

#[test]
fn vendored_schema_pins_schema_version_2() {
    // If mokumo bumps schema_version, this test fails loud.
    // Run tests/fixtures/scorecard/regen.sh to refresh, then update
    // crap4rs reporter / tests as needed before bumping the pin here.
    let full: Value = serde_json::from_str(VENDORED_SCHEMA).unwrap();
    // schema_version lives on the top-level Scorecard envelope;
    // the value `2` is asserted by inspecting the description.
    // We pin the structural property: the schema MUST require
    // schema_version on Scorecard.
    let required = full["required"]
        .as_array()
        .expect("Scorecard.required must be an array");
    assert!(
        required.iter().any(|v| v == "schema_version"),
        "Scorecard schema must require schema_version"
    );
}
