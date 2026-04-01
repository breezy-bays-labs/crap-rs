//! Git diff adapter — shells out to `git diff` and parses unified diff output.

use crate::domain::types::{CrapError, FileChangeKind, SourceSpan};
use crate::ports::DiffPort;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

#[derive(Default)]
pub struct GitDiffAdapter;

impl GitDiffAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl DiffPort for GitDiffAdapter {
    fn changed_regions(
        &self,
        diff_ref: &str,
        working_dir: &Path,
        paths: &[String],
    ) -> Result<HashMap<String, FileChangeKind>, CrapError> {
        let output = Command::new("git")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env("GIT_PAGER", "")
            .current_dir(working_dir)
            .args([
                "diff",
                "--unified=0",
                "--no-prefix",
                "--no-color",
                "--diff-filter=ACMR",
            ])
            .arg(diff_ref)
            .arg("--")
            .args(paths)
            .output()
            .map_err(|e| CrapError::DiffCompute(format!("failed to run git diff: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CrapError::DiffCompute(stderr.trim().to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_unified_diff(&stdout))
    }
}

/// Regex anchored to the full hunk header structure.
/// Captures the new-side start line and optional count from `+c,d` or `+c`.
/// Handles all 4 formats: `@@ -a,b +c,d @@`, `@@ -a +c,d @@`,
/// `@@ -a,b +c @@` (implicit count=1), `@@ -a +c @@`.
static HUNK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@").expect("hunk regex is valid")
});

#[derive(Default)]
struct DiffParseState {
    current_file: Option<String>,
    is_new_file: bool,
}

enum DiffLine<'a> {
    DiffHeader,
    NewFileMode,
    FilePath(&'a str),
    Hunk(&'a str),
    Other,
}

/// Parse unified diff output into a map of file path → change kind.
fn parse_unified_diff(input: &str) -> HashMap<String, FileChangeKind> {
    let mut result: HashMap<String, FileChangeKind> = HashMap::new();
    let mut state = DiffParseState::default();

    for line in input.lines() {
        handle_diff_line(classify_diff_line(line), &mut state, &mut result);
    }

    result
}

fn classify_diff_line(line: &str) -> DiffLine<'_> {
    if let Some(path) = line.strip_prefix("+++ ") {
        DiffLine::FilePath(path)
    } else if line.starts_with("new file mode") {
        DiffLine::NewFileMode
    } else if line.starts_with("diff --git") {
        DiffLine::DiffHeader
    } else if line.starts_with("@@ ") {
        DiffLine::Hunk(line)
    } else {
        DiffLine::Other
    }
}

fn handle_diff_line(
    line: DiffLine<'_>,
    state: &mut DiffParseState,
    result: &mut HashMap<String, FileChangeKind>,
) {
    match line {
        DiffLine::DiffHeader => state.is_new_file = false,
        DiffLine::NewFileMode => state.is_new_file = true,
        DiffLine::FilePath(path) => state.current_file = normalize_diff_path(path),
        DiffLine::Hunk(header) => handle_hunk_line(header, state, result),
        DiffLine::Other => {}
    }
}

fn normalize_diff_path(path: &str) -> Option<String> {
    if path == "/dev/null" {
        None
    } else {
        Some(normalize_path(path))
    }
}

fn handle_hunk_line(
    header: &str,
    state: &DiffParseState,
    result: &mut HashMap<String, FileChangeKind>,
) {
    let Some(file) = state.current_file.as_ref() else {
        return;
    };

    if result.get(file) == Some(&FileChangeKind::NewFile) {
        return;
    }

    if state.is_new_file {
        result.insert(file.clone(), FileChangeKind::NewFile);
        return;
    }

    if let Some(span) = parse_hunk_header(header) {
        append_modified_span(result, file, span);
    }
}

fn append_modified_span(
    result: &mut HashMap<String, FileChangeKind>,
    file: &str,
    span: SourceSpan,
) {
    result
        .entry(file.to_owned())
        .and_modify(|kind| {
            if let FileChangeKind::Modified(spans) = kind {
                spans.push(span);
            }
        })
        .or_insert_with(|| FileChangeKind::Modified(vec![span]));
}

/// Parse a hunk header line to extract the new-side span.
///
/// Returns `None` for deletion-only hunks (count=0) and for unparseable headers.
/// Unparseable headers (e.g., usize overflow on astronomical line numbers) are
/// treated as skippable — the caller silently omits the hunk, same as deletion-only.
/// This is an accepted risk: git produces well-formed output and line numbers
/// that overflow usize are not practically possible.
fn parse_hunk_header(line: &str) -> Option<SourceSpan> {
    let caps = HUNK_RE.captures(line)?;
    let start: usize = caps.get(1)?.as_str().parse().ok()?;
    let count: usize = caps
        .get(2)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(1);

    if count == 0 {
        return None; // Deletion-only hunk
    }

    Some(SourceSpan {
        start_line: start,
        end_line: start + count - 1,
    })
}

/// Normalize path separators to forward slash.
fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_hunk_header ───────────────────────────────────────────

    #[test]
    fn hunk_standard_format() {
        // @@ -a,b +c,d @@
        let span = parse_hunk_header("@@ -10,5 +20,3 @@ fn foo()").unwrap();
        assert_eq!(span.start_line, 20);
        assert_eq!(span.end_line, 22);
    }

    #[test]
    fn hunk_one_line_removed() {
        // @@ -a +c,d @@
        let span = parse_hunk_header("@@ -10 +20,3 @@").unwrap();
        assert_eq!(span.start_line, 20);
        assert_eq!(span.end_line, 22);
    }

    #[test]
    fn hunk_implicit_count_one() {
        // @@ -a,b +c @@ (count=1 implicit)
        let span = parse_hunk_header("@@ -10,5 +20 @@").unwrap();
        assert_eq!(span.start_line, 20);
        assert_eq!(span.end_line, 20);
    }

    #[test]
    fn hunk_both_count_one() {
        // @@ -a +c @@ (both implicit count=1)
        let span = parse_hunk_header("@@ -10 +20 @@").unwrap();
        assert_eq!(span.start_line, 20);
        assert_eq!(span.end_line, 20);
    }

    #[test]
    fn hunk_deletion_only() {
        // @@ -a,b +c,0 @@ → None
        assert!(parse_hunk_header("@@ -10,3 +20,0 @@").is_none());
    }

    // ── parse_unified_diff ──────────────────────────────────────────

    #[test]
    fn parse_empty_input() {
        let result = parse_unified_diff("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_modified_file_single_hunk() {
        let diff = "\
diff --git src/foo.rs src/foo.rs
index abc..def 100644
--- src/foo.rs
+++ src/foo.rs
@@ -10,3 +10,5 @@ fn existing()
+    let x = 1;
+    let y = 2;
";
        let result = parse_unified_diff(diff);
        assert_eq!(result.len(), 1);
        match &result["src/foo.rs"] {
            FileChangeKind::Modified(spans) => {
                assert_eq!(spans.len(), 1);
                assert_eq!(spans[0].start_line, 10);
                assert_eq!(spans[0].end_line, 14);
            }
            _ => panic!("expected Modified"),
        }
    }

    #[test]
    fn parse_modified_file_multiple_hunks() {
        let diff = "\
diff --git src/foo.rs src/foo.rs
index abc..def 100644
--- src/foo.rs
+++ src/foo.rs
@@ -5,0 +5,2 @@ fn first()
+    new line 1
+    new line 2
@@ -20,0 +22,1 @@ fn second()
+    another line
";
        let result = parse_unified_diff(diff);
        match &result["src/foo.rs"] {
            FileChangeKind::Modified(spans) => {
                assert_eq!(spans.len(), 2);
                assert_eq!(spans[0].start_line, 5);
                assert_eq!(spans[0].end_line, 6);
                assert_eq!(spans[1].start_line, 22);
                assert_eq!(spans[1].end_line, 22);
            }
            _ => panic!("expected Modified"),
        }
    }

    #[test]
    fn parse_new_file() {
        let diff = "\
diff --git src/new.rs src/new.rs
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ src/new.rs
@@ -0,0 +1,10 @@
+fn hello() {}
";
        let result = parse_unified_diff(diff);
        assert_eq!(result["src/new.rs"], FileChangeKind::NewFile);
    }

    #[test]
    fn parse_multiple_files() {
        let diff = "\
diff --git src/a.rs src/a.rs
new file mode 100644
index 0000000..abc
--- /dev/null
+++ src/a.rs
@@ -0,0 +1,5 @@
+content
diff --git src/b.rs src/b.rs
index abc..def 100644
--- src/b.rs
+++ src/b.rs
@@ -10,2 +10,3 @@ fn foo()
+added
";
        let result = parse_unified_diff(diff);
        assert_eq!(result.len(), 2);
        assert_eq!(result["src/a.rs"], FileChangeKind::NewFile);
        assert!(matches!(result["src/b.rs"], FileChangeKind::Modified(_)));
    }

    #[test]
    fn parse_deletion_only_hunk_skipped() {
        let diff = "\
diff --git src/foo.rs src/foo.rs
index abc..def 100644
--- src/foo.rs
+++ src/foo.rs
@@ -10,3 +10,0 @@ fn deleted_lines()
-removed1
-removed2
-removed3
";
        let result = parse_unified_diff(diff);
        // Deletion-only hunk means no new lines → file shouldn't appear
        assert!(result.is_empty());
    }

    #[test]
    fn parse_renamed_file_maps_to_new_path() {
        let diff = "\
diff --git src/old.rs src/new_name.rs
similarity index 95%
rename from src/old.rs
rename to src/new_name.rs
index abc..def 100644
--- src/old.rs
+++ src/new_name.rs
@@ -5,1 +5,2 @@ fn foo()
+    added line
";
        let result = parse_unified_diff(diff);
        assert!(result.contains_key("src/new_name.rs"));
        assert!(!result.contains_key("src/old.rs"));
    }

    // ── normalize_path ──────────────────────────────────────────────

    #[test]
    fn normalize_backslash() {
        assert_eq!(normalize_path("src\\sub\\mod.rs"), "src/sub/mod.rs");
    }

    #[test]
    fn normalize_forward_slash_unchanged() {
        assert_eq!(normalize_path("src/sub/mod.rs"), "src/sub/mod.rs");
    }

    // ── integration test with real git (tempdir) ────────────────────

    #[test]
    fn git_diff_adapter_real_repo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // Initialize repo
        test_git_repo(path);

        // Create initial commit
        std::fs::write(path.join("lib.rs"), "fn old() {}\n").unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-m", "initial"]);

        // Modify file
        std::fs::write(path.join("lib.rs"), "fn old() {}\nfn new_func() {}\n").unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-m", "add function"]);

        let adapter = GitDiffAdapter::new();
        let result = adapter
            .changed_regions("HEAD~1", path, &["lib.rs".to_string()])
            .unwrap();

        assert!(result.contains_key("lib.rs"));
        match &result["lib.rs"] {
            FileChangeKind::Modified(spans) => {
                assert!(!spans.is_empty());
                // The new line should be in the changed spans
                assert!(spans.iter().any(|s| s.start_line == 2));
            }
            FileChangeKind::NewFile => panic!("expected Modified, got NewFile"),
        }
    }

    #[test]
    fn git_diff_adapter_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        test_git_repo(path);

        // Empty initial commit
        git(path, &["commit", "--allow-empty", "-m", "initial"]);

        // Add new file
        std::fs::write(path.join("new.rs"), "fn hello() {}\n").unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-m", "add new file"]);

        let adapter = GitDiffAdapter::new();
        let result = adapter
            .changed_regions("HEAD~1", path, &["new.rs".to_string()])
            .unwrap();

        assert_eq!(result["new.rs"], FileChangeKind::NewFile);
    }

    #[test]
    fn git_diff_adapter_bad_ref() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        test_git_repo(path);
        git(path, &["commit", "--allow-empty", "-m", "initial"]);

        let adapter = GitDiffAdapter::new();
        let result = adapter.changed_regions("nonexistent-ref", path, &[]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("nonexistent-ref"),
            "error should mention the bad ref: {err}"
        );
    }

    #[test]
    fn git_diff_adapter_empty_diff() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        test_git_repo(path);

        std::fs::write(path.join("lib.rs"), "fn stable() {}\n").unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-m", "initial"]);

        // Diff HEAD against itself → empty
        let adapter = GitDiffAdapter::new();
        let result = adapter
            .changed_regions("HEAD", path, &["lib.rs".to_string()])
            .unwrap();

        assert!(result.is_empty());
    }

    // ── helpers ─────────────────────────────────────────────────────

    fn test_git_repo(dir: &Path) {
        git(dir, &["init"]);
        git(dir, &["config", "user.email", "test@test.com"]);
        git(dir, &["config", "user.name", "Test"]);
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("git command failed to start");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
