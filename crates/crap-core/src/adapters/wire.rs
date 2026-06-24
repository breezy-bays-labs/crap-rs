//! Canonical JSON wire envelope — the single owned schema shared by the
//! writer (the JSON reporter) and every reader (the baseline loader and
//! the `crap-render` binary).
//!
//! ## One type, both directions
//!
//! [`Envelope`] derives both `Serialize` and `Deserialize`, so the wire
//! contract has exactly one source of truth: a field added to the writer
//! is readable by construction, and a field the readers expect must
//! exist on the writer. The previous split — a borrow-based
//! `Serialize`-only writer struct mirrored by hand-maintained partial
//! reader structs — let the two sides drift silently (a new writer field
//! would `serde(default)` to nothing on the read side instead of failing
//! to compile). The cost of unification is that the writer clones the
//! analysis into the owned envelope at emit time, trading the previous
//! zero-copy serialization for the single schema; for a fire-and-exit
//! CLI about to pretty-print the same data, the clone is negligible.
//!
//! ## Data vs presentation
//!
//! The envelope carries two kinds of fields with different round-trip
//! contracts:
//!
//! - **Data** — `schema_version` through `result`, plus `diagnostics`,
//!   `missing_coverage_policy`, and `epsilon`. This is the analysis and
//!   the parameters that shaped it. Data fields round-trip: what the
//!   writer emits, a reader gets back, guarded by this module's tests.
//! - **Presentation** — [`ViewBlock`] (`view`) and [`DeltaBlock`]
//!   (`delta`). These describe how rows were filtered / sorted /
//!   truncated for display, and what changed vs a baseline. They are
//!   derived from the data and recomputable from it, so they are
//!   **write-only**: serialized for external consumers, but
//!   `skip_deserializing` on the read side (a parsed envelope carries
//!   defaults). In particular, a consumer that needs a delta recomputes
//!   it from two `result` blocks (as `crap-render` does) rather than
//!   trusting a pre-computed block whose inputs it cannot see.
//!
//! ## Read contract
//!
//! Only `schema_version` and `result` are required on read; every other
//! field defaults when absent. This is the baseline loader's documented
//! permissive contract (consumers may hand-produce baseline envelopes
//! carrying only what they have), now uniform across all readers.
//!
//! ## Adapter-agnostic reading
//!
//! `Envelope` is generic over `P: ParseDiagnostic` because
//! `diagnostics` carries adapter-specific parse-diagnostic shapes. A
//! reader that must accept envelopes from *any* adapter (such as
//! `crap-render`, which composes Rust and TypeScript envelopes into one
//! report) instantiates [`Envelope<RawParseDiagnostic>`]: the raw
//! diagnostic preserves the per-entry diagnostic JSON faithfully
//! without committing to a concrete adapter's type. The erasure covers
//! the `parse_diagnostics` *entries* only — the surrounding
//! [`AnalysisDiagnostics`] count fields are the shared concrete shape
//! every adapter's writer emits.

use crate::domain::delta::{DeltaSummary, DeltaViewSpec, FunctionChange};
use crate::domain::types::{
    AnalysisDiagnostics, AnalysisResult, AnalysisSummary, ComplexityMetric, FunctionVerdict,
    MissingCoveragePolicy,
};
use crate::domain::view::{GroupedView, ViewSpec};
use crate::ports::ParseDiagnostic;
use serde::{Deserialize, Serialize};

/// Envelope schema version stamped on every emitted envelope.
///
/// The writer stamps this constant and the readers validate against
/// [`SUPPORTED_SCHEMA_VERSIONS`], so the emit version and the accepted
/// range can never drift apart silently. The schema evolves additively
/// across minor releases; the bump from 1 → 2 in 0.4.0 reflects the
/// `ComplexityContributor.column` 0-based → 1-based convention shift.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

/// Envelope schema versions accepted by every reader.
///
/// v1 stays loadable across the v0.3.x → v0.4.x boundary so users can
/// keep their committed baseline JSON; the column-convention shift in
/// v2 doesn't affect delta calculations (identity-keyed matching).
/// Future schema bumps will need an explicit migration path.
pub const SUPPORTED_SCHEMA_VERSIONS: &[u32] = &[1, 2];

/// The canonical JSON envelope — the single wire schema, used owned by
/// the writer and deserialized whole by every reader.
///
/// Field declaration order is **load-bearing**: it is the wire key
/// order, pinned end-to-end by the CLI ergonomics feature
/// (`schema_version` through `view`) and at the type level by this
/// module's key-order test. New fields are appended additively after
/// `epsilon` and elided at their default so existing envelopes stay
/// byte-identical; they do not bump [`CURRENT_SCHEMA_VERSION`].
///
/// See the module docs for the data-vs-presentation split and the
/// permissive read contract (only `schema_version` and `result` are
/// required; everything else defaults).
///
/// `serde(bound = "")` suppresses the auto-generated `P: Serialize` /
/// `P: Deserialize<'de>` bounds — `P: ParseDiagnostic` already provides
/// `Serialize + DeserializeOwned`, and the auto-bounds conflict with
/// the owned-deserialize requirement.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Envelope<P: ParseDiagnostic> {
    pub schema_version: u32,
    #[serde(default)]
    pub tool_version: String,
    /// Lowercase language tag of the emitting adapter (`"rust"` /
    /// `"typescript"`), sourced from the adapter's
    /// `AdapterMeta::config_lang_key`, so each adapter stamps its own
    /// language. `crap-render` still pairs each envelope with an explicit
    /// `--input <lang>=<file>` key for routing rather than trusting this
    /// field, but the field is now a faithful per-adapter identity signal.
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub timestamp: String,
    /// Complexity metric the analysis was scored with. The writer
    /// always emits it (`Some` serializes as the bare value, so the
    /// wire bytes are unchanged); on read, `None` means the envelope
    /// omitted the field. Kept `Option` rather than defaulting so
    /// consumers can distinguish "predates / omits the field" from
    /// "scored with the default metric" — the metric-mismatch guard
    /// must not treat an omitting baseline as disagreeing.
    #[serde(default)]
    pub metric: Option<ComplexityMetric>,
    /// Workspace-level CRAP threshold the gate ran with. Same
    /// `Option` semantics as `metric`: always emitted by the writer,
    /// `None` on read means the envelope omitted it.
    #[serde(default)]
    pub threshold: Option<f64>,
    /// Git ref used for diff filtering. Always emitted (`null` when no
    /// diff filter was active) so consumers can distinguish "no diff
    /// filter" from "schema doesn't carry the key".
    #[serde(default)]
    pub diff_ref: Option<String>,
    /// The canonical analysis — the gate source of truth, and the block
    /// a consumer recomputes deltas from.
    pub result: AnalysisResult,
    /// Presentation: how the reported rows were filtered, sorted, and
    /// truncated. Write-only — see the module docs.
    #[serde(skip_deserializing, default)]
    pub view: ViewBlock,
    /// Presentation: changes vs a baseline. Present iff the emitting
    /// run compared against one; the key is elided otherwise so the
    /// no-delta envelope stays byte-identical to pre-delta output.
    /// Write-only — a consumer that needs a delta recomputes it from
    /// two `result` blocks.
    #[serde(skip_deserializing, default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<DeltaBlock<P>>,
    /// Parse diagnostics from the coverage ingestion, included when the
    /// emitting run was verbose. Adapter-specific shape; see
    /// [`RawParseDiagnostic`] for adapter-agnostic reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<AnalysisDiagnostics<P>>,
    /// Missing-coverage policy the run was configured with. Elided when
    /// `Pessimistic` (the default) so every pessimistic-run envelope
    /// stays byte-identical to envelopes emitted before the field
    /// existed; readers treat an absent key as `Pessimistic`.
    #[serde(default, skip_serializing_if = "is_pessimistic")]
    pub missing_coverage_policy: MissingCoveragePolicy,
    /// Effective threshold-border epsilon the run was configured with.
    /// Carried on the envelope — even on a baseline-less run that
    /// computes no delta — so a consumer recomputing the delta from
    /// `result` blocks applies the same border band the gate used.
    /// Elided when `0.0` (suppression off, the default).
    #[serde(default, skip_serializing_if = "is_zero_epsilon")]
    pub epsilon: f64,
}

/// `skip_serializing_if` predicate: elide `missing_coverage_policy`
/// when it carries the default, so a pessimistic run's envelope is
/// byte-identical to before the field existed.
fn is_pessimistic(policy: &MissingCoveragePolicy) -> bool {
    *policy == MissingCoveragePolicy::Pessimistic
}

/// `skip_serializing_if` predicate: elide `epsilon` when it is `0.0`
/// (suppression off — the default), so every existing envelope stays
/// byte-identical. `0.0 == -0.0` in IEEE-754, and the resolved epsilon
/// is always a finite value `>= 0.0` (validated at the CLI / config
/// boundary), so `== 0.0` is the exact "off" test.
fn is_zero_epsilon(epsilon: &f64) -> bool {
    *epsilon == 0.0
}

/// On-the-wire view block: how the reported rows were shaped for
/// display. Owned mirror of the row-view metadata; write-only
/// presentation (see the module docs), so it derives `Serialize` and
/// `Default` but deliberately not `Deserialize`.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize)]
pub struct ViewBlock {
    /// Echoes the resolved view spec so consumers can reconstruct what
    /// filters / sort / limit produced `shown`.
    pub spec: ViewSpec,
    /// Post-filter, pre-truncate count. With `truncated`, lets
    /// consumers render "Showing X of Y".
    pub eligible_count: usize,
    pub truncated: bool,
    /// Per-row list, post-filter / sort / truncate. `None` when the run
    /// asked for a minimal view, which elides the key; every other view
    /// key remains for scope context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown: Option<Vec<FunctionVerdict>>,
    pub shown_summary: AnalysisSummary,
    /// Per-key aggregation block. Always serialized (emits `null` when
    /// grouping is inactive) so consumers can distinguish "default
    /// invocation" from "schema doesn't carry grouping".
    pub grouped: Option<GroupedView>,
}

/// On-the-wire delta block: changes vs a baseline, shaped for display.
/// Owned mirror of the delta-view metadata; write-only presentation
/// (see the module docs).
#[non_exhaustive]
#[derive(Debug, Clone, Serialize)]
#[serde(bound = "")]
pub struct DeltaBlock<P: ParseDiagnostic> {
    /// Aggregate counts over the *unshaped* change set. The gate
    /// keystone — shaping never alters this.
    pub summary: DeltaSummary,
    /// Echoes the resolved delta-view spec so consumers can reconstruct
    /// what filters / sort / limit produced `shown`.
    pub spec: DeltaViewSpec,
    /// Post-filter, pre-truncate count. With `truncated`, lets
    /// consumers render "Showing X of Y".
    pub eligible_count: usize,
    pub truncated: bool,
    /// Reserved for a future baseline-label flag. Always `null` today.
    pub baseline_ref: Option<String>,
    pub baseline_tool_version: String,
    pub baseline_timestamp: String,
    /// Per-change list, post-filter / sort / truncate.
    pub shown: Vec<FunctionChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_diagnostics: Option<AnalysisDiagnostics<P>>,
}

/// Adapter-agnostic parse diagnostic: preserves a diagnostic entry's
/// JSON faithfully without committing to a concrete adapter's
/// diagnostic type.
///
/// A reader that must accept envelopes from *any* adapter instantiates
/// [`Envelope<RawParseDiagnostic>`]; the transparent `serde_json::Value`
/// representation round-trips diagnostic entries it does not understand
/// instead of rejecting or distorting them. The erasure is per-entry:
/// the [`AnalysisDiagnostics`] block around the entries keeps its
/// shared concrete shape (the count fields every adapter's writer
/// emits).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RawParseDiagnostic(pub serde_json::Value);

impl ParseDiagnostic for RawParseDiagnostic {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::make_single_function_result;
    use crate::domain::types::RiskLevel;
    use crate::test_strategies::DummyParseDiagnostic;

    fn sample_diagnostics() -> AnalysisDiagnostics<DummyParseDiagnostic> {
        AnalysisDiagnostics {
            parse_diagnostics: vec![],
            files_found: 10,
            files_unparseable: 1,
            functions_extracted: 42,
            functions_matched: 40,
            functions_no_coverage: 2,
            files_analyzed: 8,
            files_zero_coverage: 2,
        }
    }

    /// A fully-populated envelope: every data field non-default, both
    /// presentation blocks present, so a single fixture exercises every
    /// serialization branch.
    fn populated_envelope() -> Envelope<DummyParseDiagnostic> {
        Envelope {
            schema_version: CURRENT_SCHEMA_VERSION,
            tool_version: "9.9.9".to_string(),
            language: "rust".to_string(),
            timestamp: "2026-06-09T12:00:00Z".to_string(),
            metric: Some(ComplexityMetric::Cyclomatic),
            threshold: Some(11.5),
            diff_ref: Some("main".to_string()),
            result: make_single_function_result(
                "compute_crap",
                "src/domain/crap.rs",
                5,
                80.0,
                5.16,
                RiskLevel::Acceptable,
                11.5,
            ),
            view: ViewBlock {
                spec: ViewSpec::default(),
                eligible_count: 1,
                truncated: false,
                shown: None,
                shown_summary: AnalysisSummary::default(),
                grouped: None,
            },
            delta: Some(DeltaBlock {
                summary: DeltaSummary::default(),
                spec: DeltaViewSpec::default(),
                eligible_count: 0,
                truncated: false,
                baseline_ref: None,
                baseline_tool_version: "0.0.9".to_string(),
                baseline_timestamp: "2026-01-01T00:00:00Z".to_string(),
                shown: vec![],
                baseline_diagnostics: None,
            }),
            diagnostics: Some(sample_diagnostics()),
            missing_coverage_policy: MissingCoveragePolicy::Optimistic,
            epsilon: 0.25,
        }
    }

    fn to_value<T: Serialize>(value: &T) -> serde_json::Value {
        serde_json::to_value(value).expect("serializable")
    }

    #[test]
    fn data_fields_round_trip_through_the_wire() {
        let original = populated_envelope();
        let json = serde_json::to_string_pretty(&original).expect("serialize");
        let parsed: Envelope<DummyParseDiagnostic> =
            serde_json::from_str(&json).expect("the writer's output must parse as the same type");

        assert_eq!(parsed.schema_version, original.schema_version);
        assert_eq!(parsed.tool_version, original.tool_version);
        assert_eq!(parsed.language, original.language);
        assert_eq!(parsed.timestamp, original.timestamp);
        assert_eq!(parsed.metric, original.metric);
        assert_eq!(parsed.threshold, original.threshold);
        assert_eq!(parsed.diff_ref, original.diff_ref);
        assert_eq!(to_value(&parsed.result), to_value(&original.result));
        assert_eq!(
            to_value(&parsed.diagnostics),
            to_value(&original.diagnostics)
        );
        assert_eq!(
            parsed.missing_coverage_policy,
            original.missing_coverage_policy
        );
        assert_eq!(parsed.epsilon, original.epsilon);
    }

    #[test]
    fn presentation_blocks_are_emitted_but_not_read_back() {
        let original = populated_envelope();
        let json = serde_json::to_string_pretty(&original).expect("serialize");

        // Emitted: external consumers see both blocks on the wire.
        assert!(json.contains("\n  \"view\""), "view block must be emitted");
        assert!(
            json.contains("\n  \"delta\""),
            "delta block must be emitted"
        );

        // Not read back: a parsed envelope carries defaults — the blocks
        // are derived presentation, recomputable from `result`.
        let parsed: Envelope<DummyParseDiagnostic> = serde_json::from_str(&json).expect("parse");
        assert_eq!(
            parsed.view.eligible_count, 0,
            "view is write-only; reads as default"
        );
        assert!(parsed.delta.is_none(), "delta is write-only; reads as None");
    }

    #[test]
    fn wire_key_order_matches_the_pinned_contract() {
        // The leading key order (schema_version .. view) is asserted
        // end-to-end by the CLI ergonomics feature; this pins the FULL
        // order including the conditional tail keys, at the type level,
        // so a field reorder fails here before it reaches a snapshot.
        let json = serde_json::to_string_pretty(&populated_envelope()).expect("serialize");
        let keys = [
            "schema_version",
            "tool_version",
            "language",
            "timestamp",
            "metric",
            "threshold",
            "diff_ref",
            "result",
            "view",
            "delta",
            "diagnostics",
            "missing_coverage_policy",
            "epsilon",
        ];
        // Top-level keys sit at indent 2 in serde_json's pretty printer;
        // anchoring on `\n  "<key>"` keeps nested same-name keys from
        // shadowing the top-level position.
        let positions: Vec<usize> = keys
            .iter()
            .map(|k| {
                json.find(&format!("\n  \"{k}\""))
                    .unwrap_or_else(|| panic!("missing top-level key {k} in:\n{json}"))
            })
            .collect();
        for (pair, w) in keys.windows(2).zip(positions.windows(2)) {
            assert!(
                w[0] < w[1],
                "wire key order: expected {} before {}, got positions {} and {}",
                pair[0],
                pair[1],
                w[0],
                w[1],
            );
        }
    }

    #[test]
    fn elision_rules_keep_default_envelopes_lean() {
        // Defaults elide the optional tail keys so an envelope emitted
        // with no delta context, default policy, and epsilon off stays
        // byte-identical to envelopes produced before those fields
        // existed. `diff_ref` and `view` are NOT elided: consumers
        // distinguish "no diff filter" (null) from "schema doesn't carry
        // the key".
        let envelope: Envelope<DummyParseDiagnostic> = Envelope {
            schema_version: CURRENT_SCHEMA_VERSION,
            tool_version: "0.1.0".to_string(),
            language: "rust".to_string(),
            timestamp: "2026-06-09T12:00:00Z".to_string(),
            metric: Some(ComplexityMetric::Cognitive),
            threshold: Some(8.0),
            diff_ref: None,
            result: make_single_function_result(
                "f",
                "src/lib.rs",
                1,
                100.0,
                1.0,
                RiskLevel::Low,
                8.0,
            ),
            view: ViewBlock::default(),
            delta: None,
            diagnostics: None,
            missing_coverage_policy: MissingCoveragePolicy::Pessimistic,
            epsilon: 0.0,
        };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string_pretty(&envelope).expect("serialize"))
                .expect("valid JSON");

        assert!(v.get("delta").is_none(), "absent delta elides the key");
        assert!(
            v.get("diagnostics").is_none(),
            "absent diagnostics elides the key"
        );
        assert!(
            v.get("missing_coverage_policy").is_none(),
            "default policy elides the key"
        );
        assert!(v.get("epsilon").is_none(), "epsilon 0.0 elides the key");
        assert!(
            v.get("diff_ref").is_some_and(serde_json::Value::is_null),
            "diff_ref stays present as null"
        );
        assert!(v.get("view").is_some(), "view block is always emitted");
    }

    #[test]
    fn read_contract_requires_only_schema_version_and_result() {
        // The permissive read contract: consumers may hand-produce
        // envelopes carrying only the analysis itself; all metadata
        // defaults. This is the baseline loader's documented behavior,
        // uniform across every reader of the canonical type.
        let json = r#"{
            "schema_version": 2,
            "result": {
                "functions": [],
                "summary": {
                    "total_functions": 0, "total_files": 0, "exceeding_threshold": 0,
                    "average_crap": 0.0, "median_crap": 0.0,
                    "max_crap": null, "worst_function": null,
                    "distribution": { "low": 0, "acceptable": 0, "moderate": 0, "high": 0 }
                },
                "passed": true
            }
        }"#;
        let parsed: Envelope<DummyParseDiagnostic> =
            serde_json::from_str(json).expect("schema_version + result suffice");
        assert_eq!(parsed.schema_version, 2);
        assert_eq!(parsed.tool_version, "");
        assert_eq!(parsed.language, "");
        assert_eq!(parsed.timestamp, "");
        // Absent metric/threshold read as None — NOT silently as the
        // Rust-side defaults — so consumers can distinguish "the
        // envelope predates / omits the field" from "the field was
        // emitted with the default value". The metric-mismatch guard
        // depends on this: a baseline that merely omits `metric` must
        // not be treated as disagreeing with the current run.
        assert_eq!(parsed.metric, None);
        assert_eq!(parsed.threshold, None);
        assert_eq!(parsed.diff_ref, None);
        assert!(parsed.result.passed);
        assert!(parsed.diagnostics.is_none());
        assert_eq!(
            parsed.missing_coverage_policy,
            MissingCoveragePolicy::Pessimistic
        );
        assert_eq!(parsed.epsilon, 0.0);
    }

    #[test]
    fn raw_parse_diagnostic_carries_any_adapter_shape_faithfully() {
        // The erased diagnostic must accept arbitrary adapter-specific
        // entries AND re-serialize them unchanged, so an
        // adapter-agnostic reader neither rejects nor distorts
        // diagnostics it does not understand.
        let json = r#"{
            "parse_diagnostics": [
                {"kind":"MalformedRecord","line":42,"detail":"bad DA"},
                {"kind":"UnknownPrefix","line":7}
            ],
            "files_found": 5, "files_unparseable": 1,
            "functions_extracted": 10, "functions_matched": 8,
            "functions_no_coverage": 2, "files_analyzed": 5, "files_zero_coverage": 0
        }"#;
        let diag: AnalysisDiagnostics<RawParseDiagnostic> =
            serde_json::from_str(json).expect("erased diagnostics must deserialize");
        assert_eq!(diag.parse_diagnostics.len(), 2);
        let original: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        assert_eq!(
            to_value(&diag),
            original,
            "re-serialization must be value-identical to the input"
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::test_strategies::{DummyParseDiagnostic, arb_analysis_result};
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// The data contract round-trips for ANY analysis: with default
        /// (write-only) presentation blocks, serialize → parse →
        /// re-serialize is value-identical, so a writer/reader
        /// divergence on any data field fails here.
        #[test]
        fn prop_envelope_data_round_trips(
            result in arb_analysis_result(),
            threshold in 1.0..100.0f64,
            epsilon in prop_oneof![Just(0.0f64), 0.001..2.0f64],
        ) {
            let envelope: Envelope<DummyParseDiagnostic> = Envelope {
                schema_version: CURRENT_SCHEMA_VERSION,
                tool_version: "0.1.0".to_string(),
                language: "rust".to_string(),
                timestamp: "2026-01-01T00:00:00Z".to_string(),
                metric: Some(ComplexityMetric::Cognitive),
                threshold: Some(threshold),
                diff_ref: None,
                result,
                view: ViewBlock::default(),
                delta: None,
                diagnostics: None,
                missing_coverage_policy: MissingCoveragePolicy::Pessimistic,
                epsilon,
            };
            let json = serde_json::to_string_pretty(&envelope).expect("serialize");
            let parsed: Envelope<DummyParseDiagnostic> =
                serde_json::from_str(&json).expect("parse");
            prop_assert_eq!(
                serde_json::to_value(&envelope).expect("to_value"),
                serde_json::to_value(&parsed).expect("to_value")
            );
        }
    }
}
