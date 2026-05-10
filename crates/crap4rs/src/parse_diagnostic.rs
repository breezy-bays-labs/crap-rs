//! LCOV-specific parse-diagnostic type for the Rust adapter.
//!
//! Implements `crap_core::ports::ParseDiagnostic` so it can flow through
//! the language-agnostic `AnalysisDiagnostics<P>` and `ParseOutput<P>`
//! shapes. v0.4 named this type `ParseDiagnostic`; v0.5 renames to
//! `LcovParseDiagnostic` (the v0.4 path is preserved as a shim alias —
//! `crap4rs::domain::types::ParseDiagnostic = LcovParseDiagnostic`,
//! per ADR D10).

use crap_core::ports::ParseDiagnostic;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Non-fatal issues encountered during LCOV coverage parsing.
///
/// Pre-extraction this lived as `crap4rs::domain::types::ParseDiagnostic`.
/// In v0.5 it moved here (the LCOV adapter side) and the old path
/// resolves via the shim alias. v1.0 will narrow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LcovParseDiagnostic {
    /// A DA record could not be parsed (bad format, missing fields, invalid values).
    MalformedRecord {
        /// 1-based line number in the LCOV input where the issue occurred.
        line_number: usize,
        /// The raw line content that failed to parse.
        content: String,
    },
    /// An SF record had an empty path.
    EmptySourceFile {
        /// The 1-based line number in the LCOV input.
        line_number: usize,
    },
}

impl fmt::Display for LcovParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedRecord {
                line_number,
                content,
            } => write!(f, "line {line_number}: malformed record: {content}"),
            Self::EmptySourceFile { line_number } => {
                write!(f, "line {line_number}: empty SF path")
            }
        }
    }
}

impl ParseDiagnostic for LcovParseDiagnostic {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diagnostic_display_malformed_record() {
        let d = LcovParseDiagnostic::MalformedRecord {
            line_number: 42,
            content: "DA:bad".to_string(),
        };
        assert_eq!(d.to_string(), "line 42: malformed record: DA:bad");
    }

    #[test]
    fn parse_diagnostic_display_empty_source_file() {
        let d = LcovParseDiagnostic::EmptySourceFile { line_number: 7 };
        assert_eq!(d.to_string(), "line 7: empty SF path");
    }

    #[test]
    fn parse_diagnostic_serde_round_trip() {
        let d = LcovParseDiagnostic::MalformedRecord {
            line_number: 3,
            content: "DA:nope".to_string(),
        };
        let s = serde_json::to_string(&d).unwrap();
        let back: LcovParseDiagnostic = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }
}
