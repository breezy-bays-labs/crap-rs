//! Istanbul-specific parse-diagnostic type for the TypeScript adapter.
//!
//! Implements `crap_core::ports::ParseDiagnostic` so it can flow through
//! the language-agnostic `AnalysisDiagnostics<P>` and `ParseOutput<P>`
//! shapes. Mirrors `crap4rs::parse_diagnostic::LcovParseDiagnostic` —
//! the variants name Istanbul-specific failure modes:
//!
//! - `PathUnresolved` — an entry's `path` field does not resolve to a
//!   discovered source file under `--src`.
//! - `MissingField` — a required field (e.g. `path`, `s`, `statementMap`)
//!   is absent from an entry.
//! - `SchemaUnrecognized` — the top-level JSON shape is not the
//!   `{[path]: { path, s, statementMap, … }}` map Istanbul emits.
//! - `BranchMismatch` — a `b` record references a `branchId` that has no
//!   corresponding entry in `branchMap` (W2.3 surfaces this; the variant
//!   lands in W1.1 so W2.3 is purely consumer-side).
//!
//! The shape is a **flat struct** (not a tagged enum) per breadboard
//! W-3: the diagnostic carries `file_path`, `kind`, `message`, and an
//! optional `line`. Construction sites pre-render the user-facing
//! message (with detected-vs-expected hints, redirect URLs, etc.) into
//! `message`, so the `Display` impl just echoes those fields.

use crap_core::ports::ParseDiagnostic;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Non-fatal issue encountered during Istanbul JSON coverage parsing.
///
/// Constructed by `IstanbulCoverage::parse` (and W2.3's branch-coverage
/// pass) when a per-entry record cannot be consumed but the overall
/// scorecard should still produce results for the other entries — per
/// Resolved Q10 in shaping, parsers NEVER silent-drop and NEVER abort
/// first-record. The diagnostic surfaces in the JSON envelope under
/// `analysis_output.diagnostics.parse_diagnostics[]` so downstream
/// consumers (Scorecard action, mokumo) can decide whether to surface
/// or ignore.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IstanbulParseDiagnostic {
    /// The raw `path` field from the Istanbul entry (pre-normalization).
    /// For `SchemaUnrecognized` where there is no per-entry context, this
    /// is the empty string.
    pub file_path: String,

    /// Classification of the failure mode.
    pub kind: IstanbulDiagnosticKind,

    /// Pre-rendered user-facing message. Built at construction time so
    /// the message convention (detected-vs-expected hints, redirect
    /// URLs, etc.) lives next to the call site that knows the context.
    pub message: String,

    /// 1-based source line associated with the diagnostic when one is
    /// available (e.g. from a `statementMap` entry). `None` when the
    /// diagnostic does not bind to a specific line.
    pub line: Option<u32>,
}

/// Classification of an `IstanbulParseDiagnostic` — what went wrong.
///
/// Serialized as a kebab-case slug (`path-unresolved`,
/// `schema-unrecognized`, etc.) so the JSON envelope matches the
/// vocabulary the BDD `.feature` files assert against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IstanbulDiagnosticKind {
    /// The entry's `path` does not resolve to a discovered source file
    /// under `--src`. Most common cause: paths in `coverage-final.json`
    /// are absolute `/Users/.../project/src/foo.ts` while `--src` is a
    /// peer directory or a build-output tree.
    PathUnresolved,

    /// A required field is absent from the entry (e.g., an entry has
    /// `s` but no `statementMap`, or no `path`).
    MissingField,

    /// The top-level JSON shape is not the `{[path]: { path, s,
    /// statementMap, … }}` map Istanbul emits. Surfaced by `validate`
    /// before the full parse pass when possible.
    SchemaUnrecognized,

    /// A `b` (branch counts) record references a `branchId` that has no
    /// corresponding entry in `branchMap`. Lands in the W1.1 type
    /// surface; W2.3 (Istanbul branch coverage) populates it. The
    /// associated message redirects users to the coverage tool's issue
    /// tracker because this typically reflects a bug in the emitter.
    BranchMismatch,
}

impl fmt::Display for IstanbulDiagnosticKind {
    /// Renders the kebab-case slug matching the serde wire form.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let slug = match self {
            Self::PathUnresolved => "path-unresolved",
            Self::MissingField => "missing-field",
            Self::SchemaUnrecognized => "schema-unrecognized",
            Self::BranchMismatch => "branch-mismatch",
        };
        f.write_str(slug)
    }
}

impl fmt::Display for IstanbulParseDiagnostic {
    /// Renders `<kind>: <message>` with optional `(line N)` suffix.
    /// The message field is pre-rendered at construction time, so the
    /// `Display` impl is a thin formatter that adapter-aware reporters
    /// can reuse for table cells or warning lines.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}: {} (line {})", self.kind, self.message, line),
            None => write!(f, "{}: {}", self.kind, self.message),
        }
    }
}

impl ParseDiagnostic for IstanbulParseDiagnostic {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_kind_display_is_kebab_case() {
        assert_eq!(
            IstanbulDiagnosticKind::PathUnresolved.to_string(),
            "path-unresolved"
        );
        assert_eq!(
            IstanbulDiagnosticKind::MissingField.to_string(),
            "missing-field"
        );
        assert_eq!(
            IstanbulDiagnosticKind::SchemaUnrecognized.to_string(),
            "schema-unrecognized"
        );
        assert_eq!(
            IstanbulDiagnosticKind::BranchMismatch.to_string(),
            "branch-mismatch"
        );
    }

    #[test]
    fn diagnostic_display_path_unresolved_no_line() {
        let d = IstanbulParseDiagnostic {
            file_path: "/build/transpiled/foo.js".into(),
            kind: IstanbulDiagnosticKind::PathUnresolved,
            message: "path '/build/transpiled/foo.js' does not resolve under /src".into(),
            line: None,
        };
        assert_eq!(
            d.to_string(),
            "path-unresolved: path '/build/transpiled/foo.js' does not resolve under /src"
        );
    }

    #[test]
    fn diagnostic_display_with_line() {
        let d = IstanbulParseDiagnostic {
            file_path: "src/foo.ts".into(),
            kind: IstanbulDiagnosticKind::MissingField,
            message: "missing required field 'statementMap' in record for 'src/foo.ts'".into(),
            line: Some(42),
        };
        assert_eq!(
            d.to_string(),
            "missing-field: missing required field 'statementMap' in record for 'src/foo.ts' (line 42)"
        );
    }

    #[test]
    fn serde_round_trip_path_unresolved() {
        let d = IstanbulParseDiagnostic {
            file_path: "/x/y/z.ts".into(),
            kind: IstanbulDiagnosticKind::PathUnresolved,
            message: "path '/x/y/z.ts' does not resolve under /src".into(),
            line: None,
        };
        let s = serde_json::to_string(&d).unwrap();
        // Verify wire shape uses kebab-case for kind.
        assert!(s.contains("\"kind\":\"path-unresolved\""), "wire form: {s}");
        let back: IstanbulParseDiagnostic = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn serde_round_trip_missing_field() {
        let d = IstanbulParseDiagnostic {
            file_path: "src/foo.ts".into(),
            kind: IstanbulDiagnosticKind::MissingField,
            message: "missing required field 'statementMap' in record for 'src/foo.ts'".into(),
            line: Some(7),
        };
        let s = serde_json::to_string(&d).unwrap();
        assert!(s.contains("\"kind\":\"missing-field\""), "wire form: {s}");
        let back: IstanbulParseDiagnostic = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn serde_round_trip_schema_unrecognized() {
        let d = IstanbulParseDiagnostic {
            file_path: String::new(),
            kind: IstanbulDiagnosticKind::SchemaUnrecognized,
            message: "top-level shape not recognized as Istanbul; expected `{[path]: { path, s, statementMap, … }}`; received: object".into(),
            line: None,
        };
        let s = serde_json::to_string(&d).unwrap();
        assert!(
            s.contains("\"kind\":\"schema-unrecognized\""),
            "wire form: {s}"
        );
        let back: IstanbulParseDiagnostic = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn serde_round_trip_branch_mismatch() {
        let d = IstanbulParseDiagnostic {
            file_path: "src/foo.ts".into(),
            kind: IstanbulDiagnosticKind::BranchMismatch,
            message: "branch coverage record for branchId `42` references no entry in branchMap. This is likely a bug in your coverage tool — report at the coverage tool's issue tracker.".into(),
            line: None,
        };
        let s = serde_json::to_string(&d).unwrap();
        assert!(s.contains("\"kind\":\"branch-mismatch\""), "wire form: {s}");
        let back: IstanbulParseDiagnostic = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }
}
