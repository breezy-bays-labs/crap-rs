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
//! ## Schema minimalism (only model what is consumed)
//!
//! The internal deserialization types model **only** the fields the
//! parser actually reads (`path`, `s`, `statementMap.start.line`, `b`,
//! `branchMap.loc.start.line`, `branchMap.line`). Fields a producer
//! emits but the parser never consults (`f`/`fnMap`, the `end` side of
//! every span, a branch's `type`) are deliberately **not** modelled —
//! serde drops unknown fields, so they are tolerated for free.
//!
//! This is not just tidiness: it shrinks the whole-file failure
//! surface. serde aborts the entire flat-shape parse the instant any
//! *modelled, required* field has the wrong type (e.g. `"type": null`,
//! `"name": null`, `"end": {}`), and that abort happens **before** any
//! per-entry diagnostic can fire — the file produces zero coverage and
//! zero diagnostics, the worst possible outcome. Empirically, jest,
//! `@vitest/coverage-istanbul@4`, nyc, and `c8 --reporter=json` all
//! emit concrete values for the fields we *do* model, but they vary
//! freely in the fields we don't (anonymous `fnMap` names, generic
//! branch `type`, `column: null`, empty `{}` position objects). Not
//! modelling the unconsumed fields means that variance — present or
//! future — can never bail the parse.
//!
//! - **`ParseOutput.branches` is internal** — `crap-core::core::analyze`
//!   lowers branch data into per-function `branch_coverage:
//!   Option<CoverageRatio>` records, but the JSON envelope only
//!   surfaces `coverage_percent` (= line coverage). Branch records
//!   never reach the wire — verified by `wire_envelope_crap4ts` (no
//!   `branchCoverage` field in any emitted row).
//!
//! ## Path normalization
//!
//! Two-arm strategy (see `IstanbulCoverage::normalize_path`): a pure
//! `strip_prefix(effective_src)` fast path for same-machine captures,
//! then a bounded `.is_file()` longest-suffix reachability fallback
//! (#215) for portable fixtures whose absolute paths were captured on
//! a different machine and share no prefix with the local
//! `effective_src`. The orchestrator pre-canonicalizes `effective_src`
//! at the factory-closure boundary. Entries whose paths resolve under
//! neither arm emit an `IstanbulParseDiagnostic { kind: PathUnresolved,
//! … }` and are skipped — per Resolved Q10 in shaping, the scorecard
//! NEVER aborts on first failure and NEVER silent-drops a record.

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

    /// Resolve `raw` to a workspace-relative path under
    /// `effective_src`, or `None` if it can't be resolved.
    ///
    /// Two-arm strategy:
    ///
    /// 1. **Strip-prefix fast path (pure, no I/O).** Join relative
    ///    entries against `effective_src`, then strip the canonical
    ///    `effective_src` prefix. This succeeds whenever the coverage
    ///    payload was captured on the same machine/tree being analyzed
    ///    (jest/vitest/nyc on the local checkout). The orchestrator
    ///    pre-canonicalizes `effective_src`, so no `canonicalize()`
    ///    walk is needed here.
    /// 2. **Suffix-reachability fallback (bounded `.is_file()` I/O,
    ///    #215).** When the coverage was captured on a *different*
    ///    machine (a portable fixture: coverage produced on machine A,
    ///    analyzed on machine B), the entry's absolute path shares no
    ///    prefix with the local `effective_src` and arm 1 returns
    ///    `None`. Arm 2 then walks the path's components longest →
    ///    shortest, joining each suffix under `effective_src` and
    ///    accepting the first suffix that resolves to a real file. The
    ///    longest matching suffix wins, which deterministically
    ///    disambiguates a leaf filename (e.g. `index.ts`) that exists
    ///    in multiple directories — the longer shared path is the
    ///    structural truth; the machine-specific absolute prefix is
    ///    noise. If two equal-length suffixes could both match (only
    ///    reachable via symlinks/odd trees), the first found wins.
    ///
    /// This is *not* a pure function and no longer mirrors
    /// `crap4rs::adapters::coverage::LcovParser::normalize_path` (which
    /// remains pure strip-prefix): the fallback does bounded,
    /// OS-cached filesystem I/O (~components × `.is_file()` syscalls).
    /// That trade buys cross-machine fixture portability. When even the
    /// suffix match fails, this returns `None` and the caller emits
    /// `PathUnresolved` as before — the diagnostic-and-skip contract
    /// (D16) is preserved as the final arm.
    ///
    /// **Traversal guard (authoritative, #216).** The relative path
    /// returned from *either* arm is rejected (`None`) if it contains a
    /// `Component::ParentDir` (`..`). This is the single, authoritative
    /// guard and covers both arms uniformly. It is *not* redundant with
    /// arm 2's per-iteration filter: arm 1's `strip_prefix` is lexical,
    /// so `effective_src = /root/project` and a user-supplied coverage
    /// path `/root/project/../outside/secret.ts` *succeeds* arm 1 with
    /// the relative result `../outside/secret.ts` — a path that
    /// `.is_file()`-resolves *outside* `effective_src`. Since
    /// `coverage-final.json` is user-supplied, the return-point guard
    /// closes this traversal-escape for the strip-prefix fast path too,
    /// not just the suffix fallback. (Gemini security-medium on #216,
    /// scope-expanded beyond the stated arm-2-only finding after a
    /// standalone `strip_prefix` repro proved arm 1 had the same
    /// defect.)
    fn normalize_path(&self, raw: &str) -> Option<PathBuf> {
        let path = PathBuf::from(raw);
        let joined = if path.is_absolute() {
            path
        } else {
            self.effective_src.join(raw)
        };
        // Arm 1: strip the canonical effective_src prefix (W2.4 fast
        // path — same-machine capture). Arm 2: cross-machine absolute
        // paths don't share a prefix — find the longest path suffix
        // that resolves to a real file under effective_src (#215).
        let candidate = if let Ok(stripped) = joined.strip_prefix(&self.effective_src) {
            stripped.to_path_buf()
        } else {
            self.suffix_match_under(&joined)?
        };
        // Authoritative traversal guard (#216): both `strip_prefix`
        // and `starts_with` are lexical, so a `..`-containing result
        // points outside `effective_src` once resolved. Reject any
        // ParentDir in the final relative path — covers both arms.
        if candidate
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        {
            return None;
        }
        Some(candidate)
    }

    /// Find the longest suffix of `raw` whose components, joined under
    /// `effective_src`, resolve to a real file. Returns the
    /// workspace-relative suffix, or `None` if no suffix is reachable.
    ///
    /// Iterates suffixes longest → shortest (`start` from 0 outward),
    /// so the first reachable suffix is the longest one
    /// (longest-suffix-wins precedence — see [`Self::normalize_path`]).
    ///
    /// `.is_file()` is used (not `.exists()`): one syscall, and it
    /// prevents a directory false-positive when a raw path component
    /// happens to match a directory name under `effective_src`.
    ///
    /// Two local guards narrow matches; the **authoritative**
    /// traversal guard lives at [`Self::normalize_path`]'s return
    /// point (it covers both arms uniformly — see there). These are
    /// defense-in-depth / local perf, not the sole protection:
    ///
    /// - **Per-iteration ParentDir (`..`) skip.** Any suffix
    ///   containing a `Component::ParentDir` is skipped *before* the
    ///   `.is_file()` syscall — a small perf win (avoids a doomed
    ///   filesystem touch) that also clarifies the local invariant.
    ///   Filtering is per-iteration, not an up-front whole-path
    ///   reject: a `..` only contaminates the suffixes that include
    ///   it; a later, cleaner suffix (e.g. `c/file.ts` from
    ///   `/a/b/../c/file.ts`) still resolves. `CurDir` (`.`) is
    ///   harmless and left alone. The authoritative rejection is still
    ///   `normalize_path`'s return-point guard (#216).
    /// - **`starts_with(effective_src)` guard.** Defense-in-depth for
    ///   the `start == 0` degenerate case: when `raw` is absolute,
    ///   `effective_src.join(raw)` collapses back to `raw` itself
    ///   (Rust `Path::join` replaces the base with an absolute RHS),
    ///   so an absolute path that exists verbatim *outside*
    ///   `effective_src` would otherwise leak an out-of-tree match.
    fn suffix_match_under(&self, raw: &Path) -> Option<PathBuf> {
        let components: Vec<_> = raw.components().collect();
        for start in 0..components.len() {
            let candidate_rel: PathBuf = components[start..].iter().collect();
            // Reject traversal: `Path::starts_with` is lexical, so a
            // `..` component would pass the under-root guard but
            // `.is_file()` resolves it outside `effective_src`. Skip
            // any suffix containing a ParentDir component before the
            // filesystem touch (#216 gemini security-medium).
            if candidate_rel
                .components()
                .any(|c| c == std::path::Component::ParentDir)
            {
                continue;
            }
            let candidate_abs = self.effective_src.join(&candidate_rel);
            if candidate_abs.starts_with(&self.effective_src) && candidate_abs.is_file() {
                return Some(candidate_rel);
            }
        }
        None
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
    /// `statement_id` → statement location. Only `start.line` is read
    /// (the line-coverage join key).
    #[serde(default)]
    statement_map: HashMap<String, StatementLoc>,
    /// `branch_id` → per-arm hit counts. Each branchId maps to a
    /// `Vec<u64>` of arm-level counts; the parser fans these into one
    /// `BranchCoverage` row per arm at the branching site's line.
    #[serde(default)]
    b: HashMap<String, Vec<u64>>,
    /// `branch_id` → branch location. See `b` above.
    #[serde(default)]
    branch_map: HashMap<String, BranchLoc>,
    // `f`/`fnMap` (function exec counts + metadata) are intentionally
    // not modelled: line coverage from `s`/`statementMap` is sufficient
    // for every function shape (including arrow functions), so the
    // parser never reads them. serde drops them as unknown fields.
    // Modelling `fnMap` would re-introduce a whole-file bail vector —
    // its `name` is `null` for anonymous functions in some producers.
}

/// A span location. Istanbul carries `{ start, end }`, but only
/// `start.line` is the line-coverage join key, so `end` (and the
/// column on either side) is not modelled — see the module-level
/// "Schema minimalism" note. serde ignores the unmodelled `end`.
#[derive(Debug, Deserialize)]
struct StatementLoc {
    start: Position,
}

/// The 1-based source line of a span's start — the only positional
/// datum the line-coverage join consumes. Istanbul also emits a
/// `column` here (and a whole `end` position), sometimes `null` or an
/// empty `{}` object depending on the producer; none of that is
/// modelled because none of it is read, so its variance can never
/// fail the parse.
#[derive(Debug, Deserialize)]
struct Position {
    line: u32,
}

/// Branch location. Only the branching-site line is consumed: the
/// emitter-set `line` when present (some emitters pin it to the
/// branching keyword's line while `loc` spans both arms), else
/// `loc.start.line`. Istanbul's branch `type` and `locations[]` are
/// not modelled — unread, and `type` is `null` / `locations[]` holds
/// empty `{}` objects in some producers' output.
#[derive(Debug, Deserialize)]
struct BranchLoc {
    loc: StatementLoc,
    /// Optional emitter-set branching-site line; takes precedence over
    /// `loc.start.line` when present.
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
