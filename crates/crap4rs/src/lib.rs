//! crap4rs — Rust adapter binding for the language-agnostic `crap_core`
//! analyzer.
//!
//! This crate exposes the LCOV/`syn` adapter pipeline plus the CLI / core
//! orchestration. The v0.4.0 public surface is preserved as `pub mod`
//! shims that mirror the original module structure but re-export from
//! `crap_core` (per ADR D10 — public-API shim-module pattern). v0.5.x
//! library consumers' existing imports compile unchanged; v1.0 will
//! narrow the shims.

pub mod adapters; // still in crap4rs through S2 (relocates in S3)
pub mod core; // still in crap4rs through S3 (relocates in S4)
pub mod parse_diagnostic;
// `cli` is bin-only — declared in src/main.rs as `mod cli;` so the
// in-crate `use crap4rs::...` imports inside cli/mod.rs resolve to
// the lib (the bin is a separate crate that depends on the lib by
// name). Keeping cli OUT of the lib preserves the v0.4.0 build shape.

// ── v0.4 backward-compat shim modules (ADR D10) ─────────────────────
//
// Every nested path that resolved against v0.4.0 (e.g.
// `crap4rs::domain::types::ParseDiagnostic`,
// `crap4rs::ports::ParseOutput`) continues to resolve against v0.5.0.
// Per-symbol re-exports are enumerated (NOT `pub use ::*`) because the
// concretized type aliases (`AnalysisDiagnostics`, `ParseOutput`) would
// collide with their generic crap-core counterparts under glob re-export.

pub mod domain {
    //! v0.4 shim — re-exports `crap_core::domain` submodules under the
    //! original `crap4rs::domain::*` namespace.

    pub use crap_core::domain::{crap, delta, diagnostic, matching, summary, threshold, view};

    pub mod types {
        //! v0.4 shim — preserves `crap4rs::domain::types::*` paths.
        //!
        //! `ParseDiagnostic` is the v0.4 alias of v0.5's
        //! `LcovParseDiagnostic` (the LCOV-specific concrete impl moved
        //! out of crap-core). `AnalysisDiagnostics` is concretized to
        //! `AnalysisDiagnostics<LcovParseDiagnostic>` so v0.4 consumers
        //! that wrote it without a type parameter keep compiling.

        pub use crap_core::domain::types::{
            AnalysisResult, AnalysisSummary, BranchCoverage, ComplexityContributor,
            ComplexityMetric, ContributorKind, CoverageMetric, CoverageRatio, CrapError, CrapScore,
            Diagnostic, FileChangeKind, FunctionComplexity, FunctionCoverage, FunctionIdentity,
            FunctionVerdict, LineCoverage, RiskDistribution, RiskLevel, ScoredFunction, SourceSpan,
        };

        /// v0.4 alias of [`crate::parse_diagnostic::LcovParseDiagnostic`].
        pub use crate::parse_diagnostic::LcovParseDiagnostic as ParseDiagnostic;

        /// v0.4 alias of `crap_core::domain::types::AnalysisDiagnostics<P>`,
        /// concretized to the LCOV adapter's diagnostic shape.
        pub type AnalysisDiagnostics = crap_core::domain::types::AnalysisDiagnostics<
            crate::parse_diagnostic::LcovParseDiagnostic,
        >;
    }
}

pub mod ports {
    //! v0.4 shim — re-exports `crap_core::ports::*` and concretizes
    //! `ParseOutput` to the LCOV adapter's diagnostic type.

    pub use crap_core::ports::{ComplexityPort, CoveragePort, DiffPort, ParseDiagnostic};

    /// v0.4 alias of `crap_core::ports::ParseOutput<P>`, concretized to
    /// the LCOV adapter's diagnostic shape.
    pub type ParseOutput =
        crap_core::ports::ParseOutput<crate::parse_diagnostic::LcovParseDiagnostic>;
}
