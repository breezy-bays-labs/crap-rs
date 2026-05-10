//! Istanbul-specific parse-diagnostic type for the TypeScript adapter.
//!
//! Implements `crap_core::ports::ParseDiagnostic` so it can flow through
//! the language-agnostic `AnalysisDiagnostics<P>` and `ParseOutput<P>`
//! shapes. Mirrors `crap4rs::parse_diagnostic::LcovParseDiagnostic` —
//! the variants are Istanbul-flavoured placeholders. The Istanbul JSON
//! parser is a stub in S5, so these variants are not constructed by
//! any code path today; they exist so the trait bound + serde
//! envelope are real (not phantom) before the parser lands.

use crap_core::ports::ParseDiagnostic;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Non-fatal issues encountered during Istanbul JSON coverage parsing.
///
/// ALPHA: no code path constructs these variants yet — the Istanbul
/// parser at `crate::adapters::coverage::IstanbulCoverage` runtime-
/// panics. The variants are picked to mirror the shape of LCOV's
/// `MalformedRecord` / `EmptySourceFile` so the JSON envelope
/// surfaced for downstream consumers (Scorecard action, mokumo) is
/// structurally consistent across adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IstanbulParseDiagnostic {
    /// A JSON document was structurally invalid (parse error).
    MalformedJson {
        /// 1-based line number in the input where parsing failed, if
        /// the parser surfaces one. `0` is used when the parser cannot
        /// localise the error.
        line_number: usize,
        /// Short description from the underlying parser.
        content: String,
    },
    /// A coverage record was missing a required field.
    MissingField {
        /// The dotted JSON path of the missing field
        /// (e.g. `path`, `s`, `b`, `fnMap`).
        field: String,
    },
}

impl fmt::Display for IstanbulParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedJson {
                line_number,
                content,
            } => write!(f, "line {line_number}: malformed JSON: {content}"),
            Self::MissingField { field } => {
                write!(f, "missing required field: {field}")
            }
        }
    }
}

impl ParseDiagnostic for IstanbulParseDiagnostic {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diagnostic_display_malformed_json() {
        let d = IstanbulParseDiagnostic::MalformedJson {
            line_number: 12,
            content: "expected `,` or `}`".to_string(),
        };
        assert_eq!(
            d.to_string(),
            "line 12: malformed JSON: expected `,` or `}`"
        );
    }

    #[test]
    fn parse_diagnostic_display_missing_field() {
        let d = IstanbulParseDiagnostic::MissingField {
            field: "path".to_string(),
        };
        assert_eq!(d.to_string(), "missing required field: path");
    }

    #[test]
    fn parse_diagnostic_serde_round_trip() {
        let d = IstanbulParseDiagnostic::MalformedJson {
            line_number: 3,
            content: "unexpected end of input".to_string(),
        };
        let s = serde_json::to_string(&d).unwrap();
        let back: IstanbulParseDiagnostic = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }
}
