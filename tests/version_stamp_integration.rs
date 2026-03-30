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
    assert_eq!(
        output,
        format!("crap4rs {}", env!("CARGO_PKG_VERSION")),
        "-V must print the package semver only"
    );
}

// ── --version long flag ───────────────────────────────────────────────

/// `--version` must match `crap4rs X.Y.Z (XXXXXXX YYYY-MM-DD)` when git is
/// available, or `crap4rs X.Y.Z (YYYY-MM-DD)` for tarball/offline builds.
///
/// The date is always present — it provides a freshness signal even when the
/// git hash cannot be determined.
#[test]
fn long_flag_matches_expected_pattern() {
    let output = run_version_flag("--version");
    assert!(
        output.starts_with(&format!("crap4rs {}", env!("CARGO_PKG_VERSION"))),
        "--version should start with package semver, got: {output:?}"
    );

    // Strip "crap4rs " prefix
    let rest = output.trim_start_matches("crap4rs ").trim();

    // Split at first space: "0.1.0" and "(abc1234 2026-03-29)" or "(2026-03-29)"
    let mut parts = rest.splitn(2, ' ');
    let semver = parts.next().unwrap_or("");
    let meta = parts.next();

    // Semver portion
    assert!(
        semver.chars().all(|c| c.is_ascii_digit() || c == '.'),
        "version portion must be semver, got: {semver:?}"
    );

    // Metadata is always present (date at minimum)
    let meta = meta.expect("--version must always include build metadata (date)");
    let meta = meta.trim_matches(|c| c == '(' || c == ')');
    let mut meta_parts = meta.splitn(2, ' ');
    let first = meta_parts.next().unwrap_or("");
    let second = meta_parts.next();

    let date = if let Some(date) = second {
        // Full form: "(hash date)" — first is the git hash
        let hash = first;
        assert!(
            hash.len() >= 7,
            "hash must be at least 7 hex chars, got: {hash:?}"
        );
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash must be hex, got: {hash:?}"
        );
        date
    } else {
        // Date-only form: "(date)" — no git hash available
        first
    };

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

/// `--version` output must begin with the package semver declared in Cargo.toml.
#[test]
fn long_flag_semver_matches_cargo_pkg_version() {
    let expected_semver = env!("CARGO_PKG_VERSION");
    let output = run_version_flag("--version");
    assert!(
        output.starts_with(&format!("crap4rs {expected_semver}")),
        "--version should start with semver {expected_semver:?}, got: {output:?}"
    );
}
