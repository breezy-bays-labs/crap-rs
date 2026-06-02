//! LCOV coverage parser adapter.
//!
//! Parses `cargo-llvm-cov --lcov` output into per-file, per-line hit data.
//! Uses SF (source file), DA (line data), and BRDA (branch data) records.
//! FN/FNDA records are ignored because function matching uses line ranges
//! from syn, not LCOV function names (which are mangled Rust symbols).

use crate::domain::types::{BranchCoverage, CrapError, LineCoverage};
use crate::parse_diagnostic::LcovParseDiagnostic;
use crap_core::ports::{CoveragePort, ParseOutput};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Branch key: (line, block, branch) for deduplication and merging.
type BranchKey = (usize, u32, u32);
/// Per-file line accumulator used during parsing before conversion to domain types.
type RawCoverage = HashMap<String, BTreeMap<usize, u64>>;
/// Per-file branch accumulator used during parsing before conversion to domain types.
type RawBranches = HashMap<String, BTreeMap<BranchKey, Option<u64>>>;

/// Parses LCOV format coverage data.
///
/// Uses a single-pass block accumulator: iterates lines once,
/// SF: starts a new block, DA: accumulates into a BTreeMap per block,
/// and blocks are flushed at the next SF: or end of input.
pub struct LcovParser {
    root_path: PathBuf,
}

#[derive(Default)]
struct ParseState {
    raw_coverage: RawCoverage,
    raw_branches: RawBranches,
    diagnostics: Vec<LcovParseDiagnostic>,
    current_path: Option<String>,
    current_lines: BTreeMap<usize, u64>,
    current_branches: BTreeMap<BranchKey, Option<u64>>,
}

impl LcovParser {
    pub fn new(root_path: PathBuf) -> Self {
        Self { root_path }
    }

    /// Reduce an `SF:` path to the source-root-relative key the walker
    /// uses to identify functions (`FunctionIdentity::file_path`).
    ///
    /// Three shapes reach this function, all of which must collapse to
    /// the same key the syn walker emits via `strip_prefix(options.src)`:
    ///
    /// 1. **Absolute, same-machine** (`SF:/ws/crates/foo/src/bar.rs`) —
    ///    the dominant `cargo llvm-cov` shape when `--src` is canonical.
    ///    The lexical `strip_prefix(root)` resolves it directly.
    /// 2. **Already src-relative** (`SF:bar.rs`) — strip is a no-op and
    ///    the lexical result already names the file under the root.
    /// 3. **Workspace-relative** (`SF:crates/foo/src/bar.rs`) — the
    ///    natural shape `cargo llvm-cov` emits when run from the
    ///    workspace root. The orchestrator hands this parser the
    ///    *canonical absolute* root, which a relative path cannot
    ///    lexically strip, so the lexical result keeps the full
    ///    workspace-relative path. The walker keys by the src-relative
    ///    basename, so the two never matched and coverage silently
    ///    dropped to 0. A filesystem-validated longest-suffix match
    ///    rescues this case.
    ///
    /// The suffix match does bounded `.is_file()` I/O — this parser is
    /// no longer pure strip-prefix; it now converges with crap4ts's
    /// `IstanbulCoverage::normalize_path`, which made the same trade for
    /// cross-form portability.
    fn normalize_path(&self, path: &str) -> String {
        let fwd = path.replace('\\', "/");
        let root_fwd = self.root_path.to_string_lossy().replace('\\', "/");
        let p = Path::new(&fwd);
        let root = Path::new(&root_fwd);
        let lexical = p.strip_prefix(root).unwrap_or(p);

        // Fast path: the lexical strip already names a real file under
        // the source root (shapes 1 and 2 above). Keep it verbatim — no
        // suffix scan needed.
        //
        // Traversal guard (mirrors the crap4ts adapter's guard, applied
        // in the suffix-match below too): `strip_prefix` is lexical, so a
        // user-supplied `SF:/root/../outside/secret.rs` strips to
        // `../outside/secret.rs` and `root.join(..).is_file()` would
        // resolve — and thus probe the existence of — a file *outside*
        // the source root. Reject any `..` here so a `..`-bearing path
        // falls through to the suffix match, which skips `..` suffixes
        // and re-anchors the clean tail under the root.
        let lexical_has_parentdir = lexical
            .components()
            .any(|c| c == std::path::Component::ParentDir);
        if !lexical_has_parentdir && root.join(lexical).is_file() {
            return lexical.to_string_lossy().replace('\\', "/");
        }

        // Rescue path: a workspace-relative `SF:` line couldn't strip
        // the canonical absolute root. Recover the walker's key as the
        // longest path suffix that resolves to a real file.
        if let Some(suffix) = suffix_match_under(root, p) {
            return suffix;
        }

        // Cross-machine fixtures and the parser's fake-path unit tests
        // have no on-disk file to anchor against; preserve the
        // historical lexical output so those keys (and their
        // diagnostics) stay byte-identical.
        lexical.to_string_lossy().replace('\\', "/")
    }
}

/// Find the longest suffix of `path` whose components, joined under
/// `root`, resolve to a real file. Returns the root-relative,
/// forward-slash-normalised suffix, or `None` if no suffix is reachable.
///
/// Iterates longest → shortest so a leaf filename that exists in
/// multiple directories is disambiguated by the longest shared path —
/// the structural truth, not the machine-specific prefix. Mirrors
/// `crap4ts`'s `IstanbulCoverage::suffix_match_under`.
///
/// `.is_file()` (one syscall, not `.exists()`) avoids a directory
/// false-positive. Any suffix containing a `..` component is skipped
/// before the syscall: `SF:` records are user-supplied, so a `..` that
/// lexically passes `starts_with(root)` would resolve a real file
/// *outside* `root` (mirrors the crap4ts adapter's traversal guard).
fn suffix_match_under(root: &Path, path: &Path) -> Option<String> {
    let components: Vec<_> = path.components().collect();
    for start in 0..components.len() {
        let candidate_rel: PathBuf = components[start..].iter().collect();
        if candidate_rel
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        {
            continue;
        }
        let candidate_abs = root.join(&candidate_rel);
        if candidate_abs.starts_with(root) && candidate_abs.is_file() {
            return Some(candidate_rel.to_string_lossy().replace('\\', "/"));
        }
    }
    None
}

impl LcovParser {
    /// Parse already-loaded LCOV text. Pure, no I/O — the public
    /// [`CoveragePort::parse`] impl slurps the file and delegates here
    /// so the parsing logic stays testable without writing every
    /// fixture to a tempfile.
    ///
    /// Kept `pub(crate)` so the in-tree test module can exercise the
    /// parser against string literals; external callers always go
    /// through the port.
    pub(crate) fn parse_str(&self, data: &str) -> ParseOutput<LcovParseDiagnostic> {
        let mut state = ParseState::default();

        for (line, line_number) in data.lines().zip(1usize..) {
            handle_parse_line(self, &mut state, line, line_number);
        }

        flush_block(&mut state);
        build_parse_output(state)
    }
}

impl CoveragePort for LcovParser {
    type Diagnostic = LcovParseDiagnostic;

    /// Slurp the LCOV file at `path` and parse it.
    ///
    /// **Slurp choice (vs streaming via `BufReader`)**: deliberate, but
    /// only because the memory bound doesn't matter yet — not because
    /// streaming would be costly or awkward. A streaming rewrite is
    /// viable and clean: `read_line(&mut buf)` + `buf.clear()` reuses a
    /// single buffer across all lines (zero per-line allocation), and
    /// the single-pass block accumulator wouldn't change at all —
    /// `ParseState` already owns everything it keeps (`BTreeMap`, owned
    /// path `String`s, the diagnostic `Vec`); each line `&str` is
    /// borrowed only within one `handle_parse_line` call, never held
    /// across reads. So streaming *would* drop the file-size term from
    /// peak RSS, bounding it at roughly the parse-tree size.
    ///
    /// The reason to keep slurp is simply that peak RSS is trivial at
    /// realistic LCOV sizes. Benchmarking `cargo-llvm-cov --lcov` output
    /// (workspace coverage — line-oriented text, no expansion): a 45 MB
    /// / 5 M-line file parses at ~58 MB peak RSS (file size + ~13 MB
    /// parse tree) sub-second; 90 MB / 10 M lines at ~106 MB. Real files
    /// — including large monorepos — sit in the low tens of MB. Slurp's
    /// extra peak RSS is the file-size term, which is negligible there.
    /// The multi-GB-file trigger that would make the file-size term
    /// worth eliminating has not fired; until it does, the streaming
    /// refactor is tracked separately rather than built speculatively
    /// (the abstraction would take `impl BufRead` so production streams
    /// and tests slice strings via `.as_bytes()`).
    ///
    /// [`Self::validate`] streams via `BufReader` for a different
    /// reason: it short-circuits on the first match, so the per-line
    /// cost is dominated by the I/O it skips, and slurping there would
    /// double peak RSS against the `parse` read that follows.
    fn parse(&self, path: &Path) -> Result<ParseOutput<LcovParseDiagnostic>, CrapError> {
        let data = std::fs::read_to_string(path).map_err(CrapError::Io)?;
        Ok(self.parse_str(&data))
    }

    /// LCOV-flavoured pre-flight: stream the file line-by-line via
    /// `BufReader` and short-circuit on the first well-formed
    /// `DA:line,hits` inside an `SF:` block. Mirrors the structural
    /// shape that `parse` consumes; orphan `DA:` records outside an
    /// `SF:` block and malformed line/hit pairs are rejected.
    ///
    /// Streams (rather than slurping) because this gate short-circuits
    /// on the first well-formed record — it almost never reads the whole
    /// file — and `parse` will read the file again shortly after.
    /// Slurping here would hold the full file in memory only to discard
    /// it after the first match, and the early-exit means the per-line
    /// allocation cost is dominated by the I/O the gate skips.
    fn validate(&self, path: &Path) -> Result<(), String> {
        use std::io::{BufRead, BufReader};
        let file = std::fs::File::open(path)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let reader = BufReader::new(file);
        let mut in_sf_block = false;
        for line in reader.lines() {
            let line = line.map_err(|e| format!("read error: {e}"))?;
            if accepts_preflight_line(&line, &mut in_sf_block) {
                return Ok(());
            }
        }
        Err("no SF/DA records".to_string())
    }
}

/// Classify one pre-flight line. Returns `true` only when the line is
/// the first well-formed `DA:` record inside an `SF:` block — the
/// signal that the file carries usable coverage. An `SF:` line opens a
/// block (mutating `in_sf_block`) but is never itself an accept. The
/// gate short-circuits on the first `true`, so this never needs to read
/// past the first usable record.
fn accepts_preflight_line(line: &str, in_sf_block: &mut bool) -> bool {
    if line.starts_with("SF:") {
        *in_sf_block = true;
        return false;
    }
    *in_sf_block && is_well_formed_da(line)
}

/// True when `line` is a well-formed `DA:line,hits` record: a `DA:`
/// prefix, a comma-split into a parseable line number and a parseable
/// (checksum-tolerant) hit count. The pre-flight gate short-circuits on
/// the first match, so this only needs to recognize structural
/// well-formedness, not the full parse `parse_da` performs.
fn is_well_formed_da(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("DA:") else {
        return false;
    };
    let Some((line_no, hits)) = rest.split_once(',') else {
        return false;
    };
    line_no.parse::<usize>().is_ok() && hits.split(',').next().unwrap_or("").parse::<u64>().is_ok()
}

fn handle_parse_line(parser: &LcovParser, state: &mut ParseState, line: &str, line_number: usize) {
    if let Some(path) = line.strip_prefix("SF:") {
        start_source_file(parser, state, path, line_number);
        return;
    }

    if let Some(da_rest) = line.strip_prefix("DA:") {
        record_da(state, da_rest, line, line_number);
        return;
    }

    if let Some(brda_rest) = line.strip_prefix("BRDA:") {
        record_brda(state, brda_rest, line, line_number);
    }
}

fn start_source_file(parser: &LcovParser, state: &mut ParseState, path: &str, line_number: usize) {
    flush_block(state);

    if path.is_empty() {
        state
            .diagnostics
            .push(LcovParseDiagnostic::EmptySourceFile { line_number });
        state.current_path = None;
    } else {
        state.current_path = Some(parser.normalize_path(path));
    }
}

fn record_da(state: &mut ParseState, da_rest: &str, line: &str, line_number: usize) {
    if state.current_path.is_none() {
        return;
    }

    match parse_da(da_rest) {
        Ok((line_no, hits)) => merge_hits(&mut state.current_lines, line_no, hits),
        Err(_) => push_malformed_record(state, line_number, line),
    }
}

fn record_brda(state: &mut ParseState, brda_rest: &str, line: &str, line_number: usize) {
    if state.current_path.is_none() {
        return;
    }

    match parse_brda(brda_rest) {
        Ok((line_no, block, branch, taken)) => {
            merge_branch_value(&mut state.current_branches, (line_no, block, branch), taken);
        }
        Err(_) => push_malformed_record(state, line_number, line),
    }
}

fn push_malformed_record(state: &mut ParseState, line_number: usize, line: &str) {
    state
        .diagnostics
        .push(LcovParseDiagnostic::MalformedRecord {
            line_number,
            content: line.to_string(),
        });
}

/// Parse a DA record value (after "DA:" prefix).
/// Line 0 is treated as malformed (LCOV is 1-based).
fn parse_da(da: &str) -> Result<(usize, u64), ()> {
    let (line_str, rest) = da.split_once(',').ok_or(())?;
    // DA format: line,hits[,checksum] — ignore optional checksum field
    let hits_str = rest.split(',').next().ok_or(())?;
    let line_no = parse_lcov_line_no(line_str)?;
    let hits: u64 = hits_str.parse().map_err(|_| ())?;
    Ok((line_no, hits))
}

/// Parse a BRDA record value (after "BRDA:" prefix).
/// Format: line,block,branch,taken where taken is "-" or a non-negative integer.
/// Line 0 is treated as malformed (LCOV is 1-based).
fn parse_brda(brda: &str) -> Result<(usize, u32, u32, Option<u64>), ()> {
    let (line_str, block_str, branch_str, taken_str) = split_brda_fields(brda)?;

    let line_no = parse_lcov_line_no(line_str)?;
    let block: u32 = block_str.parse().map_err(|_| ())?;
    let branch: u32 = branch_str.parse().map_err(|_| ())?;
    let taken = parse_brda_taken(taken_str)?;

    Ok((line_no, block, branch, taken))
}

/// Split a BRDA value into its four comma-separated fields
/// (`line,block,branch,taken`). Fewer than four fields is malformed.
fn split_brda_fields(brda: &str) -> Result<(&str, &str, &str, &str), ()> {
    let mut parts = brda.splitn(4, ',');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(l), Some(bl), Some(br), Some(t)) => Ok((l, bl, br, t)),
        _ => Err(()),
    }
}

/// Parse a 1-based LCOV line number. Line 0 is rejected — LCOV line
/// numbers are 1-based, so a 0 signals a malformed record.
fn parse_lcov_line_no(s: &str) -> Result<usize, ()> {
    let line_no: usize = s.parse().map_err(|_| ())?;
    if line_no == 0 { Err(()) } else { Ok(line_no) }
}

/// Parse the `taken` field of a BRDA record: `"-"` means the branch was
/// never reached (`None`); any other value must be a non-negative
/// integer hit count (`Some`).
fn parse_brda_taken(taken_str: &str) -> Result<Option<u64>, ()> {
    if taken_str == "-" {
        Ok(None)
    } else {
        taken_str.parse::<u64>().map(Some).map_err(|_| ())
    }
}

fn flush_block(state: &mut ParseState) {
    let Some(path) = state.current_path.as_deref() else {
        clear_current_block(state);
        return;
    };

    merge_line_block(&mut state.raw_coverage, path, &state.current_lines);
    merge_branch_block(&mut state.raw_branches, path, &state.current_branches);
    clear_current_block(state);
}

fn merge_line_block(
    raw_coverage: &mut RawCoverage,
    path: &str,
    current_lines: &BTreeMap<usize, u64>,
) {
    if current_lines.is_empty() {
        return;
    }

    let existing = raw_coverage.entry(path.to_owned()).or_default();
    for (&line, &hits) in current_lines {
        merge_hits(existing, line, hits);
    }
}

fn merge_branch_block(
    raw_branches: &mut RawBranches,
    path: &str,
    current_branches: &BTreeMap<BranchKey, Option<u64>>,
) {
    if current_branches.is_empty() {
        return;
    }

    let file_branches = raw_branches.entry(path.to_owned()).or_default();
    for (&key, &taken) in current_branches {
        merge_branch_value(file_branches, key, taken);
    }
}

fn merge_hits(lines: &mut BTreeMap<usize, u64>, line_no: usize, hits: u64) {
    lines
        .entry(line_no)
        .and_modify(|existing| *existing = existing.saturating_add(hits))
        .or_insert(hits);
}

fn merge_branch_value(
    branches: &mut BTreeMap<BranchKey, Option<u64>>,
    key: BranchKey,
    taken: Option<u64>,
) {
    branches
        .entry(key)
        .and_modify(|existing| *existing = merge_taken(*existing, taken))
        .or_insert(taken);
}

fn merge_taken(existing: Option<u64>, taken: Option<u64>) -> Option<u64> {
    match (existing, taken) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn clear_current_block(state: &mut ParseState) {
    state.current_lines.clear();
    state.current_branches.clear();
}

fn build_parse_output(state: ParseState) -> ParseOutput<LcovParseDiagnostic> {
    let coverage = state
        .raw_coverage
        .into_iter()
        .map(|(file, entries)| (file, to_line_coverage(entries)))
        .collect();

    let branches = (!state.raw_branches.is_empty()).then(|| {
        state
            .raw_branches
            .into_iter()
            .map(|(file, entries)| {
                let vec = entries
                    .into_iter()
                    .map(|((line, _, _), taken)| BranchCoverage { line, taken })
                    .collect();
                (file, vec)
            })
            .collect()
    });

    ParseOutput {
        coverage,
        branches,
        diagnostics: state.diagnostics,
    }
}

fn to_line_coverage(entries: BTreeMap<usize, u64>) -> Vec<LineCoverage> {
    entries
        .into_iter()
        .map(|(line, hits)| LineCoverage { line, hits })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parser() -> LcovParser {
        LcovParser::new(PathBuf::from("/project"))
    }

    fn parse(input: &str) -> ParseOutput<LcovParseDiagnostic> {
        parser().parse_str(input)
    }

    #[test]
    fn empty_input_returns_empty_output() {
        let output = parse("");
        assert!(output.coverage.is_empty());
        assert!(output.diagnostics.is_empty());
    }

    // ── validate (preflight) ──────────────────────────────────────────

    /// Materialise `contents` into a tempfile and run `validate` against
    /// its path. The adapter streams the file via `BufReader` so we
    /// must hand it a real on-disk path; using `&str` directly would
    /// skip the streaming behaviour Gemini flagged as the regression
    /// trigger.
    fn validate_str(contents: &str) -> Result<(), String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preflight.info");
        std::fs::write(&path, contents).unwrap();
        parser().validate(&path)
    }

    #[test]
    fn validate_empty_input_rejected() {
        assert!(validate_str("").is_err());
    }

    #[test]
    fn validate_no_da_lines_rejected() {
        assert!(validate_str("SF:src/main.rs\nend_of_record\n").is_err());
    }

    #[test]
    fn validate_da_outside_sf_block_rejected() {
        assert!(validate_str("DA:1,5\nend_of_record\n").is_err());
    }

    #[test]
    fn validate_malformed_da_rejected() {
        assert!(validate_str("SF:src/main.rs\nDA:not_a_number\nend_of_record\n").is_err());
    }

    #[test]
    fn validate_single_da_inside_sf_block_passes() {
        assert!(validate_str("SF:src/main.rs\nDA:1,5\nend_of_record\n").is_ok());
    }

    #[test]
    fn validate_first_da_in_second_block_passes() {
        // First SF: has no DA — second SF: provides one. validate
        // walks both blocks and accepts the first valid DA encountered.
        assert!(
            validate_str("SF:src/a.rs\nend_of_record\nSF:src/b.rs\nDA:1,1\nend_of_record\n")
                .is_ok()
        );
    }

    #[test]
    fn validate_missing_path_returns_open_error() {
        // Adapter owns I/O — file-not-found surfaces as the structured
        // `Err(String)` from `validate`, not as an `io::Error`
        // propagated from a separate slurp step. Lets the CLI compose
        // it with the hint via the same path as "no records".
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.info");
        let err = parser().validate(&missing).unwrap_err();
        assert!(err.contains("cannot open"), "got: {err}");
    }

    #[test]
    fn single_file_single_da() {
        let output = parse("SF:/project/src/main.rs\nDA:1,5\nend_of_record\n");
        assert_eq!(output.coverage.len(), 1);
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line, 1);
        assert_eq!(lines[0].hits, 5);
    }

    #[test]
    fn multiple_da_lines() {
        let output = parse("SF:/project/src/lib.rs\nDA:1,3\nDA:2,0\nDA:3,7\nend_of_record\n");
        let lines = &output.coverage["src/lib.rs"];
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].line, lines[0].hits), (1, 3));
        assert_eq!((lines[1].line, lines[1].hits), (2, 0));
        assert_eq!((lines[2].line, lines[2].hits), (3, 7));
    }

    #[test]
    fn multiple_source_files() {
        let input = "SF:/project/src/a.rs\nDA:1,1\nend_of_record\nSF:/project/src/b.rs\nDA:2,2\nend_of_record\n";
        let output = parse(input);
        assert_eq!(output.coverage.len(), 2);
        assert_eq!(output.coverage["src/a.rs"][0].hits, 1);
        assert_eq!(output.coverage["src/b.rs"][0].hits, 2);
    }

    #[test]
    fn hit_count_preservation() {
        let output = parse("SF:/project/src/main.rs\nDA:42,7\nDA:10,0\nend_of_record\n");
        let lines = &output.coverage["src/main.rs"];
        assert_eq!((lines[0].line, lines[0].hits), (10, 0));
        assert_eq!((lines[1].line, lines[1].hits), (42, 7));
    }

    #[test]
    fn path_normalization_strips_prefix() {
        let output = parse("SF:/project/src/main.rs\nDA:1,1\nend_of_record\n");
        assert!(output.coverage.contains_key("src/main.rs"));
    }

    #[test]
    fn non_matching_path_passes_through() {
        let output = parse("SF:/other/lib.rs\nDA:1,1\nend_of_record\n");
        assert!(output.coverage.contains_key("/other/lib.rs"));
    }

    #[test]
    fn backslash_normalized_to_forward_slash() {
        let p = LcovParser::new(PathBuf::from("C:\\project"));
        let output = p.parse_str("SF:C:\\project\\src\\main.rs\nDA:1,1\n");
        assert!(output.coverage.contains_key("src/main.rs"));
    }

    #[test]
    fn path_boundary_not_confused_by_prefix_substring() {
        let output = parse("SF:/project-old/src/main.rs\nDA:1,1\nend_of_record\n");
        assert!(output.coverage.contains_key("/project-old/src/main.rs"));
    }

    #[test]
    fn parentdir_sf_path_does_not_take_fast_path_is_file_probe() {
        // `SF:` records are user-supplied; a `..` makes the lexical
        // `strip_prefix` succeed with a `..`-bearing relative result
        // that `root.join(..).is_file()` resolves OUTSIDE the root — a
        // traversal-escape existence oracle on the fast path. The fast
        // path must reject `..` and fall through to the guarded suffix
        // match, which re-anchors the clean tail under the root. Needs
        // real on-disk files so the `is_file()` behaviour is actually
        // exercised.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().canonicalize().expect("canonicalize tempdir");
        std::fs::write(root.join("secret.rs"), "fn x() {}").expect("write secret.rs");
        let leaf = root.file_name().unwrap().to_string_lossy().into_owned();
        let parser = LcovParser::new(root.clone());

        // `<root>/../<leaf>/secret.rs` lexically strips to
        // `../<leaf>/secret.rs`; an unguarded fast path would resolve
        // that and return the unusable `..` key.
        let sf = format!("{}/../{leaf}/secret.rs", root.display());
        let output = parser.parse_str(&format!("SF:{sf}\nDA:1,1\nend_of_record\n"));

        assert!(
            output.coverage.keys().all(|k| !k.contains("..")),
            "no coverage key may contain `..`: {:?}",
            output.coverage.keys().collect::<Vec<_>>()
        );
        assert!(
            output.coverage.contains_key("secret.rs"),
            "the `..` path must re-anchor to the in-tree `secret.rs` via the \
             guarded suffix match; keys = {:?}",
            output.coverage.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn repeated_sf_blocks_merge_coverage() {
        let input = "\
SF:/project/src/main.rs\nDA:1,3\nDA:2,5\nend_of_record\n\
SF:/project/src/main.rs\nDA:2,2\nDA:3,7\nend_of_record\n";
        let output = parse(input);
        assert_eq!(output.coverage.len(), 1);
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].line, lines[0].hits), (1, 3));
        assert_eq!((lines[1].line, lines[1].hits), (2, 7)); // 5 + 2
        assert_eq!((lines[2].line, lines[2].hits), (3, 7));
    }

    #[test]
    fn duplicate_da_lines_sum_hits() {
        let output = parse("SF:/project/src/main.rs\nDA:42,3\nDA:42,1\nend_of_record\n");
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line, 42);
        assert_eq!(lines[0].hits, 4);
    }

    #[test]
    fn duplicate_da_multiple_lines_sum_correctly() {
        let output = parse(
            "SF:/project/src/main.rs\nDA:10,2\nDA:20,3\nDA:30,1\nDA:10,5\nDA:20,7\nDA:30,4\nend_of_record\n",
        );
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].line, lines[0].hits), (10, 7));
        assert_eq!((lines[1].line, lines[1].hits), (20, 10));
        assert_eq!((lines[2].line, lines[2].hits), (30, 5));
    }

    #[test]
    fn malformed_da_produces_diagnostic() {
        let output = parse("SF:/project/src/main.rs\nDA:not_a_number\nend_of_record\n");
        assert!(!output.coverage.contains_key("src/main.rs"));
        assert_eq!(output.diagnostics.len(), 1);
        match &output.diagnostics[0] {
            LcovParseDiagnostic::MalformedRecord {
                line_number,
                content,
            } => {
                assert_eq!(*line_number, 2);
                assert_eq!(content, "DA:not_a_number");
            }
            _ => panic!("Expected MalformedRecord"),
        }
    }

    #[test]
    fn malformed_da_does_not_stop_parsing() {
        let output = parse("SF:/project/src/main.rs\nDA:1,5\nDA:bad\nDA:3,7\nend_of_record\n");
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 2);
        assert_eq!(output.diagnostics.len(), 1);
    }

    #[test]
    fn da_missing_hit_count_is_malformed() {
        let output = parse("SF:/project/src/main.rs\nDA:42\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            LcovParseDiagnostic::MalformedRecord { .. }
        ));
    }

    #[test]
    fn da_negative_hit_count_is_malformed() {
        let output = parse("SF:/project/src/main.rs\nDA:42,-1\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            LcovParseDiagnostic::MalformedRecord { .. }
        ));
    }

    #[test]
    fn da_line_zero_is_malformed() {
        let output = parse("SF:/project/src/main.rs\nDA:0,5\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        match &output.diagnostics[0] {
            LcovParseDiagnostic::MalformedRecord {
                line_number,
                content,
            } => {
                assert_eq!(*line_number, 2);
                assert_eq!(content, "DA:0,5");
            }
            _ => panic!("Expected MalformedRecord for line 0"),
        }
    }

    #[test]
    fn end_of_record_is_ignored() {
        let with_eor = parse("SF:/project/src/main.rs\nDA:1,5\nend_of_record\n");
        let without_eor = parse("SF:/project/src/main.rs\nDA:1,5\n");
        assert_eq!(with_eor.coverage.len(), without_eor.coverage.len());
        assert_eq!(
            with_eor.coverage["src/main.rs"][0].hits,
            without_eor.coverage["src/main.rs"][0].hits
        );
    }

    #[test]
    fn unterminated_final_block_emits_data() {
        let output = parse("SF:/project/src/main.rs\nDA:1,5\nDA:2,3");
        assert_eq!(output.coverage.len(), 1);
        assert_eq!(output.coverage["src/main.rs"].len(), 2);
    }

    #[test]
    fn empty_sf_path_emits_diagnostic() {
        let output = parse("SF:\nDA:1,5\nend_of_record\n");
        assert!(output.coverage.is_empty());
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            LcovParseDiagnostic::EmptySourceFile { line_number: 1 }
        ));
    }

    #[test]
    fn da_with_checksum_field_is_accepted() {
        let output = parse("SF:/project/src/main.rs\nDA:5,3,abc123\nend_of_record\n");
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 1);
        assert_eq!((lines[0].line, lines[0].hits), (5, 3));
        assert!(output.diagnostics.is_empty());
    }

    #[test]
    fn non_coverage_records_are_ignored() {
        let output = parse(
            "TN:\nSF:/project/src/main.rs\nFN:1,main\nFNDA:5,main\nBRDA:1,0,0,1\nDA:1,5\nLF:1\nLH:1\nend_of_record\n",
        );
        assert_eq!(output.coverage.len(), 1);
        assert_eq!(output.coverage["src/main.rs"].len(), 1);
        assert!(output.diagnostics.is_empty());
        // BRDA record should now be parsed
        let branches = output.branches.as_ref().expect("branches should be Some");
        assert_eq!(branches["src/main.rs"].len(), 1);
        assert_eq!(branches["src/main.rs"][0].taken, Some(1));
    }

    // ── BRDA parsing tests ──────────────────────────────────────────

    #[test]
    fn brda_and_da_records_parsed() {
        let output =
            parse("SF:/project/src/lib.rs\nDA:1,5\nBRDA:1,0,0,3\nBRDA:1,0,1,0\nend_of_record\n");
        assert_eq!(output.coverage["src/lib.rs"].len(), 1);
        let branches = output.branches.as_ref().expect("branches should be Some");
        let file_branches = &branches["src/lib.rs"];
        assert_eq!(file_branches.len(), 2);
    }

    #[test]
    fn brda_dash_maps_to_none() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0,-\nend_of_record\n");
        let branches = output.branches.as_ref().expect("branches should be Some");
        assert_eq!(branches["src/lib.rs"][0].taken, None);
    }

    #[test]
    fn brda_numeric_maps_to_some() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0,5\nend_of_record\n");
        let branches = output.branches.as_ref().expect("branches should be Some");
        assert_eq!(branches["src/lib.rs"][0].taken, Some(5));
    }

    #[test]
    fn brda_zero_maps_to_some_zero() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0,0\nend_of_record\n");
        let branches = output.branches.as_ref().expect("branches should be Some");
        assert_eq!(branches["src/lib.rs"][0].taken, Some(0));
    }

    #[test]
    fn duplicate_brda_sum_taken() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0,3\nBRDA:10,0,0,7\nend_of_record\n");
        let branches = output.branches.as_ref().expect("branches should be Some");
        let file_branches = &branches["src/lib.rs"];
        // Duplicates merged by summing: 3 + 7 = 10
        assert_eq!(file_branches.len(), 1);
        assert_eq!(file_branches[0].taken, Some(10));
    }

    #[test]
    fn brda_dash_and_numeric_merge() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0,-\nBRDA:10,0,0,5\nend_of_record\n");
        let branches = output.branches.as_ref().expect("branches should be Some");
        // None + Some(5) = Some(5)
        assert_eq!(branches["src/lib.rs"][0].taken, Some(5));
    }

    #[test]
    fn malformed_brda_produces_diagnostic() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:not,valid\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        match &output.diagnostics[0] {
            LcovParseDiagnostic::MalformedRecord {
                line_number,
                content,
            } => {
                assert_eq!(*line_number, 2);
                assert_eq!(content, "BRDA:not,valid");
            }
            _ => panic!("Expected MalformedRecord"),
        }
    }

    #[test]
    fn brda_missing_fields_is_malformed() {
        // Only 2 fields instead of 4
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            LcovParseDiagnostic::MalformedRecord { .. }
        ));
    }

    #[test]
    fn brda_missing_taken_is_malformed() {
        // Only 3 fields instead of 4
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            LcovParseDiagnostic::MalformedRecord { .. }
        ));
    }

    #[test]
    fn repeated_sf_blocks_merge_branch_coverage() {
        let input = "\
SF:/project/src/main.rs\nBRDA:1,0,0,3\nBRDA:1,0,1,2\nend_of_record\n\
SF:/project/src/main.rs\nBRDA:1,0,0,7\nBRDA:2,0,0,1\nend_of_record\n";
        let output = parse(input);
        let branches = output.branches.as_ref().expect("branches should be Some");
        let file_branches = &branches["src/main.rs"];
        // BRDA:1,0,0 appears in both blocks: 3 + 7 = 10
        // BRDA:1,0,1 appears only in first: 2
        // BRDA:2,0,0 appears only in second: 1
        assert_eq!(file_branches.len(), 3);
        let by_line: std::collections::HashMap<usize, Vec<Option<u64>>> = {
            let mut m: std::collections::HashMap<usize, Vec<Option<u64>>> =
                std::collections::HashMap::new();
            for b in file_branches {
                m.entry(b.line).or_default().push(b.taken);
            }
            m
        };
        // Line 1 has two branches: taken=10 (merged) and taken=2
        let line1 = by_line.get(&1).unwrap();
        assert_eq!(line1.len(), 2);
        assert!(line1.contains(&Some(10)));
        assert!(line1.contains(&Some(2)));
        // Line 2 has one branch: taken=1
        assert_eq!(by_line[&2], vec![Some(1)]);
    }

    #[test]
    fn brda_line_zero_is_malformed() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:0,0,0,5\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            LcovParseDiagnostic::MalformedRecord { .. }
        ));
    }

    #[test]
    fn no_brda_produces_none_branches() {
        let output = parse("SF:/project/src/lib.rs\nDA:1,5\nend_of_record\n");
        assert!(output.branches.is_none());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_da_line() -> impl Strategy<Value = String> {
        (1..10000usize, 0..1000u64).prop_map(|(line, hits)| format!("DA:{},{}", line, hits))
    }

    fn arb_lcov_block(prefix: &'static str) -> impl Strategy<Value = String> {
        prop::collection::vec(arb_da_line(), 0..10).prop_map(move |das| {
            let mut block = format!("SF:/project/src/{}.rs\n", prefix);
            for da in das {
                block.push_str(&da);
                block.push('\n');
            }
            block.push_str("end_of_record\n");
            block
        })
    }

    fn arb_lcov_input() -> impl Strategy<Value = String> {
        (arb_lcov_block("a"), arb_lcov_block("b")).prop_map(|(a, b)| format!("{}{}", a, b))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn no_panic_on_structured_input(input in arb_lcov_input()) {
            let parser = LcovParser::new(PathBuf::from("/project"));
            let _ = parser.parse_str(&input);
        }

        #[test]
        fn no_panic_on_arbitrary_input(input in ".*") {
            let parser = LcovParser::new(PathBuf::from("/project"));
            let _ = parser.parse_str(&input);
        }

        #[test]
        fn all_line_numbers_are_positive(input in arb_lcov_input()) {
            let parser = LcovParser::new(PathBuf::from("/project"));
            let output = parser.parse_str(&input);
            for lines in output.coverage.values() {
                for lc in lines {
                    prop_assert!(lc.line > 0);
                }
            }
        }

        #[test]
        fn no_panic_on_arbitrary_brda(input in "BRDA:.*") {
            let lcov = format!("SF:/project/src/test.rs\n{input}\nend_of_record\n");
            let parser = LcovParser::new(PathBuf::from("/project"));
            let _ = parser.parse_str(&lcov);
        }

        #[test]
        fn no_cross_file_leakage(
            a_lines in prop::collection::vec((1..100usize, 0..10u64), 1..5),
            b_lines in prop::collection::vec((200..300usize, 0..10u64), 1..5),
        ) {
            let mut input = String::from("SF:/project/src/a.rs\n");
            for (line, hits) in &a_lines {
                input.push_str(&format!("DA:{},{}\n", line, hits));
            }
            input.push_str("end_of_record\nSF:/project/src/b.rs\n");
            for (line, hits) in &b_lines {
                input.push_str(&format!("DA:{},{}\n", line, hits));
            }
            input.push_str("end_of_record\n");

            let parser = LcovParser::new(PathBuf::from("/project"));
            let output = parser.parse_str(&input);

            if let Some(a_coverage) = output.coverage.get("src/a.rs") {
                for lc in a_coverage {
                    prop_assert!(lc.line < 200, "File A leaked line {} from file B range", lc.line);
                }
            }
            if let Some(b_coverage) = output.coverage.get("src/b.rs") {
                for lc in b_coverage {
                    prop_assert!(lc.line >= 200, "File B leaked line {} from file A range", lc.line);
                }
            }
        }
    }
}
