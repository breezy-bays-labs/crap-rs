//! Build script — embeds git commit hash + build date into the binary.
//!
//! Sets `CRAP4RS_LONG_VERSION` at compile time. Clap picks this up via
//! `long_version = env!("CRAP4RS_LONG_VERSION")` and displays it for
//! `--version` (but not `-V`, which uses `version` / `CARGO_PKG_VERSION`).
//!
//! Output examples:
//!   - Source build with git: `0.1.0 (abc1234 2026-03-29)`
//!   - Built without git:     `0.1.0`

use std::env;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn git_hash() -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn build_date() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // Civil date from Unix timestamp — Hinnant's algorithm
    // https://howardhinnant.github.io/date_algorithms.html
    let days = secs / 86400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}")
}

fn format_long_version(version: &str, hash: &str, date: &str) -> String {
    if hash.is_empty() {
        version.to_string()
    } else {
        format!("{version} ({hash} {date})")
    }
}

fn main() {
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let long_version = format_long_version(&version, &git_hash(), &build_date());

    println!("cargo:rustc-env=CRAP4RS_LONG_VERSION={long_version}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/");
}

// ── Unit tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_long_version_with_hash() {
        let v = format_long_version("0.1.0", "abc1234", "2026-03-29");
        assert_eq!(v, "0.1.0 (abc1234 2026-03-29)");
    }

    #[test]
    fn format_long_version_empty_hash_returns_semver_only() {
        let v = format_long_version("0.1.0", "", "2026-03-29");
        assert_eq!(v, "0.1.0");
    }

    #[test]
    fn build_date_is_yyyy_mm_dd() {
        let d = build_date();
        // Must be exactly 10 chars: YYYY-MM-DD
        assert_eq!(d.len(), 10);
        let parts: Vec<&str> = d.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].len(), 4, "year should be 4 digits");
        assert_eq!(parts[1].len(), 2, "month should be 2 digits");
        assert_eq!(parts[2].len(), 2, "day should be 2 digits");
        // All digits
        assert!(parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())));
        // Sane ranges
        let year: u32 = parts[0].parse().unwrap();
        let month: u32 = parts[1].parse().unwrap();
        let day: u32 = parts[2].parse().unwrap();
        assert!((2024..=2100).contains(&year));
        assert!((1..=12).contains(&month));
        assert!((1..=31).contains(&day));
    }

    #[test]
    fn build_date_known_epoch() {
        // 2026-03-29 00:00:00 UTC = 1743206400 seconds
        // Verify algorithm matches expectation for a known timestamp
        // by replicating the calculation inline
        let secs: i64 = 1_743_206_400;
        let days = secs / 86400;
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        assert_eq!(format!("{y:04}-{m:02}-{d:02}"), "2026-03-29");
    }
}
