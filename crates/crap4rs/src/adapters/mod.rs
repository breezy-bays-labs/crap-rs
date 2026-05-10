//! crap4rs adapters — Rust-specific bindings + v0.4 backward-compat
//! shims for the language-agnostic adapters that relocated to
//! `crap_core::adapters` in S3 (#135).
//!
//! Two families live here in source form:
//! - **`complexity`**: `syn`-based AST walker that extracts
//!   per-function cognitive / cyclomatic complexity.
//! - **`coverage`**: LCOV parser that converts `cargo-llvm-cov` output
//!   into `ParseOutput<LcovParseDiagnostic>`.
//!
//! Both are Rust / Rust-toolchain coupled and intentionally don't
//! relocate to `crap-core` — they would fail the AST-purity gate.
//!
//! The four language-agnostic adapter modules (`reporters`, `baseline`,
//! `config`, `diff`) re-export from `crap_core::adapters::*` below so
//! v0.4 import paths like `crap4rs::adapters::reporters::JsonConfig`
//! and `crap4rs::adapters::baseline::load(...)` keep compiling. Per-
//! symbol concretization aliases (e.g. `JsonConfig<'a>`,
//! `BaselineSnapshot`) match the v0.4 unparameterized type names so
//! consumer struct-literal init / function calls remain unchanged.

pub mod complexity;
pub mod coverage;

// ── v0.4 shim: language-agnostic adapters relocated to crap-core ────
//
// `config`, `diff`, and `reporters` (mostly) re-export verbatim because
// none of their public types reference `AnalysisDiagnostics<P>`. The
// `baseline` and `reporters::json` shims add v0.4 concretization
// aliases on top of the bare `pub use` so callers don't have to write
// `<LcovParseDiagnostic>` themselves.

pub use crap_core::adapters::{config, diff};

pub mod baseline {
    //! v0.4 shim — re-exports `crap_core::adapters::baseline::*` and
    //! concretizes `BaselineSnapshot<P>` / `load<P>` to the LCOV
    //! adapter's diagnostic shape.

    pub use crap_core::adapters::baseline::{
        BaselineError, CURRENT_SCHEMA_VERSION, SUPPORTED_SCHEMA_VERSIONS,
    };

    /// v0.4 alias of `crap_core::adapters::baseline::BaselineSnapshot<P>`,
    /// concretized to the LCOV adapter's diagnostic shape.
    pub type BaselineSnapshot = crap_core::adapters::baseline::BaselineSnapshot<
        crate::parse_diagnostic::LcovParseDiagnostic,
    >;

    /// v0.4 wrapper for `crap_core::adapters::baseline::load`,
    /// concretized to the LCOV adapter's diagnostic shape so existing
    /// `baseline::load(path)` callers keep compiling unchanged.
    pub fn load(path: &std::path::Path) -> Result<BaselineSnapshot, BaselineError> {
        crap_core::adapters::baseline::load::<crate::parse_diagnostic::LcovParseDiagnostic>(path)
    }
}

pub mod reporters {
    //! v0.4 shim — re-exports `crap_core::adapters::reporters::*` plus
    //! the JSON reporter's concretization aliases. `JsonConfig` and
    //! `DeltaContext` are relocated as generic-over-`P` types in
    //! crap-core; v0.4 paths see them as the LCOV-concretized aliases.

    pub use crap_core::adapters::reporters::{
        format_csv, format_html, format_markdown, format_sarif, format_scorecard_row, format_table,
        format_table_with_explain, render_advice_summary,
    };

    /// v0.4 alias of `crap_core::adapters::reporters::json::JsonConfig<'a, P>`,
    /// concretized to the LCOV adapter's diagnostic shape.
    pub type JsonConfig<'a> = crap_core::adapters::reporters::json::JsonConfig<
        'a,
        crate::parse_diagnostic::LcovParseDiagnostic,
    >;

    /// v0.4 wrapper for `crap_core::adapters::reporters::json::format_json`,
    /// concretized to the LCOV adapter's diagnostic shape.
    pub fn format_json(
        view: &crap_core::domain::view::AnalysisView<'_>,
        config: &JsonConfig<'_>,
    ) -> Result<String, serde_json::Error> {
        crap_core::adapters::reporters::json::format_json::<
            crate::parse_diagnostic::LcovParseDiagnostic,
        >(view, config)
    }

    pub mod json {
        //! v0.4 shim — preserves `crap4rs::adapters::reporters::json::*`
        //! paths. `JsonConfig` and `DeltaContext` are concretized to the
        //! LCOV adapter's diagnostic shape.

        pub use crap_core::adapters::reporters::json::format_json;

        /// v0.4 alias of `crap_core::adapters::reporters::json::JsonConfig<'a, P>`.
        pub type JsonConfig<'a> = crap_core::adapters::reporters::json::JsonConfig<
            'a,
            crate::parse_diagnostic::LcovParseDiagnostic,
        >;

        /// v0.4 alias of `crap_core::adapters::reporters::json::DeltaContext<'a, P>`.
        pub type DeltaContext<'a> = crap_core::adapters::reporters::json::DeltaContext<
            'a,
            crate::parse_diagnostic::LcovParseDiagnostic,
        >;
    }
}
