//! GitHub Actions inline annotations reporter.
//!
//! Emits `::warning` workflow-command lines so CRAP findings render
//! inline on the PR "Files Changed" tab — universal, free, no GHAS /
//! Code Scanning dependency. The Actions runner intercepts the
//! `::workflow-command file=…,line=…,title=…::message` shape and
//! renders an inline annotation at the named line.
//!
//! Like the SARIF reporter, this is a *gate translation*, not a
//! display: results derive from `view.full.functions.iter().filter(|v|
//! v.exceeds)` so PR annotations reflect the unshapeable gate.
//! `--top`, `--sort-by`, `--only-failing`, and other view-shaping flags
//! do NOT alter what is emitted — the reporter sorts by CRAP DESC
//! itself, then truncates at `annotation_limit`.
//!
//! GitHub Actions silently drops annotations past a per-step cap (10
//! warning + 10 error + 10 notice per step; 50 per job; 50 per
//! workflow). The configurable `annotation_limit` plus a trailing
//! `::notice::N more functions exceed threshold` summary are the user-
//! visible mitigation; the runner cap is the underlying constraint.
//!
//! Spec: <https://docs.github.com/en/actions/using-workflows/workflow-commands-for-github-actions>

use std::path::Path;

use crate::domain::view::AnalysisView;

/// Format an `AnalysisView` as a stream of GitHub Actions workflow-
/// command lines.
///
/// One `::warning` line per `FunctionVerdict` whose `exceeds == true`,
/// sorted CRAP DESC. When the eligible set exceeds `annotation_limit`,
/// the top-N are emitted and a single trailing `::notice::N more
/// functions exceed threshold; see scorecard for the full list` line
/// is appended so reviewers know findings were dropped.
///
/// `tool_name` and `tool_version` are accepted for parity with the
/// SARIF reporter signature (the adapter binary threads them via
/// `AdapterMeta`); they are not currently embedded in the emitted
/// lines because workflow commands have no driver/version slot in
/// their wire shape.
pub fn format_github_annotations(
    view: &AnalysisView<'_>,
    _tool_name: &str,
    _tool_version: &str,
    annotation_limit: usize,
) -> String {
    let mut eligible: Vec<_> = view.full.functions.iter().filter(|v| v.exceeds).collect();
    // CRAP DESC is the primary key; tie-break on (file_path, start_line)
    // so equal-CRAP runs are deterministic across walker orderings (the
    // syn walker is sequential today but a future parallel walker
    // would otherwise leak nondeterminism into PR annotations).
    eligible.sort_by(|a, b| {
        b.scored
            .crap
            .value
            .partial_cmp(&a.scored.crap.value)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.scored
                    .identity
                    .file_path
                    .cmp(&b.scored.identity.file_path)
            })
            .then_with(|| {
                a.scored
                    .identity
                    .span
                    .start_line
                    .cmp(&b.scored.identity.span.start_line)
            })
    });

    let total = eligible.len();
    let take = total.min(annotation_limit);
    let cwd = std::env::current_dir().ok();

    let mut out = String::new();
    for verdict in eligible.iter().take(take) {
        let s = &verdict.scored;
        let raw_file = relativize_path(&s.identity.file_path, cwd.as_deref());
        let line = s.identity.span.start_line;
        let raw_message = format!(
            "Function `{}` has CRAP {:.2} (complexity={}, coverage={:.1}%) which exceeds threshold {:.1}",
            s.identity.qualified_name,
            s.crap.value,
            s.complexity,
            s.coverage_percent,
            verdict.threshold,
        );
        // Two escape contexts per the GH Actions workflow-command spec:
        //   * property values (between `name=` and the next `,` or `::`)
        //     escape `%`, `\r`, `\n`, plus the delimiters `:` and `,`
        //   * message data (after the final `::`) escapes only `%`,
        //     `\r`, `\n` — `:` and `,` are legal in message content
        // `line` is an integer (no escape needed); `title` is built
        // from a deterministic `CRAP <f64>` (no delimiter chars
        // possible). `file` is the only dynamic property value and
        // MUST go through property-level escaping — POSIX file paths
        // legally contain `:` and `,`, and an unescaped delimiter
        // here would corrupt the runner's parse of the workflow
        // command.
        let file = gha_escape_property(&raw_file);
        let message = gha_escape(&raw_message);
        out.push_str(&format!(
            "::warning file={file},line={line},title=CRAP {crap:.1}::{message}\n",
            file = file,
            line = line,
            crap = s.crap.value,
            message = message,
        ));
    }

    let dropped = total.saturating_sub(take);
    if dropped > 0 {
        out.push_str(&format!(
            "::notice::{dropped} more functions exceed threshold; see scorecard for the full list\n"
        ));
    }

    out
}

/// Percent-encode the three characters that would otherwise terminate
/// or corrupt a workflow-command message: `%`, `\r`, `\n`. Per the GH
/// Actions spec, this is the message-data escape (applied to text
/// after the final `::` in the workflow command). Property values use
/// a stricter escape via [`gha_escape_property`] which also covers
/// the property-list delimiters.
///
/// `%` must be escaped first so the `%25` from the subsequent CR/LF
/// substitutions does not get re-escaped.
fn gha_escape(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Percent-encode all five characters the GH Actions spec requires in
/// property-value positions: `%`, `\r`, `\n`, plus the property-list
/// delimiters `:` and `,`. The runner parses each annotation as
/// `name=value,name=value,...::message`, so an unescaped `:` or `,`
/// inside a dynamic value (most realistically a `file=` path on
/// POSIX, where both characters are legal) would split or terminate
/// the property list and corrupt the annotation.
///
/// The five-step order matters: `%` is escaped first via [`gha_escape`]
/// so the `%25` introduced for `\r`/`\n` is not re-escaped; `:` and
/// `,` are then appended for property-only escaping, and their own
/// percent-encoded forms (`%3A`, `%2C`) are not subject to further
/// substitution because no later step touches `%`.
fn gha_escape_property(s: &str) -> String {
    gha_escape(s).replace(':', "%3A").replace(',', "%2C")
}

/// Strip a CWD prefix from `file_path` so PR annotations reference
/// files by repo-relative path (which GitHub renders inline on the
/// diff). Returns the original path unchanged when:
///   * the path is already relative, or
///   * no CWD is available (`current_dir()` failed), or
///   * the path does not live under CWD (`strip_prefix` fails).
///
/// `cwd` is parameterized so unit tests can pin the prefix without
/// chdir'ing the process; production callers thread
/// `std::env::current_dir().ok().as_deref()`.
fn relativize_path(file_path: &str, cwd: Option<&Path>) -> String {
    let p = Path::new(file_path);
    if !p.is_absolute() {
        return file_path.to_string();
    }
    match cwd.and_then(|c| p.strip_prefix(c).ok()) {
        Some(rel) => rel.to_string_lossy().into_owned(),
        None => file_path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::*;
    use crate::domain::types::RiskLevel;

    fn fmt(view: &AnalysisView<'_>, limit: usize) -> String {
        format_github_annotations(view, TEST_TOOL_NAME, TEST_TOOL_VERSION, limit)
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let result = make_empty_result();
        let view = make_view_default(&result);
        assert_eq!(fmt(&view, usize::MAX), "");
    }

    #[test]
    fn single_exceeding_function_emits_one_warning_line() {
        let result = make_single_function_result(
            "complex_fn",
            "src/lib.rs",
            10,
            30.0,
            30.0,
            RiskLevel::High,
            8.0,
        );
        let view = make_view_default(&result);
        let out = fmt(&view, usize::MAX);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 1, "expected one line, got {lines:?}");
        let line = lines[0];
        assert!(line.starts_with("::warning "), "wrong prefix: {line}");
        assert!(line.contains("file=src/lib.rs"));
        assert!(line.contains("line=1"));
        assert!(line.contains("title=CRAP 30.0"));
        assert!(line.contains("complex_fn"));
        assert!(line.contains("complexity=10"));
    }

    #[test]
    fn below_threshold_function_emits_nothing() {
        // Low risk, score below threshold, exceeds=false
        let result = make_single_function_result(
            "simple_fn",
            "src/lib.rs",
            1,
            100.0,
            1.0,
            RiskLevel::Low,
            8.0,
        );
        let view = make_view_default(&result);
        assert_eq!(fmt(&view, usize::MAX), "");
    }

    #[test]
    fn output_is_sorted_by_crap_desc() {
        use crate::domain::types::{AnalysisResult, AnalysisSummary};
        let low = make_verdict("low", "src/a.rs", 5, 50.0, 12.0, RiskLevel::Moderate, 8.0);
        let mid = make_verdict("mid", "src/b.rs", 8, 30.0, 22.0, RiskLevel::High, 8.0);
        let high = make_verdict("high", "src/c.rs", 12, 20.0, 45.0, RiskLevel::High, 8.0);
        let result = AnalysisResult {
            // intentionally unsorted on input
            functions: vec![low, high, mid],
            summary: AnalysisSummary {
                total_functions: 3,
                ..Default::default()
            },
            passed: false,
        };
        let view = make_view_default(&result);
        let out = fmt(&view, usize::MAX);
        let lines: Vec<&str> = out.lines().collect();
        // CRAP descending: high (45), mid (22), low (12)
        assert!(lines[0].contains("high"), "first should be high: {lines:?}");
        assert!(lines[1].contains("mid"), "second should be mid: {lines:?}");
        assert!(lines[2].contains("low"), "third should be low: {lines:?}");
    }

    #[test]
    fn message_escapes_percent_carriage_return_and_newline() {
        // A qualified name laced with the three escape-required chars.
        // gha_escape must replace `%` first (else the `%25` from CR/LF
        // would re-escape its own `%`).
        let raw = "weird%name\rwith\nbreaks";
        let escaped = gha_escape(raw);
        assert_eq!(escaped, "weird%25name%0Dwith%0Abreaks");
    }

    #[test]
    fn gha_escape_leaves_safe_chars_alone() {
        assert_eq!(
            gha_escape("module::submodule::function"),
            "module::submodule::function",
            "colons are legal in message data, must NOT be escaped"
        );
        assert_eq!(gha_escape("a,b,c"), "a,b,c", "commas legal in message");
        assert_eq!(gha_escape(""), "");
    }

    #[test]
    fn gha_escape_property_covers_colon_and_comma() {
        // Five-char property escape: %, CR, LF, :, ,
        assert_eq!(
            gha_escape_property("src:weird,file.rs"),
            "src%3Aweird%2Cfile.rs"
        );
        // % is still escaped (inherits from gha_escape) so a literal
        // % in a path doesn't get confused with our percent-encoded
        // sequences.
        assert_eq!(gha_escape_property("f%o.rs"), "f%25o.rs");
        // CR / LF still get the message-data escape (we never want
        // them to land in property values either).
        assert_eq!(gha_escape_property("a\rb\nc"), "a%0Db%0Ac");
        // No-op on a plain path.
        assert_eq!(gha_escape_property("src/lib.rs"), "src/lib.rs");
    }

    #[test]
    fn file_property_escapes_delimiters_in_path() {
        // A path that legally contains both delimiters must not corrupt
        // the workflow command. The annotation should still parse as
        // a single `name=value,name=value::message` triple.
        let result = make_single_function_result(
            "weird_fn",
            "src/a:b,c.rs",
            10,
            0.0,
            42.0,
            RiskLevel::High,
            8.0,
        );
        let view = make_view_default(&result);
        let out = fmt(&view, usize::MAX);
        let line = out.lines().next().expect("one warning line");
        assert!(
            line.contains("file=src/a%3Ab%2Cc.rs"),
            "file= must escape `:` and `,`, got: {line}"
        );
        // Exactly two `,` separators between three properties
        // (file/line/title), then the `::` data marker.
        let before_message = line.split("::").nth(1).expect("`::` separator present");
        assert_eq!(
            before_message.matches(',').count(),
            2,
            "property list must have exactly two `,` separators between (file/line/title), got: {before_message}"
        );
    }

    #[test]
    fn equal_crap_scores_sort_by_file_path_then_line() {
        use crate::domain::types::{AnalysisResult, AnalysisSummary};
        // Three exceeders with identical CRAP score; deliberately
        // shuffled file/line order on input. Tie-break must produce
        // (z.rs, a.rs:5, a.rs:10) → sorted by file ASC, then line ASC.
        let v_z = make_verdict("z_fn", "z.rs", 10, 0.0, 42.0, RiskLevel::High, 8.0);
        let v_a_10 = make_verdict("a_late", "a.rs", 10, 0.0, 42.0, RiskLevel::High, 8.0);
        let mut v_a_5 = make_verdict("a_early", "a.rs", 10, 0.0, 42.0, RiskLevel::High, 8.0);
        v_a_5.scored.identity.span.start_line = 5;
        let mut v_a_10_at_10 = v_a_10.clone();
        v_a_10_at_10.scored.identity.span.start_line = 10;
        let result = AnalysisResult {
            functions: vec![v_z, v_a_10_at_10, v_a_5], // intentionally shuffled
            summary: AnalysisSummary {
                total_functions: 3,
                ..Default::default()
            },
            passed: false,
        };
        let view = make_view_default(&result);
        let out = fmt(&view, usize::MAX);
        let lines: Vec<&str> = out.lines().collect();
        // Expected order: a.rs:5 (a_early), a.rs:10 (a_late), z.rs (z_fn).
        assert!(
            lines[0].contains("a_early"),
            "tie-break by file ASC then line ASC: a.rs:5 first, got:\n{out}"
        );
        assert!(
            lines[1].contains("a_late"),
            "tie-break by line within file: a.rs:10 second, got:\n{out}"
        );
        assert!(lines[2].contains("z_fn"), "z.rs last, got:\n{out}");
    }

    #[test]
    fn relativize_strips_cwd_prefix_when_path_is_absolute_under_cwd() {
        let cwd = Path::new("/home/user/repo");
        let abs = "/home/user/repo/src/lib.rs";
        assert_eq!(relativize_path(abs, Some(cwd)), "src/lib.rs");
    }

    #[test]
    fn relativize_falls_back_to_absolute_when_strip_prefix_fails() {
        let cwd = Path::new("/home/user/repo");
        let abs = "/elsewhere/other/file.rs";
        assert_eq!(relativize_path(abs, Some(cwd)), "/elsewhere/other/file.rs");
    }

    #[test]
    fn relativize_passes_through_already_relative_paths() {
        let cwd = Path::new("/home/user/repo");
        assert_eq!(relativize_path("src/lib.rs", Some(cwd)), "src/lib.rs");
    }

    #[test]
    fn relativize_handles_no_cwd_gracefully() {
        let abs = "/home/user/repo/src/lib.rs";
        assert_eq!(relativize_path(abs, None), "/home/user/repo/src/lib.rs");
    }

    #[test]
    fn truncation_emits_top_n_and_appends_dropped_notice() {
        use crate::domain::types::{AnalysisResult, AnalysisSummary};
        // Five exceeders with strictly-decreasing CRAP — limit=2 must
        // keep the top two (CRAP 50, 40) and drop the bottom three
        // (30, 20, 10). The trailing ::notice must name the dropped
        // count (3) in the exact wording asserted by the BDD scenario.
        let v50 = make_verdict("worst", "src/a.rs", 12, 10.0, 50.0, RiskLevel::High, 8.0);
        let v40 = make_verdict("bad", "src/b.rs", 10, 15.0, 40.0, RiskLevel::High, 8.0);
        let v30 = make_verdict("mid", "src/c.rs", 8, 25.0, 30.0, RiskLevel::High, 8.0);
        let v20 = make_verdict("low", "src/d.rs", 6, 40.0, 20.0, RiskLevel::High, 8.0);
        let v10 = make_verdict("least", "src/e.rs", 4, 60.0, 10.0, RiskLevel::Moderate, 8.0);
        let result = AnalysisResult {
            functions: vec![v10, v20, v30, v40, v50], // intentionally unsorted
            summary: AnalysisSummary {
                total_functions: 5,
                ..Default::default()
            },
            passed: false,
        };
        let view = make_view_default(&result);
        let out = fmt(&view, 2);

        let warnings: Vec<&str> = out
            .lines()
            .filter(|l| l.starts_with("::warning "))
            .collect();
        assert_eq!(warnings.len(), 2, "expected 2 ::warnings, got:\n{out}");
        assert!(warnings[0].contains("worst"), "top-1 must be worst: {out}");
        assert!(warnings[1].contains("bad"), "top-2 must be bad: {out}");

        let notices: Vec<&str> = out.lines().filter(|l| l.starts_with("::notice")).collect();
        assert_eq!(
            notices.len(),
            1,
            "expected one trailing notice, got:\n{out}"
        );
        assert_eq!(
            notices[0],
            "::notice::3 more functions exceed threshold; see scorecard for the full list",
        );
    }

    #[test]
    fn no_notice_when_limit_not_exceeded() {
        // Limit == total eligible: every exceeder is emitted, no
        // notice line follows.
        let result = make_single_function_result(
            "complex_fn",
            "src/lib.rs",
            10,
            20.0,
            30.0,
            RiskLevel::High,
            8.0,
        );
        let view = make_view_default(&result);
        let out = fmt(&view, 10);
        let warnings = out.lines().filter(|l| l.starts_with("::warning ")).count();
        let notices = out.lines().filter(|l| l.starts_with("::notice")).count();
        assert_eq!(warnings, 1);
        assert_eq!(notices, 0, "no notice expected, got:\n{out}");
    }

    #[test]
    fn qualified_name_with_colons_passes_through_verbatim() {
        let result = make_single_function_result(
            "module::sub::function",
            "src/lib.rs",
            10,
            30.0,
            30.0,
            RiskLevel::High,
            8.0,
        );
        let view = make_view_default(&result);
        let out = fmt(&view, usize::MAX);
        assert!(
            out.contains("module::sub::function"),
            "qualified name must appear verbatim: {out}"
        );
    }
}
