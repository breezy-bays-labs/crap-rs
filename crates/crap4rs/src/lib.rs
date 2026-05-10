//! crap4rs — Rust adapter binding for the language-agnostic `crap_core`
//! analyzer.
//!
//! This crate exposes the LCOV/`syn` adapter pipeline. Domain types,
//! port traits, language-agnostic adapters (reporters, baseline, config,
//! diff), the orchestrator (`core::analyze`), and the CLI dispatch shell
//! (`cli`) all live in `crap_core` post-S4 (#136). The v0.4.0 public
//! surface is preserved as `pub mod` shims that mirror the original
//! module structure but re-export from `crap_core` (per ADR D10 —
//! public-API shim-module pattern). v0.5.x library consumers' existing
//! imports compile unchanged; v1.0 will narrow the shims.

pub mod adapters;
pub mod parse_diagnostic;

// ── v0.4 backward-compat shim modules (ADR D10) ─────────────────────
//
// Every nested path that resolved against v0.4.0 (e.g.
// `crap4rs::domain::types::ParseDiagnostic`,
// `crap4rs::ports::ParseOutput`, `crap4rs::core::AnalyzeOptions`,
// `crap4rs::cli::Args`) continues to resolve against v0.5.0.
// Per-symbol re-exports are enumerated (NOT `pub use ::*`) because the
// concretized type aliases (`AnalysisDiagnostics`, `ParseOutput`,
// `AnalysisOutput`) would collide with their generic crap-core
// counterparts under glob re-export.

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

pub mod core {
    //! v0.4 shim — relocated to `crap_core::core` in S4 (#136). The
    //! orchestrator's signature gained `<P: ParseDiagnostic>` plus
    //! injected `&dyn ComplexityPort` + `&dyn CoveragePort<Diagnostic
    //! = P>` parameters; the v0.4 alias re-exports under the
    //! LCOV-concretized name so existing consumers keep compiling.

    pub use crap_core::core::AnalyzeOptions;
    pub use crap_core::core::analyze;

    /// v0.4 alias of `crap_core::core::AnalysisOutput<P>`,
    /// concretized to the LCOV adapter's diagnostic shape.
    pub type AnalysisOutput =
        crap_core::core::AnalysisOutput<crate::parse_diagnostic::LcovParseDiagnostic>;
}

pub mod cli {
    //! v0.4 shim — relocated to `crap_core::cli` in S4 (#136). The
    //! orchestrator `cli::run` gained `<P: ParseDiagnostic + Display>`
    //! plus injected port parameters and threaded `tool_version` /
    //! `long_version` strings (per S4 lesson 7 — env vars set by the
    //! binary's build.rs don't reach crap-core's compile, so the
    //! caller passes them at runtime). The bare path
    //! `crap4rs::cli::Args` resolves through this shim.

    pub use crap_core::cli::{
        Cli, Cli as Args, ColorArg, Command, DeltaKindArg, DeltaSortKeyArg, DisplayArgs,
        FilterArgs, FormatArg, FormatSpec, GroupByArg, InputArgs, MetricArg, OutputArgs, ShellArg,
        SortKeyArg, run,
    };
}
