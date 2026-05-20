//! napi-rs Node.js binding entry point for crap4ts.
//!
//! Feature-gated behind `napi-binding`: see `crates/crap4ts/Cargo.toml`
//! `[features]`. The standalone `crap4ts` CLI binary never links
//! against Node-provided `napi_*` symbols, so this module is excluded
//! from the default build.
//!
//! A single `analyze()` export is the npm surface. Node consumers call
//! it programmatically and parse the returned JSON themselves; the
//! binding stays thin instead of re-shaping `AnalysisOutput` into napi
//! object types.

use std::path::PathBuf;

use napi_derive::napi;
use serde::Serialize;

use crap_core::core::{AnalyzeOptions as CoreAnalyzeOptions, analyze as core_analyze};
use crap_core::domain::threshold::{ThresholdConfig, ThresholdPreset};
use crap_core::domain::types::{AnalysisDiagnostics, AnalysisResult, ComplexityMetric};
use crap_core::ports::ParseDiagnostic;

use crate::adapters::coverage::IstanbulCoverage;
use crate::adapters::walker::OxcWalker;
use crate::parse_diagnostic::IstanbulParseDiagnostic;

/// File extensions the walker discovers. Mirrors
/// `crates/crap4ts/src/main.rs`'s `EXTENSIONS` constant so the napi
/// surface sees the same source set the CLI does.
const EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs"];

/// Inputs to [`analyze`]. JSON-side these map onto the same fields a
/// `crap4ts` CLI invocation would set via `--src`, `--coverage`,
/// `--threshold`, `--metric`.
#[napi(object)]
pub struct AnalyzeOptions {
    /// Absolute or workspace-relative path to the directory containing
    /// TypeScript / JavaScript source files to analyze.
    pub source_root: String,
    /// Path to the Istanbul-format coverage JSON file
    /// (`coverage-final.json`) produced by jest / vitest / nyc.
    pub coverage_path: String,
    /// CRAP score threshold above which a function is flagged. Defaults
    /// to the metric-correct preset (cyclomatic: 16.0, cognitive: 25.0).
    pub threshold: Option<f64>,
    /// Complexity metric. `"cyclomatic"` is the only metric supported
    /// in 2.0 — `"cognitive"` surfaces `CrapError::MetricNotSupported`
    /// through the returned `napi::Error`. Defaults to `"cyclomatic"`.
    pub metric: Option<String>,
}

/// Wire shape for the JSON returned by [`analyze`]. Mirrors
/// `crap_core::core::AnalysisOutput<P>`'s `{ result, diagnostics }`
/// shape verbatim so Node consumers read the same fields a Rust
/// embedder would. `#[serde(bound = "")]` suppresses serde's
/// auto-generated bounds — `P: ParseDiagnostic` already requires
/// `Serialize + DeserializeOwned`.
#[derive(Serialize)]
#[serde(bound = "")]
struct AnalyzeWireOutput<'a, P: ParseDiagnostic> {
    result: &'a AnalysisResult,
    diagnostics: &'a AnalysisDiagnostics<P>,
}

fn parse_metric(s: &str) -> Result<ComplexityMetric, String> {
    match s {
        "cyclomatic" => Ok(ComplexityMetric::Cyclomatic),
        "cognitive" => Ok(ComplexityMetric::Cognitive),
        other => Err(format!(
            "invalid metric `{other}`: expected `cyclomatic` or `cognitive`"
        )),
    }
}

/// Run CRAP analysis against a TypeScript / JavaScript source tree
/// with Istanbul-format coverage. Returns the analysis output —
/// functions, summary, diagnostics — as a JSON-encoded `String`.
///
/// Construction mirrors `crap4ts/src/main.rs`'s binary path: the same
/// `OxcWalker` + `IstanbulCoverage` ports flow through
/// `crap_core::core::analyze::<IstanbulParseDiagnostic>`. The JSON
/// shape is `{ result: AnalysisResult, diagnostics: AnalysisDiagnostics }`.
#[napi]
pub fn analyze(opts: AnalyzeOptions) -> napi::Result<String> {
    // Canonicalize matches `crap_core::core::AnalysisContext::new`'s
    // own pre-canonicalization of `options.src`. The factory closure
    // in `crap4ts::main` receives an already-canonical src from
    // `cli::run`; the napi entry has no such pre-step, so without
    // this alignment a relative or symlink'd `source_root` would
    // produce coverage records keyed off the un-canonical path while
    // the analyzer walked the canonical one — every record would
    // fail `strip_prefix` and surface as a PathUnresolved diagnostic.
    let src = PathBuf::from(&opts.source_root);
    let src = src.canonicalize().unwrap_or(src);
    let coverage = PathBuf::from(&opts.coverage_path);

    let metric = match opts.metric.as_deref() {
        Some(m) => parse_metric(m).map_err(napi::Error::from_reason)?,
        None => ComplexityMetric::Cyclomatic,
    };

    let global_threshold = opts
        .threshold
        .unwrap_or_else(|| ThresholdPreset::Default.threshold(metric));

    let analyze_options = CoreAnalyzeOptions {
        src: src.clone(),
        coverage,
        threshold_config: ThresholdConfig {
            global: global_threshold,
            overrides: Vec::new(),
        },
        metric,
        extensions: EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
        ..CoreAnalyzeOptions::default()
    };

    let walker = OxcWalker::new();
    let coverage_adapter = IstanbulCoverage::new(src);

    let output =
        core_analyze::<IstanbulParseDiagnostic>(&analyze_options, &walker, &coverage_adapter)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

    let wire = AnalyzeWireOutput {
        result: &output.result,
        diagnostics: &output.diagnostics,
    };
    serde_json::to_string(&wire).map_err(|e| napi::Error::from_reason(e.to_string()))
}
