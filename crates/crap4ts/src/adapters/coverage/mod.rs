//! Istanbul JSON coverage parser — statement + branch + schema-tolerant.
//!
//! Consumes the per-file `coverage-final.json` map jest, vitest, and
//! nyc emit. Each entry maps a `path` to a `statementMap` (statement
//! id → `{start: {line, column}, end: {line, column}}`) and an `s`
//! hash (statement id → exec count). The parser joins these two maps
//! to produce one `LineCoverage { line, hits }` record per statement
//! in `statementMap`, keyed by the entry's path (after normalization
//! against `effective_src` so the downstream line-range join sees
//! workspace-relative paths).
//!
//! ## Scope
//!
//! - **W1.1**: statement coverage (`s` + `statementMap`) + jest-flat
//!   top-level shape + `PathUnresolved` diagnostics. Branch- /
//!   schema-variance fields landed in W1.1's struct with
//!   `#[serde(default)]` so the consumer-side extensions stay
//!   type-stable.
//! - **W2.3 (#186)**: branch coverage. Each Istanbul branchId in `b`
//!   carries a `Vec<u64>` of per-arm hit counts; the parser fans this
//!   into one `BranchCoverage` row per arm keyed at the `branchMap`
//!   entry's start line. Orphan branchIds (`b` references a missing
//!   `branchMap` entry) emit `BranchMismatch` and skip THAT branch
//!   only — the rest of the file still parses.
//! - **W2.4 (#187)**: jest / vitest / nyc + wrapped emitter
//!   tolerance. The parser tries the flat `{[path]: entry}` shape
//!   first, then a single-level unwrap (`{"coverage-final":
//!   {...flat...}}`), then emits `SchemaUnrecognized` with the
//!   detected top-level keys. `MissingField` covers entries that
//!   have `s` records but an empty `statementMap` (or vice versa).
//!
//! ## Decision points (locked)
//!
//! - **No `f`/`fnMap` backfill** — W1.1 discovery 5a–5d empirically
//!   showed `s`/`statementMap` data is sufficient for arrow-function
//!   coverage (CLAUDE.md locked decision #13).
//! - **`ParseOutput.branches` is internal** — `crap-core::core::analyze`
//!   lowers branch data into per-function `branch_coverage:
//!   Option<CoverageRatio>` records, but the JSON envelope only
//!   surfaces `coverage_percent` (= line coverage). Branch records
//!   never reach the wire — verified by `wire_envelope_crap4ts` (no
//!   `branchCoverage` field in any emitted row).
//!
//! ## Path normalization
//!
//! Mirrors `crap4rs::adapters::coverage::LcovParser::normalize_path` —
//! a pure `strip_prefix(effective_src)`. The orchestrator pre-
//! canonicalizes `effective_src` at the factory-closure boundary, so
//! the parser does no filesystem I/O during path joining. Entries
//! whose paths fall outside `effective_src` emit an
//! `IstanbulParseDiagnostic { kind: PathUnresolved, … }` and are
//! skipped — per Resolved Q10 in shaping, the scorecard NEVER aborts
//! on first failure and NEVER silent-drops a record.

use crap_core::domain::types::{BranchCoverage, CrapError, LineCoverage};
use crap_core::ports::{CoveragePort, ParseOutput};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::parse_diagnostic::{IstanbulDiagnosticKind, IstanbulParseDiagnostic};

/// Istanbul JSON coverage parser.
///
/// Constructed via [`IstanbulCoverage::new`] with the post-merge,
/// pre-canonicalized source root. Implements `CoveragePort` with
/// `Diagnostic = IstanbulParseDiagnostic` so the binary's
/// `crap_core::cli::run` dispatch picks it up via the factory closure
/// in `src/main.rs`.
pub struct IstanbulCoverage {
    /// Canonical source root used to normalize per-entry `path`
    /// fields. The orchestrator's `canonicalize_src` call in
    /// `crap_core::cli::run` hands us an already-canonical path, so
    /// the parser does no further `canonicalize()` walking inside
    /// `normalize_path`.
    effective_src: PathBuf,
}

impl IstanbulCoverage {
    /// Construct a new parser rooted at the post-merge `--src` value.
    ///
    /// Callers in `crap_core::cli::run` pass the already-canonicalized
    /// effective source root; smoke tests should canonicalize the
    /// temp-dir root before passing it through so fixture path
    /// stripping matches.
    pub fn new(root: PathBuf) -> Self {
        Self {
            effective_src: root,
        }
    }

    /// Strip `effective_src` from `raw` and return a workspace-relative
    /// path, or `None` if the entry resolves outside the source tree.
    ///
    /// Pure: no filesystem I/O. Mirrors `LcovParser::normalize_path`
    /// in `crap4rs`. The orchestrator canonicalizes `effective_src`
    /// before construction, so no further `canonicalize()` walk is
    /// needed here. Joining relative entries against `effective_src`
    /// and stripping the prefix is the same two-step the LCOV parser
    /// uses on its own platform.
    fn normalize_path(&self, raw: &str) -> Option<PathBuf> {
        let path = PathBuf::from(raw);
        let joined = if path.is_absolute() {
            path
        } else {
            self.effective_src.join(raw)
        };
        joined
            .strip_prefix(&self.effective_src)
            .ok()
            .map(|p| p.to_path_buf())
    }
}

// ── Internal Istanbul schema types ───────────────────────────────────
//
// All types are private (parser-internal). They model the minimal
// jest-flavored shape W1.1 consumes; `#[serde(default)]` on optional
// fields keeps deserialization permissive against jest/vitest/nyc
// metadata fields we don't care about (`hash`, `contentHash`, `all`,
// etc.) — serde drops unknown fields by default.

/// One per-file entry in `coverage-final.json`. Field types are widened
/// to `u64` (vs the breadboard's `u32`) to handle high-iteration code
/// — Istanbul's `s` counts can comfortably exceed `u32::MAX` on stress
/// tests, and `LineCoverage.hits` is `u64` downstream anyway.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IstanbulCoverageFile {
    path: String,
    /// `statement_id` → execution count.
    #[serde(default)]
    s: HashMap<String, u64>,
    /// `statement_id` → `{ start: {line, column}, end: {line, column} }`.
    #[serde(default)]
    statement_map: HashMap<String, StatementLoc>,
    /// W2.3 (now consumed): `branch_id` → `[hit count per branch arm]`.
    /// Each branchId maps to a `Vec<u64>` of arm-level hit counts; the
    /// parser fans these into one `BranchCoverage` row per arm at the
    /// associated `branchMap[id]` start line.
    #[serde(default)]
    b: HashMap<String, Vec<u64>>,
    /// W2.3 (now consumed): `branch_id` → branch location. See `b`
    /// above.
    #[serde(default)]
    branch_map: HashMap<String, BranchLoc>,
    /// `function_id` → execution count. Locked decision #13: NOT
    /// consumed — W1.1 discovery 5a–5d showed `s`/`statementMap` is
    /// sufficient for arrow-function coverage. Field kept for forward
    /// compatibility (and to keep test fixtures faithful to real
    /// emitter output) so future inclusion is consumer-side only.
    #[serde(default)]
    #[allow(dead_code)]
    f: HashMap<String, u64>,
    /// `function_id` → function metadata. See `f` above; not consumed
    /// in v2.0.0.
    #[serde(default)]
    #[allow(dead_code)]
    fn_map: HashMap<String, FnLoc>,
}

/// Range location for a statement (or function decl/body). `start`
/// and `end` carry 1-based line numbers + 0-based columns.
#[derive(Debug, Deserialize)]
struct StatementLoc {
    start: Position,
    #[allow(dead_code)]
    end: Position,
}

/// 1-based line + 0-based column. Matches Istanbul's emitter
/// convention; downstream `LineCoverage.line` is 1-based `usize`.
///
/// `column` is `Option<u32>` because `@vitest/coverage-istanbul` (and
/// possibly other producers) emit `"column": null` on the `end` side of
/// every span, signalling "unknown column" — the underlying V8
/// inspector data they transform doesn't always have a precise
/// end-column. crap4ts line-range matching is line-only, so the
/// column value is advisory and never consulted; accepting `null` is
/// semantically a no-op. Surfaced by W3.1's crap4ts@1.x corpus
/// capture (#189) where 1,943 of 4,696 columns were null; tracked
/// fix: #211.
#[derive(Debug, Deserialize)]
struct Position {
    line: u32,
    #[allow(dead_code)]
    column: Option<u32>,
}

/// W2.3 branch metadata. `kind` (e.g. `"if"`, `"switch"`, `"cond-expr"`)
/// is carried but not consumed by the parser — line attribution joins
/// only on the `loc.start.line` value (or the optional emitter-set
/// `line` field when present, which some emitters use as the
/// branching-site line independent of `loc`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BranchLoc {
    loc: StatementLoc,
    /// Istanbul branch kind (`if`, `switch`, `cond-expr`, `default-arg`,
    /// `binary-expr`, etc.). Kept for downstream display only; not
    /// consumed by the line-range join.
    #[serde(rename = "type")]
    #[allow(dead_code)]
    kind: String,
    /// Optional emitter-set branching-site line. When present, takes
    /// precedence over `loc.start.line` for line attribution (some
    /// emitters set `loc` to a wide span covering both arms but pin
    /// `line` to the branching keyword's line).
    #[serde(default)]
    line: Option<u32>,
}

/// W2.3 fallback: function metadata. Declared minimal here for the
/// same reason as `BranchLoc`; `name` + `decl` + `loc` + `line` is the
/// minimum surface needed if `f`/`fnMap` later backfills arrow
/// coverage that `s`/`statementMap` undercounts.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FnLoc {
    name: String,
    decl: StatementLoc,
    loc: StatementLoc,
    #[serde(default)]
    line: Option<u32>,
}

impl IstanbulCoverage {
    /// Build a `ParseOutput` from an already-deserialized flat map of
    /// per-file Istanbul entries.
    ///
    /// Shared between the happy path (`parse_str` → flat) and the
    /// one-level unwrap path (`parse_str` → wrapped → inner flat
    /// map). Per-entry diagnostics (`PathUnresolved`, `MissingField`,
    /// `BranchMismatch`) are emitted into `diagnostics`; the function
    /// never aborts the whole parse and never silent-drops a record.
    fn build_parse_output(
        &self,
        raw: HashMap<String, IstanbulCoverageFile>,
    ) -> ParseOutput<IstanbulParseDiagnostic> {
        let mut coverage: HashMap<String, Vec<LineCoverage>> = HashMap::new();
        let mut branch_map_out: HashMap<String, Vec<BranchCoverage>> = HashMap::new();
        let mut diagnostics: Vec<IstanbulParseDiagnostic> = Vec::new();

        for (_key, entry) in raw {
            // ── Path normalization ──────────────────────────────────
            let Some(normalized) = self.normalize_path(&entry.path) else {
                diagnostics.push(IstanbulParseDiagnostic {
                    file_path: entry.path.clone(),
                    kind: IstanbulDiagnosticKind::PathUnresolved,
                    message: format!(
                        "path '{}' does not resolve to a discovered source file under {}",
                        entry.path,
                        self.effective_src.display()
                    ),
                    line: None,
                });
                continue;
            };

            // ── MissingField: `s` populated but `statementMap` empty,
            // or `statementMap` populated but `s` empty. Both cases
            // indicate a partial / corrupt emitter record; emit and
            // skip per breadboard W-3.
            if entry.s.is_empty() != entry.statement_map.is_empty() {
                let (present, missing) = if entry.s.is_empty() {
                    ("statementMap", "s")
                } else {
                    ("s", "statementMap")
                };
                diagnostics.push(IstanbulParseDiagnostic {
                    file_path: entry.path.clone(),
                    kind: IstanbulDiagnosticKind::MissingField,
                    message: format!(
                        "entry for '{}' has `{}` records but `{}` is empty; one half of the statement-coverage pair is missing",
                        entry.path, present, missing
                    ),
                    line: None,
                });
                continue;
            }

            // ── Line coverage (W1.1) ────────────────────────────────
            // Build per-line records by joining `s` (counts) to
            // `statementMap` (line spans). Statements whose IDs do not
            // appear in `statementMap` are skipped; statements that
            // span multiple lines emit one `LineCoverage` per line in
            // the span keyed by `start.line` (Istanbul records hits at
            // the start-of-statement granularity, per its emitter).
            let mut lines: Vec<LineCoverage> = Vec::with_capacity(entry.s.len());
            for (stmt_id, hits) in &entry.s {
                if let Some(loc) = entry.statement_map.get(stmt_id) {
                    lines.push(LineCoverage {
                        line: loc.start.line as usize,
                        hits: *hits,
                    });
                }
            }
            // Sort by line for deterministic downstream consumption
            // (mirrors LCOV parser ordering).
            lines.sort_by_key(|lc| lc.line);

            // ── Branch coverage (W2.3) ──────────────────────────────
            // For each branchId in `b`, look up its `branchMap` entry.
            // Missing branchMap entries emit `BranchMismatch` and
            // skip THAT branch only (rest of the file still parses).
            // Per the consumer contract at
            // `crap-core::domain::matching::compute_branch_coverage`,
            // each `BranchCoverage` row represents ONE branch arm —
            // total = N records, covered = records with taken > 0.
            // So we fan one Istanbul branchId with K arms into K
            // separate `BranchCoverage` rows at the branching site's
            // line, each carrying that arm's `taken` count. (Summing
            // arm hits into a single record would collapse partial
            // coverage into 100% — see PR body's plan-of-record
            // deviation note for the worked example.)
            let mut branches: Vec<BranchCoverage> = Vec::new();
            for (branch_id, arms) in &entry.b {
                let Some(loc) = entry.branch_map.get(branch_id) else {
                    diagnostics.push(IstanbulParseDiagnostic {
                        file_path: entry.path.clone(),
                        kind: IstanbulDiagnosticKind::BranchMismatch,
                        message: format!(
                            "branch coverage record for branchId `{branch_id}` references no entry in branchMap. This is likely a bug in your coverage tool — report at the coverage tool's issue tracker."
                        ),
                        line: None,
                    });
                    continue;
                };
                // Prefer the emitter-set `line` when present (some
                // emitters set it to the branching keyword's line
                // independent of `loc`); fall back to
                // `loc.start.line`.
                let line = loc.line.unwrap_or(loc.loc.start.line) as usize;
                for hits in arms {
                    branches.push(BranchCoverage {
                        line,
                        taken: Some(*hits),
                    });
                }
            }
            // Deterministic ordering for downstream consumers.
            branches.sort_by_key(|b| b.line);

            let key = normalized.to_string_lossy().into_owned();
            coverage.insert(key.clone(), lines);
            if !branches.is_empty() {
                branch_map_out.insert(key, branches);
            }
        }

        // `branches: None` ↔ "no branch data in this coverage file"
        // (semantic distinction from `Some({})`; mirrors the LCOV
        // adapter's `(!state.raw_branches.is_empty()).then(|| ...)`
        // gate). This regression-pins existing W1.1 fixtures which
        // have `"b": {}` everywhere.
        let branches = (!branch_map_out.is_empty()).then_some(branch_map_out);

        ParseOutput {
            coverage,
            branches,
            diagnostics,
        }
    }

    /// Try the one-level unwrap arm: `{"<single-key>": <flat-map>}`.
    /// Returns the unwrapped flat map when the top-level value is a
    /// single-key object whose value deserializes as Istanbul's flat
    /// shape; returns `None` otherwise so the caller can emit
    /// `SchemaUnrecognized`.
    fn try_unwrap(value: &Value) -> Option<HashMap<String, IstanbulCoverageFile>> {
        let obj = value.as_object()?;
        if obj.len() != 1 {
            return None;
        }
        // Take the single inner value and re-deserialize it as the
        // flat shape. We use `serde_json::from_value` here (cloning
        // is acceptable — the wrapped shape is rare and small
        // relative to the inner flat map's deserialization cost).
        let inner = obj.values().next()?;
        serde_json::from_value::<HashMap<String, IstanbulCoverageFile>>(inner.clone()).ok()
    }
}

impl CoveragePort for IstanbulCoverage {
    type Diagnostic = IstanbulParseDiagnostic;

    /// Parse a `coverage-final.json` payload into per-file
    /// `LineCoverage` + `BranchCoverage` records.
    ///
    /// **Top-level shape tolerance (W2.4)**:
    ///
    /// 1. Try the flat `{[path]: entry}` shape first (jest, vitest,
    ///    nyc baseline).
    /// 2. If that fails, parse as `serde_json::Value` and try a
    ///    single-level unwrap (`{"coverage-final": {...flat...}}`).
    /// 3. If neither matches, emit a `SchemaUnrecognized` diagnostic
    ///    (with detected top-level keys) and return an empty
    ///    `ParseOutput` carrying that diagnostic — the scorecard's
    ///    downstream "no functions extracted" path then surfaces a
    ///    non-zero exit. Never abort first-record.
    /// 4. If the input is not valid JSON at all,
    ///    `Err(CrapError::SourceParse("istanbul: ..."))` propagates
    ///    (fatal — no recovery path).
    ///
    /// Per-entry errors (`PathUnresolved`, `MissingField`,
    /// `BranchMismatch`) push an `IstanbulParseDiagnostic` and skip
    /// the entry / branch; other entries still parse cleanly.
    fn parse(&self, data: &str) -> Result<ParseOutput<IstanbulParseDiagnostic>, CrapError> {
        // Path 1: flat-shape fast path.
        if let Ok(raw) = serde_json::from_str::<HashMap<String, IstanbulCoverageFile>>(data) {
            return Ok(self.build_parse_output(raw));
        }

        // Path 2: re-parse as untyped `Value` for the unwrap arm and
        // for top-level-key detection. If JSON itself is malformed,
        // surface as fatal `SourceParse` per the W1.1 contract.
        let value: Value = serde_json::from_str(data)
            .map_err(|e| CrapError::SourceParse(format!("istanbul: {e}")))?;

        if let Some(raw) = Self::try_unwrap(&value) {
            return Ok(self.build_parse_output(raw));
        }

        // Path 3: schema unrecognized. Detect top-level keys (if the
        // value is an object) so the diagnostic message names what we
        // received vs what we expected. Use a single
        // `Ok(ParseOutput { …, diagnostics: [SchemaUnrecognized] })`
        // path so downstream "no functions extracted" produces the
        // non-zero exit and the diagnostic surfaces in the envelope.
        let detected_keys = match &value {
            Value::Object(map) => {
                let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
                keys.sort();
                if keys.is_empty() {
                    "{}".to_string()
                } else {
                    format!("[{}]", keys.join(", "))
                }
            }
            Value::Array(_) => "array".to_string(),
            Value::String(_) => "string".to_string(),
            Value::Number(_) => "number".to_string(),
            Value::Bool(_) => "bool".to_string(),
            Value::Null => "null".to_string(),
        };
        Ok(ParseOutput {
            coverage: HashMap::new(),
            branches: None,
            diagnostics: vec![IstanbulParseDiagnostic {
                file_path: String::new(),
                kind: IstanbulDiagnosticKind::SchemaUnrecognized,
                message: format!(
                    "top-level shape not recognized as Istanbul; expected `{{[path]: {{ path, s, statementMap, … }}}}`; received keys: {detected_keys}"
                ),
                line: None,
            }],
        })
    }

    /// Pre-flight structural check: parse the file as Istanbul JSON and
    /// require at least one entry with a non-empty `statementMap`.
    ///
    /// **Tolerance parity with `parse`** (W2.4): mirrors the
    /// flat-then-unwrap cascade in `parse` so the CLI's pre-flight
    /// gate doesn't reject wrapped fixtures before parse ever runs.
    /// The CLI calls `validate` via
    /// `crap-core::cli::check_coverage_has_data`; a strict pre-flight
    /// would short-circuit the parse cascade and the
    /// `SchemaUnrecognized` diagnostic surface for valid-but-wrapped
    /// emitters.
    ///
    /// Returns `Err("not a recognizable Istanbul JSON shape: …")` when
    /// neither the flat shape nor a single-level unwrap deserialize as
    /// `HashMap<String, IstanbulCoverageFile>` (drives the
    /// `schema-unrecognized` user-facing error). Returns
    /// `Err("no statement coverage records")` when every consumable
    /// entry has an empty `statementMap` (drives the "regenerate
    /// coverage" hint). CLI layer surfaces these via
    /// `AdapterMeta::coverage_hint`.
    fn validate(&self, path: &Path) -> Result<(), String> {
        let data = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;

        // Path 1: flat-shape fast path (jest / vitest / nyc baseline).
        let raw = match serde_json::from_str::<HashMap<String, IstanbulCoverageFile>>(&data) {
            Ok(raw) => raw,
            Err(flat_err) => {
                // Path 2: try the one-level unwrap before rejecting,
                // so a wrapped `{"coverage-final": {…flat…}}`
                // payload still gets through to the parse cascade.
                // We re-parse as `Value` once and pass it to the
                // same `try_unwrap` helper `parse` uses; this keeps
                // the validate / parse contracts symmetric.
                let value: Value = serde_json::from_str(&data).map_err(|json_err| {
                    format!("not a recognizable Istanbul JSON shape: {json_err}")
                })?;
                Self::try_unwrap(&value)
                    .ok_or_else(|| format!("not a recognizable Istanbul JSON shape: {flat_err}"))?
            }
        };

        if raw.values().all(|f| f.statement_map.is_empty()) {
            return Err("no statement coverage records".into());
        }
        Ok(())
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
    fn parse_empty_object_returns_ok_with_no_coverage() {
        let c = IstanbulCoverage::new(PathBuf::from("/tmp"));
        let out = c.parse("{}").expect("empty JSON object parses cleanly");
        assert!(out.coverage.is_empty());
        assert!(out.diagnostics.is_empty());
        assert!(out.branches.is_none());
    }

    #[test]
    fn parse_malformed_json_returns_source_parse_with_istanbul_prefix() {
        let c = IstanbulCoverage::new(PathBuf::from("/tmp"));
        let err = c.parse("{not json").unwrap_err();
        match err {
            CrapError::SourceParse(msg) => {
                assert!(msg.starts_with("istanbul: "), "msg: {msg}");
            }
            other => panic!("expected SourceParse, got {other:?}"),
        }
    }
}
