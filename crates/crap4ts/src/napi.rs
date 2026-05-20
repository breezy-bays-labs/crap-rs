//! napi-rs Node.js binding entry point for crap4ts.
//!
//! Feature-gated behind `napi-binding`: see `crates/crap4ts/Cargo.toml`
//! `[features]`. The standalone `crap4ts` CLI binary never links
//! against Node-provided `napi_*` symbols, so this module is excluded
//! from the default build.
//!
//! Thin shim — the orchestration logic lives in [`crate::analyze_to_json`]
//! so it stays feature-independent and gets exercised by Rust
//! integration tests + the LCOV coverage gate. This module only
//! handles unpacking `AnalyzeOptions` and mapping `String` errors to
//! `napi::Error`.

use std::path::Path;

use napi_derive::napi;

use crap_core::domain::types::ComplexityMetric;

use crate::analyze_to_json;

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
/// `crap_core::core::analyze::<IstanbulParseDiagnostic>` via the
/// crate-internal [`crate::analyze_to_json`] helper. The JSON shape
/// is `{ result: AnalysisResult, diagnostics: AnalysisDiagnostics }`.
#[napi]
pub fn analyze(opts: AnalyzeOptions) -> napi::Result<String> {
    let metric = match opts.metric.as_deref() {
        Some(m) => parse_metric(m).map_err(napi::Error::from_reason)?,
        None => ComplexityMetric::Cyclomatic,
    };
    analyze_to_json(
        Path::new(&opts.source_root),
        Path::new(&opts.coverage_path),
        opts.threshold,
        metric,
    )
    .map_err(napi::Error::from_reason)
}
