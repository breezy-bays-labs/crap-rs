//! Oxc-based TypeScript complexity walker — STUB.
//!
//! The real implementation will use `oxc::parser` to walk the AST and
//! count decision points for cognitive / cyclomatic complexity, mirror-
//! ing `crates/crap4rs/src/adapters/complexity/mod.rs`. It lands in
//! the follow-up `crap-rs/2026XXXX-typescript-adapter` pipeline (see
//! breezy-bays-labs/crap4rs#137 follow-up).
//!
//! ALPHA: invoking `extract(...)` runtime-panics with `unimplemented!`.
//! The struct exists so the trait bound is wired and the binary's
//! `--help` / `--version` paths (which never call into `extract`)
//! work today.

use crap_core::domain::types::{ComplexityMetric, CrapError, FunctionComplexity};
use crap_core::ports::ComplexityPort;

/// Oxc-based complexity extractor — stub. Implements `ComplexityPort`
/// so the binary's `crap_core::cli::run` dispatch picks it up.
pub struct OxcWalker {
    _private: (),
}

impl OxcWalker {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for OxcWalker {
    fn default() -> Self {
        Self::new()
    }
}

impl ComplexityPort for OxcWalker {
    fn extract(
        &self,
        _source: &str,
        _file_path: &str,
        _metric: ComplexityMetric,
    ) -> Result<Vec<FunctionComplexity>, CrapError> {
        unimplemented!(
            "oxc walker lands in the typescript-adapter follow-up pipeline (see crap4rs#137)"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oxc_walker_constructible() {
        let _w = OxcWalker::new();
        let _w2 = OxcWalker::default();
    }

    #[test]
    #[should_panic(expected = "oxc walker lands in the typescript-adapter follow-up pipeline")]
    fn oxc_walker_extract_unimplemented() {
        let w = OxcWalker::new();
        let _ = w.extract("", "stub.ts", ComplexityMetric::Cognitive);
    }
}
