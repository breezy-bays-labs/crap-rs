//! Regression guard ("smoke detector") for
//! `scripts/coverage-staleness-check.sh`.
//!
//! The script runs in CI (`.github/workflows/quick-start-smoke.yml`)
//! and warns when a contributor edits the `crap-examples` sample
//! SOURCE without regenerating the committed coverage fixture — drift
//! that would otherwise publish a stale baseline envelope and silently
//! corrupt every downstream consumer's Delta tab. This test exercises
//! all four of the script's branches in a hermetic temp git repo so a
//! future workflow refactor cannot silently kill the warning:
//!
//!   * empty / all-zero base ref            -> `::notice::` (no valid base ref)
//!   * unreachable base ref                 -> `::notice::` (base ... unreachable)
//!   * sample source changed, no regen      -> `::warning::` (the drift signal)
//!   * fixture regenerated, or no change    -> silent (exit 0)
//!
//! This guards the script's LOGIC. The complementary proof that
//! GitHub's runtime actually drives these branches (e.g. supplies an
//! all-zero SHA on an orphan first-push) is the synthetic-push
//! validation done empirically on real Actions runs — the two together
//! cover both the logic layer and the integration layer.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

fn script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/coverage-staleness-check.sh")
        .canonicalize()
        .expect("staleness script exists at scripts/coverage-staleness-check.sh")
}

/// Run a git command in `repo`, fully isolated from the developer's
/// global/system git config (so local `commit.gpgsign`, hooks, or a
/// custom `init.defaultBranch` can't perturb the hermetic fixture).
fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git is on PATH");
    assert!(status.success(), "git {args:?} failed");
}

/// Write `contents` to `repo/rel`, creating parent dirs, then commit
/// just that file with message `msg`.
fn commit_file(repo: &Path, rel: &str, contents: &str, msg: &str) {
    let path = repo.join(rel);
    std::fs::create_dir_all(path.parent().expect("rel has a parent dir"))
        .expect("create parent dirs");
    std::fs::write(&path, contents).expect("write file");
    git(repo, &["add", rel]);
    git(repo, &["commit", "-q", "-m", msg]);
}

/// Initialize a temp git repo with a single base commit and return the
/// repo dir plus the base commit SHA.
fn init_repo_with_base() -> (TempDir, String) {
    let tmp = tempfile::tempdir().expect("tempdir");
    git(tmp.path(), &["init", "-q", "-b", "main"]);
    commit_file(tmp.path(), "README.md", "base\n", "base commit");
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(tmp.path())
        .output()
        .expect("git rev-parse runs");
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (tmp, sha)
}

/// Invoke the staleness script in `repo` against `base_ref`, asserting
/// it always exits 0 (warn-not-fail is the whole point) and returning
/// its stdout.
fn run_check(repo: &Path, base_ref: &str) -> String {
    let out = Command::new("bash")
        .arg(script_path())
        .arg(base_ref)
        .current_dir(repo)
        .output()
        .expect("bash runs the script");
    assert!(
        out.status.success(),
        "staleness script must always exit 0 (warn-not-fail). stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn empty_base_ref_emits_no_valid_base_ref_notice() {
    let (tmp, _base) = init_repo_with_base();
    let stdout = run_check(tmp.path(), "");
    assert!(
        stdout.contains("::notice::") && stdout.contains("no valid base ref"),
        "expected a no-valid-base-ref notice, got:\n{stdout}"
    );
}

#[test]
fn all_zero_base_ref_emits_no_valid_base_ref_notice() {
    let (tmp, _base) = init_repo_with_base();
    let stdout = run_check(tmp.path(), ZERO_SHA);
    assert!(
        stdout.contains("::notice::") && stdout.contains("no valid base ref"),
        "expected a no-valid-base-ref notice for the all-zero SHA, got:\n{stdout}"
    );
}

#[test]
fn unreachable_base_ref_emits_unreachable_notice() {
    // A plausible-looking but nonexistent SHA in a repo with no `origin`
    // remote: the `git fetch` fallback fails silently, the second
    // `cat-file` check still fails, and the script exits with the
    // unreachable notice (never an error).
    let (tmp, _base) = init_repo_with_base();
    let bogus = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let stdout = run_check(tmp.path(), bogus);
    assert!(
        stdout.contains("::notice::") && stdout.contains("unreachable"),
        "expected an unreachable-base notice, got:\n{stdout}"
    );
}

#[test]
fn rust_source_changed_without_regen_emits_warning() {
    let (tmp, base) = init_repo_with_base();
    commit_file(
        tmp.path(),
        "crates/crap-examples/src/event_log.rs",
        "// edited, no lcov regen\n",
        "edit rust sample without regen",
    );
    let stdout = run_check(tmp.path(), &base);
    assert!(
        stdout.contains("::warning::") && stdout.contains("coverage regen"),
        "expected a drift warning for changed Rust source, got:\n{stdout}"
    );
}

#[test]
fn ts_source_changed_without_regen_emits_warning() {
    let (tmp, base) = init_repo_with_base();
    commit_file(
        tmp.path(),
        "crates/crap-examples/ts/eventLog.ts",
        "// edited, no coverage-final.json regen\n",
        "edit ts sample without regen",
    );
    let stdout = run_check(tmp.path(), &base);
    assert!(
        stdout.contains("::warning::") && stdout.contains("coverage regen"),
        "expected a drift warning for changed TypeScript source, got:\n{stdout}"
    );
}

#[test]
fn source_and_fixture_both_changed_is_silent() {
    let (tmp, base) = init_repo_with_base();
    let path = tmp.path();
    // Stage both a source edit AND a fixture regen in the same HEAD diff.
    std::fs::create_dir_all(path.join("crates/crap-examples/src")).unwrap();
    std::fs::write(
        path.join("crates/crap-examples/src/event_log.rs"),
        "// edited\n",
    )
    .unwrap();
    std::fs::write(path.join("crates/crap-examples/lcov.info"), "SF:x\n").unwrap();
    git(path, &["add", "-A"]);
    git(path, &["commit", "-q", "-m", "edit + regen together"]);
    let stdout = run_check(path, &base);
    assert!(
        !stdout.contains("::warning::"),
        "no warning expected when the fixture was regenerated, got:\n{stdout}"
    );
}

#[test]
fn non_sample_change_is_silent() {
    let (tmp, base) = init_repo_with_base();
    commit_file(
        tmp.path(),
        "src/unrelated.rs",
        "// not under crap-examples\n",
        "unrelated change",
    );
    let stdout = run_check(tmp.path(), &base);
    assert!(
        !stdout.contains("::warning::") && !stdout.contains("::notice::"),
        "no annotation expected for a change outside crap-examples, got:\n{stdout}"
    );
}
