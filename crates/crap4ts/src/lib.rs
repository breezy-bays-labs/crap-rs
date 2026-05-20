//! crap4ts — TypeScript adapter for the language-agnostic `crap_core`
//! analyzer.
//!
//! Combines AST complexity (via oxc) with Istanbul JSON coverage to
//! identify functions that are both complex and under-tested. Default
//! metric is cyclomatic complexity; cognitive surfaces
//! `CrapError::MetricNotSupported` (D5 + locked decision #2).
//!
//! Three consumer surfaces:
//! - the `crap4ts` CLI binary (`src/main.rs`) — default build, no napi
//!   linkage,
//! - the Rust library API [`analyze_to_json`] — feature-independent,
//!   exercised by Rust integration tests + the LCOV coverage gate,
//! - the napi-rs cdylib (`src/napi.rs`) — gated behind the
//!   `napi-binding` feature; a thin shim that unpacks `AnalyzeOptions`
//!   and delegates to [`analyze_to_json`] so the orchestration logic
//!   stays in feature-independent code and self-CRAP can score it.

use std::path::Path;

use serde::Serialize;

use crap_core::core::{AnalyzeOptions, analyze};
use crap_core::domain::threshold::{ThresholdConfig, ThresholdPreset};
use crap_core::domain::types::{AnalysisDiagnostics, AnalysisResult, ComplexityMetric};
use crap_core::ports::ParseDiagnostic;

use crate::adapters::coverage::IstanbulCoverage;
use crate::adapters::walker::OxcWalker;
use crate::parse_diagnostic::IstanbulParseDiagnostic;

pub mod adapters;
pub mod parse_diagnostic;

#[cfg(feature = "napi-binding")]
pub mod napi;

/// File extensions the walker discovers. Mirrors
/// `crates/crap4ts/src/main.rs`'s `EXTENSIONS` constant so library
/// callers see the same source set the CLI does.
const EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs"];

/// Wire shape for the JSON returned by [`analyze_to_json`]. Mirrors
/// `crap_core::core::AnalysisOutput<P>`'s `{ result, diagnostics }`
/// shape verbatim. `#[serde(bound = "")]` suppresses serde's
/// auto-generated bounds — `P: ParseDiagnostic` already requires
/// `Serialize + DeserializeOwned`.
#[derive(Serialize)]
#[serde(bound = "")]
struct AnalyzeWireOutput<'a, P: ParseDiagnostic> {
    result: &'a AnalysisResult,
    diagnostics: &'a AnalysisDiagnostics<P>,
}

/// Run CRAP analysis against a TypeScript / JavaScript source tree
/// with Istanbul-format coverage. Returns the analysis output
/// (functions + summary + diagnostics) as a JSON-encoded `String`.
///
/// This is the feature-independent orchestration entry point — both
/// the binary's library reuse and the napi cdylib shim funnel through
/// here. The function pre-canonicalizes `source_root` to match
/// `crap_core::core::AnalysisContext::new`'s own canonicalization, so
/// relative or symlink'd source paths do not surface as
/// `PathUnresolved` diagnostics from the Istanbul parser.
///
/// `threshold` defaults to the metric-correct preset
/// (`ThresholdPreset::Default`) when `None`.
///
/// Errors are returned as `String` so the napi shim can hand them
/// directly to `napi::Error::from_reason`; library consumers wanting
/// structured `CrapError` should call `crap_core::core::analyze`
/// directly.
pub fn analyze_to_json(
    source_root: &Path,
    coverage_path: &Path,
    threshold: Option<f64>,
    metric: ComplexityMetric,
) -> Result<String, String> {
    let src = source_root.to_path_buf();
    let src = src.canonicalize().unwrap_or(src);
    let coverage = coverage_path.to_path_buf();

    let global_threshold = threshold.unwrap_or_else(|| ThresholdPreset::Default.threshold(metric));

    let options = AnalyzeOptions {
        src: src.clone(),
        coverage,
        threshold_config: ThresholdConfig {
            global: global_threshold,
            overrides: Vec::new(),
        },
        metric,
        extensions: EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
        ..AnalyzeOptions::default()
    };

    let walker = OxcWalker::new();
    let coverage_adapter = IstanbulCoverage::new(src);

    let output = analyze::<IstanbulParseDiagnostic>(&options, &walker, &coverage_adapter)
        .map_err(|e| e.to_string())?;

    let wire = AnalyzeWireOutput {
        result: &output.result,
        diagnostics: &output.diagnostics,
    };
    serde_json::to_string(&wire).map_err(|e| e.to_string())
}
