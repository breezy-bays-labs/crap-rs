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
    eligible.sort_by(|a, b| {
        b.scored
            .crap
            .value
            .partial_cmp(&a.scored.crap.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total = eligible.len();
    let take = total.min(annotation_limit);
    let cwd = std::env::current_dir().ok();

    let mut out = String::new();
    for verdict in eligible.iter().take(take) {
        let s = &verdict.scored;
        let file = relativize_path(&s.identity.file_path, cwd.as_deref());
        let line = s.identity.span.start_line;
        let raw_message = format!(
            "Function `{}` has CRAP {:.2} (complexity={}, coverage={:.1}%) which exceeds threshold {:.1}",
            s.identity.qualified_name,
            s.crap.value,
            s.complexity,
            s.coverage_percent,
            verdict.threshold,
        );
        // Only the data after `::` carries dynamic text — file/line/title
        // property values are deterministic (path, integer, score-only)
        // so they need no delimiter escaping. The message data only
        // needs percent / CR / LF escaping per the GH Actions spec.
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
/// Actions spec, only the message data needs this escape — property
/// values (`file=`, `line=`, `title=`) are separately delimited by `,`
/// and `:` and require different escaping IF they carry dynamic data
/// (we keep them deterministic so they do not).
///
/// `%` must be escaped first so the `%25` from the subsequent CR/LF
/// substitutions does not get re-escaped.
fn gha_escape(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
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
