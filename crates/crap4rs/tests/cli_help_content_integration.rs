//! `--help` content integration tests (#161).
//!
//! Verifies that the `ABOUT` / `LONG_ABOUT` strings declared in
//! `crap4rs/src/main.rs` actually reach clap's help output via
//! `AdapterMeta`. A regression here means the threading from binary
//! `const` → `AdapterMeta` → `clap::Command::about/long_about` is
//! broken — a silent failure that only surfaces when a human runs
//! `crap4rs --help` and notices the wrong copy.

use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

fn run_help(flag: &str) -> String {
    let out = Command::new(BINARY)
        .arg(flag)
        .output()
        .expect("failed to run crap4rs binary");
    // Without this guard, a non-zero exit (e.g., clap parse failure
    // from a future regression) would surface as a confusing
    // "expected substring not found" downstream rather than the
    // actual command failure.
    assert!(
        out.status.success(),
        "crap4rs {flag} exited with {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn short_help_contains_about_string() {
    // `-h` triggers clap's short help, which uses `about`.
    let out = run_help("-h");
    assert!(
        out.contains("CRAP score analyzer for Rust"),
        "short help must contain ABOUT from main.rs, got:\n{out}"
    );
}

#[test]
fn long_help_contains_long_about_phrase() {
    // `--help` triggers clap's long help, which uses `long_about`.
    // Anchor on a distinctive phrase from LONG_ABOUT that wouldn't
    // appear in the default clap-derive help text.
    let out = run_help("--help");
    assert!(
        out.contains("Change Risk Anti-Patterns") && out.contains("cognitive complexity"),
        "long help must contain LONG_ABOUT from main.rs, got:\n{out}"
    );
}

#[test]
fn long_help_contains_after_help_examples() {
    // `--help` should also append the AFTER_HELP block with EXAMPLES.
    let out = run_help("--help");
    assert!(
        out.contains("EXAMPLES:") && out.contains("crap4rs --coverage lcov.info"),
        "long help must contain AFTER_HELP EXAMPLES from main.rs, got:\n{out}"
    );
}
