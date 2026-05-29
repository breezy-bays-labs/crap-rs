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

use crap_core::core::identity::IdentityBase;
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

/// File extensions the walker discovers. The single source of truth
/// for the crap4ts source set — the CLI binary (`src/main.rs`) and the
/// library entry point [`analyze_to_json`] both reference this constant
/// so a CLI run and a programmatic run analyze identical file sets.
pub const EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs"];

/// Glob patterns excluded from analysis by default — vendored
/// dependencies (`node_modules`) and build / coverage output. The
/// single source of truth: the CLI advertises these as the
/// commented-out excludes its `init` subcommand writes, and the
/// library entry point [`analyze_to_json`] applies them directly so a
/// programmatic caller pointed at a project root does not walk into
/// `node_modules` (which, for the CLI, the config-merge step filters).
pub const DEFAULT_EXCLUDES: &[&str] = &["node_modules/**", "dist/**", "coverage/**"];

/// Adapter-mandated structural skips applied to every analysis run
/// regardless of CLI flags or operator config. crap4ts skips
/// TypeScript declaration files (`*.d.ts`) because they contain only
/// ambient type declarations — never executable code — so they
/// contribute zero useful complexity or coverage signal and showing
/// them in a CRAP report is always misleading (crap-rs#253). Flowed
/// through both the CLI path (`AdapterMeta::forced_excludes`) and the
/// library / napi path ([`analyze_to_json`]) so a programmatic caller
/// gets the same skip behavior as the CLI.
pub const FORCED_EXCLUDES: &[&str] = &["**/*.d.ts"];

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
/// This is the feature-independent orchestration entry point — the napi
/// cdylib shim and the crate's integration tests both funnel through
/// here. The function pre-canonicalizes `source_root` to match
/// `crap_core::core::AnalysisContext::new`'s own canonicalization, so
/// relative or symlink'd source paths do not surface as
/// `PathUnresolved` diagnostics from the Istanbul parser.
///
/// `threshold` defaults to the metric-correct preset
/// (`ThresholdPreset::Default`) when `None`.
///
/// [`DEFAULT_EXCLUDES`] is applied so a caller pointed at a project
/// root does not walk into `node_modules`, `dist`, or `coverage`.
/// [`FORCED_EXCLUDES`] is also applied so `.d.ts` declaration files
/// are skipped on this path (the napi shim funnels through here);
/// without it, programmatic callers would see ambient-type entries
/// the CLI doesn't (crap-rs#253).
///
/// Inputs are validated up front: `source_root` must resolve to an
/// existing directory, and `threshold` — when set — must be a finite
/// non-negative number. Either failure returns a descriptive `Err`
/// rather than walking an empty tree or scoring against a `NaN`
/// threshold.
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
    if let Some(t) = threshold
        && (!t.is_finite() || t < 0.0)
    {
        return Err(format!(
            "crap4ts: invalid threshold {t}: expected a finite number >= 0"
        ));
    }

    if !source_root.is_dir() {
        return Err(format!(
            "crap4ts: source_root '{}' does not exist or is not a directory",
            source_root.display()
        ));
    }

    let src = source_root.to_path_buf();
    let src = src.canonicalize().unwrap_or(src);
    let coverage = coverage_path.to_path_buf();

    let global_threshold = threshold.unwrap_or_else(|| ThresholdPreset::Default.threshold(metric));

    let options = AnalyzeOptions {
        // Single-root TypeScript analysis: src-relative identity,
        // byte-identical to before multi-root (crap-rs#336). The base
        // holds the canonicalized `src` so the strip matches both the
        // discovery root and `IstanbulCoverage::new(src)`'s anchor.
        identity_base: IdentityBase::SrcRelative(src.clone()),
        src: vec![src.clone()],
        coverage,
        threshold_config: ThresholdConfig {
            global: global_threshold,
            overrides: Vec::new(),
        },
        metric,
        extensions: EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
        // Forced excludes lead so a programmatic caller can't override
        // structural skips (`*.d.ts` per crap-rs#253), DEFAULT_EXCLUDES
        // follows for vendored / build / coverage paths.
        exclude: FORCED_EXCLUDES
            .iter()
            .chain(DEFAULT_EXCLUDES.iter())
            .map(|&s| s.to_string())
            .collect(),
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
