//! Integration tests for version stamp output (#50).
//!
//! Verifies that `--version` includes git metadata when available and that
//! `-V` always returns semver only (clap's short-version behavior).

use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

fn run_version_flag(flag: &str) -> String {
    let out = Command::new(BINARY)
        .arg(flag)
        .output()
        .expect("failed to run crap4rs binary");
    // clap prints version to stdout
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// ── -V short flag ─────────────────────────────────────────────────────

/// `-V` must always print semver only — no git metadata.
///
/// This is clap's built-in `version` attribute; `long_version` does NOT
/// affect it. Pinning this keeps the output stable for scripts.
#[test]
fn short_flag_prints_semver_only() {
    let output = run_version_flag("-V");
    // Must be exactly "crap4rs X.Y.Z" — no parenthetical suffix
    assert!(
        output.starts_with("crap4rs "),
        "output should start with 'crap4rs ': {output:?}"
    );
    let version_part = output.trim_start_matches("crap4rs ").trim();
    // Semver: digits and dots only
    assert!(
        version_part.chars().all(|c| c.is_ascii_digit() || c == '.'),
        "-V should be semver only, got: {output:?}"
    );
    // No parenthetical metadata
    assert!(
        !output.contains('('),
        "-V must not contain git metadata: {output:?}"
    );
}

// ── --version long flag ───────────────────────────────────────────────

/// `--version` must match `crap4rs X.Y.Z` optionally followed by
/// ` (XXXXXXX YYYY-MM-DD)`.
///
/// The test accepts both forms because CI may build without a git repo
/// (e.g. GitHub Actions `actions/checkout` with `fetch-depth: 0` is fine
/// but shallow clones may not expose HEAD). The regex allows the metadata
/// suffix to be absent gracefully.
#[test]
fn long_flag_matches_expected_pattern() {
    let output = run_version_flag("--version");
    assert!(
        output.starts_with("crap4rs "),
        "output should start with 'crap4rs ': {output:?}"
    );

    // Strip "crap4rs " prefix
    let rest = output.trim_start_matches("crap4rs ").trim();

    // Split at first space: "0.1.0" and optional "(abc1234 2026-03-29)"
    let mut parts = rest.splitn(2, ' ');
    let semver = parts.next().unwrap_or("");
    let meta = parts.next();

    // Semver portion
    assert!(
        semver.chars().all(|c| c.is_ascii_digit() || c == '.'),
        "version portion must be semver, got: {semver:?}"
    );

    // If metadata present, validate format
    if let Some(meta) = meta {
        let meta = meta.trim_matches(|c| c == '(' || c == ')');
        let mut meta_parts = meta.splitn(2, ' ');
        let hash = meta_parts.next().unwrap_or("");
        let date = meta_parts.next().unwrap_or("");

        assert_eq!(hash.len(), 7, "hash must be 7 hex chars, got: {hash:?}");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash must be hex, got: {hash:?}"
        );

        assert_eq!(date.len(), 10, "date must be YYYY-MM-DD, got: {date:?}");
        let date_parts: Vec<&str> = date.split('-').collect();
        assert_eq!(date_parts.len(), 3);
        assert!(
            date_parts
                .iter()
                .all(|p| p.chars().all(|c| c.is_ascii_digit())),
            "date must be all digits, got: {date:?}"
        );
    }
}

/// `--version` output must begin with the package semver declared in Cargo.toml.
#[test]
fn long_flag_semver_matches_cargo_pkg_version() {
    let expected_semver = env!("CARGO_PKG_VERSION");
    let output = run_version_flag("--version");
    assert!(
        output.contains(expected_semver),
        "--version should contain semver {expected_semver:?}, got: {output:?}"
    );
}
