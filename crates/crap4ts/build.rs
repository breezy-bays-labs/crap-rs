//! Build script — runs the napi-rs build hook AND embeds git commit hash
//! + build date into the binary.
//!
//! Two responsibilities:
//!
//! 1. `napi_build::setup()` — emits per-platform link directives
//!    required for the napi-rs cdylib. Notably on macOS this prints
//!    `cargo:rustc-cdylib-link-arg=-undefined dynamic_lookup`, which
//!    is what lets the `.node`/`.dylib` link against Node-provided
//!    `napi_*` symbols at runtime instead of at link time.
//! 2. `CRAP4TS_LONG_VERSION` — same shape as crap4rs's `build.rs`.
//!    Clap picks this up via `long_version = env!("CRAP4TS_LONG_VERSION")`
//!    and displays it for `--version`.
//!
//! Output examples:
//!   - Source build with git: `2.0.0-alpha.1 (abc1234 2026-05-10)`
//!   - Built without git:     `2.0.0-alpha.1 (2026-05-10)`

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

fn build_date_from_secs(secs: i64) -> String {
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

fn build_date() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    build_date_from_secs(secs)
}

fn format_long_version(version: &str, hash: &str, date: &str) -> String {
    if hash.is_empty() {
        format!("{version} ({date})")
    } else {
        format!("{version} ({hash} {date})")
    }
}

fn main() {
    // napi-rs cdylib link-arg setup. Must run first so the printed
    // `cargo::` directives reach cargo before any subsequent prints.
    napi_build::setup();

    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let long_version = format_long_version(&version, &git_hash(), &build_date());

    println!("cargo:rustc-env=CRAP4TS_LONG_VERSION={long_version}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/");
}

// ── Unit tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_long_version_with_hash() {
        let v = format_long_version("2.0.0-alpha.1", "abc1234", "2026-05-10");
        assert_eq!(v, "2.0.0-alpha.1 (abc1234 2026-05-10)");
    }

    #[test]
    fn format_long_version_empty_hash_returns_date_only() {
        let v = format_long_version("2.0.0-alpha.1", "", "2026-05-10");
        assert_eq!(v, "2.0.0-alpha.1 (2026-05-10)");
    }

    #[test]
    fn build_date_known_epoch() {
        // 2026-05-10 00:00:00 UTC = 1746835200 seconds
        assert_eq!(build_date_from_secs(1_746_835_200), "2026-05-10");
    }
}
