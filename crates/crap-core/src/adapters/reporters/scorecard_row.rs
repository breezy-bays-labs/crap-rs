//! Scorecard-row reporter — emits a single mokumo `Row::CrapDelta` JSON
//! object to stdout.
//!
//! The wire shape mirrors mokumo's locked schema fragment at
//! `breezy-bays-labs/mokumo/.config/scorecard/schema.json#/definitions/Row`
//! (CrapDelta member of the `oneOf`, schema_version=2). We replicate
//! the shape locally via private serde structs rather than depending
//! on mokumo's `scorecard` crate so the analyzer stays mokumo-
//! decoupled — the scorecard schema is the contract, not mokumo's
//! Rust crate. Model P (producer-mints-status) is the locked
//! convention; integration tests in adapter crates validate output
//! against a vendored copy of mokumo's schema.

use serde::Serialize;

use crate::domain::summary::{CrapDeltaRowData, CrapDeltaStatus};

/// Format a `CrapDeltaRowData` as a mokumo `Row::CrapDelta` JSON object.
///
/// Output is pretty-printed (multi-line) for CI-log readability — the
/// aggregator parses either form. A trailing newline is appended so the
/// CLI dispatcher's `print!` produces a clean line.
pub fn format_scorecard_row(data: &CrapDeltaRowData) -> String {
    let wire = ScorecardRowWire::from_data(data);
    let mut out = serde_json::to_string_pretty(&wire)
        .expect("ScorecardRowWire serializes infallibly — fields are owned strings + primitives");
    out.push('\n');
    out
}

/// On-the-wire shape. Serializes to mokumo's `Row::CrapDelta`.
///
/// Field order is illustrative only — JSON Schema validation is order-
/// agnostic, and downstream consumers parse by key, not position.
#[derive(Debug, Serialize)]
struct ScorecardRowWire<'a> {
    #[serde(rename = "type")]
    ty: &'static str,
    id: &'static str,
    label: &'static str,
    anchor: &'static str,
    status: WireStatus,
    threshold: u32,
    delta_count: i32,
    delta_text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_detail_md: Option<&'a str>,
}

impl<'a> ScorecardRowWire<'a> {
    fn from_data(data: &'a CrapDeltaRowData) -> Self {
        Self {
            ty: "CrapDelta",
            id: "crap_delta",
            label: "CRAP Δ",
            anchor: "crap-delta",
            status: WireStatus::from(data.status),
            threshold: data.threshold,
            delta_count: data.delta_count,
            delta_text: &data.delta_text,
            failure_detail_md: data.failure_detail_md.as_deref(),
        }
    }
}

/// PascalCase serialization mirrors mokumo's `Status` enum
/// (`#[serde(rename_all = "PascalCase")]` in `crates/scorecard/src/lib.rs`).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "PascalCase")]
enum WireStatus {
    Green,
    Yellow,
    Red,
}

impl From<CrapDeltaStatus> for WireStatus {
    fn from(s: CrapDeltaStatus) -> Self {
        match s {
            CrapDeltaStatus::Green => WireStatus::Green,
            CrapDeltaStatus::Yellow => WireStatus::Yellow,
            CrapDeltaStatus::Red => WireStatus::Red,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn parse(out: &str) -> Value {
        serde_json::from_str(out).expect("output must be valid JSON")
    }

    fn green_data() -> CrapDeltaRowData {
        CrapDeltaRowData {
            status: CrapDeltaStatus::Green,
            threshold: 15,
            delta_count: 0,
            delta_text: "5 → 5".to_string(),
            failure_detail_md: None,
        }
    }

    fn yellow_data() -> CrapDeltaRowData {
        CrapDeltaRowData {
            status: CrapDeltaStatus::Yellow,
            threshold: 15,
            delta_count: 0,
            delta_text: "5 → 5 (regressions on existing functions)".to_string(),
            failure_detail_md: None,
        }
    }

    fn red_data() -> CrapDeltaRowData {
        CrapDeltaRowData {
            status: CrapDeltaStatus::Red,
            threshold: 15,
            delta_count: 2,
            delta_text: "5 → 7 (+2)".to_string(),
            failure_detail_md: Some(
                "**New CRAP threshold violations (>15):**\n- `foo` — `a.rs:1` — CRAP 22.0 (newly added)\n"
                    .to_string(),
            ),
        }
    }

    #[test]
    fn static_fields_are_locked() {
        let v = parse(&format_scorecard_row(&green_data()));
        assert_eq!(v["type"], "CrapDelta");
        assert_eq!(v["id"], "crap_delta");
        assert_eq!(v["label"], "CRAP Δ");
        assert_eq!(v["anchor"], "crap-delta");
    }

    #[test]
    fn green_omits_failure_detail() {
        let v = parse(&format_scorecard_row(&green_data()));
        assert!(v.get("failure_detail_md").is_none());
        assert_eq!(v["status"], "Green");
        assert_eq!(v["delta_count"], 0);
        assert_eq!(v["threshold"], 15);
    }

    #[test]
    fn yellow_omits_failure_detail() {
        let v = parse(&format_scorecard_row(&yellow_data()));
        assert!(v.get("failure_detail_md").is_none());
        assert_eq!(v["status"], "Yellow");
    }

    #[test]
    fn red_includes_failure_detail() {
        let v = parse(&format_scorecard_row(&red_data()));
        assert_eq!(v["status"], "Red");
        assert_eq!(v["delta_count"], 2);
        assert_eq!(v["delta_text"], "5 → 7 (+2)");
        let detail = v["failure_detail_md"].as_str().expect("failure_detail set");
        assert!(detail.contains("CRAP 22.0"));
    }

    #[test]
    fn output_ends_with_newline() {
        let out = format_scorecard_row(&green_data());
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn round_trip_is_byte_stable() {
        let out1 = format_scorecard_row(&red_data());
        let out2 = format_scorecard_row(&red_data());
        assert_eq!(out1, out2, "format is deterministic");
    }

    // ── Byte-level snapshot locks ───────────────────────────────
    //
    // Complement the existing `scorecard_row_integration.rs` schema
    // validation (which gates the shape against mokumo's
    // `Row::CrapDelta` JSON Schema) with insta snapshots that pin the
    // exact serialization — pretty-printer settings, field ordering,
    // failure-detail rendering. Schema + insta together = belt +
    // suspenders.
    //
    // One snapshot per `CrapDeltaStatus` because each carries a
    // structurally distinct payload (green/yellow omit
    // `failure_detail_md`; red includes it). Without/with-delta is
    // covered by the green vs red split (delta_count 0 vs 2).

    #[test]
    fn green_scorecard_row_snapshot() {
        let out = format_scorecard_row(&green_data());
        insta::assert_snapshot!(out);
    }

    #[test]
    fn yellow_scorecard_row_snapshot() {
        let out = format_scorecard_row(&yellow_data());
        insta::assert_snapshot!(out);
    }

    #[test]
    fn red_scorecard_row_snapshot() {
        let out = format_scorecard_row(&red_data());
        insta::assert_snapshot!(out);
    }
}
