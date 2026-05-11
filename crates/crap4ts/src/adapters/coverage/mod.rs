//! Istanbul JSON coverage parser — STUB.
//!
//! The real implementation will deserialize `coverage-final.json`
//! (Istanbul's per-file `statementMap` / `s` / `branchMap` / `b`
//! shape) into `ParseOutput<IstanbulParseDiagnostic>`, mirroring
//! `crates/crap4rs/src/adapters/coverage/mod.rs`. It lands in the
//! follow-up `crap-rs/2026XXXX-typescript-adapter` pipeline.
//!
//! ALPHA: invoking `parse(...)` runtime-panics with `unimplemented!`.
//! The struct exists so the trait bound is wired and the binary's
//! `--help` / `--version` paths (which never call into `parse`) work
//! today.

use crap_core::domain::types::CrapError;
use crap_core::ports::{CoveragePort, ParseOutput};
use std::path::PathBuf;

use crate::parse_diagnostic::IstanbulParseDiagnostic;

/// Istanbul JSON coverage parser — stub. Implements `CoveragePort`
/// with `Diagnostic = IstanbulParseDiagnostic` so the binary's
/// `crap_core::cli::run` dispatch picks it up.
pub struct IstanbulCoverage {
    /// Source root the parser will eventually use to canonicalise the
    /// per-file `path` field that Istanbul emits. Stored but unused in
    /// alpha — the real parser will consume it.
    _root: PathBuf,
}

impl IstanbulCoverage {
    pub fn new(root: PathBuf) -> Self {
        Self { _root: root }
    }
}

impl CoveragePort for IstanbulCoverage {
    type Diagnostic = IstanbulParseDiagnostic;

    fn parse(&self, _data: &str) -> Result<ParseOutput<Self::Diagnostic>, CrapError> {
        unimplemented!("Istanbul JSON parser lands in the typescript-adapter follow-up pipeline")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn istanbul_coverage_constructible() {
        let _c = IstanbulCoverage::new(PathBuf::from("/tmp"));
    }

    #[test]
    #[should_panic(
        expected = "Istanbul JSON parser lands in the typescript-adapter follow-up pipeline"
    )]
    fn istanbul_coverage_parse_unimplemented() {
        let c = IstanbulCoverage::new(PathBuf::from("/tmp"));
        let _ = c.parse("");
    }
}
