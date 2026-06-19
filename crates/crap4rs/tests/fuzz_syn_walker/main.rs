//! Q4 fuzz: panic-freedom of the syn complexity walker over arbitrary source.
//!
//! Two complementary layers, both running on **stable** under the existing
//! `cargo nextest run --workspace --all-targets` lane — no nightly, no
//! sanitizer, no `cargo-bolero` binary:
//!
//! 1. `replay_committed_corpus_and_crashes` — a deterministic regression net.
//!    It reads every committed `tests/fuzz_syn_walker/{corpus,crashes}` seed
//!    (via `CARGO_MANIFEST_DIR`, so the path is correct in a workspace) and
//!    drives each through the walker for both metrics. This is what guarantees
//!    a discovered-and-fixed crash STAYS fixed: the minimized input lands in
//!    `crashes/` in the same PR as its fix and is replayed on every run
//!    thereafter. (bolero's own `check!` corpus auto-discovery does NOT fire
//!    under plain libtest/nextest — its `is_harnessed()` keys off the libtest
//!    thread name and routes corpus lookup to an internal `__fuzz__` path used
//!    only by the `cargo-bolero` CLI — so we replay the seeds explicitly.)
//! 2. `fuzz_syn_walker_{cognitive,cyclomatic}` — bolero `check!` targets doing
//!    bounded randomized generation (`BOLERO_RANDOM_ITERATIONS`, pinned in CI).
//!    These are the discovery layer and escalate, unchanged, to coverage-guided
//!    libFuzzer fuzzing under the optional nightly `cargo bolero fuzz` cron
//!    lane (which DOES consume the on-disk corpus/crashes dirs).
//!
//! Robustness contract under test: the walker must never PANIC (or abort via
//! stack overflow) on arbitrary input. Returning `Err` (e.g. a syn parse
//! failure on non-Rust bytes) is correct, expected behavior — only a
//! panic/abort is a bug. We therefore drive the `Result`-returning
//! `ComplexityPort::extract` seam directly and discard the `Result`; we do NOT
//! use the `extract_src` test helper, which `.unwrap()`s and would conflate a
//! benign parse `Err` with a crash.
//!
//! Boundary-Rule note: this ADDS coverage the existing `proptest` suite cannot
//! reach. `no_panic_on_fixture_files` (adapters/complexity/mod.rs) is
//! fixture-vocabulary bounded; this target explores arbitrary bytes and the
//! deep-nesting recursion regime (mutually-recursive cognitive/cyclomatic
//! counters + syn's own recursive `full`-feature parse) that fixtures never
//! exercise.

use crap4rs::adapters::complexity::SynComplexityAdapter;
use crap4rs::domain::types::ComplexityMetric;
use crap4rs::ports::ComplexityPort;

/// Drive one input through the walker for one metric. The contract is
/// panic-freedom: `Ok` and `Err` are both acceptable outcomes; only a
/// panic/abort is a bug.
fn drive(input: &[u8], metric: ComplexityMetric) {
    let src = String::from_utf8_lossy(input);
    let _ = SynComplexityAdapter::new().extract(&src, "fuzz.rs", metric);
}

fn drive_both(input: &[u8]) {
    drive(input, ComplexityMetric::Cognitive);
    drive(input, ComplexityMetric::Cyclomatic);
}

/// Deterministic per-PR regression net: replay every committed corpus seed and
/// crash regression through the walker. Runs on stable as an ordinary test.
#[test]
fn replay_committed_corpus_and_crashes() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fuzz_syn_walker");
    let mut replayed = 0usize;
    for sub in ["corpus", "crashes"] {
        let dir = base.join(sub);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            // Skip dotfiles (.gitkeep) like bolero's own corpus loader does.
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
            {
                continue;
            }
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|e| panic!("failed to read seed {}: {e}", path.display()));
            drive_both(&bytes);
            replayed += 1;
        }
    }
    // The committed corpus is hand-seeded, so at least the basic + deep-nesting
    // seeds must be present — a zero here means the seeds went missing.
    assert!(
        replayed >= 2,
        "expected >= 2 committed corpus/crash seeds to replay, found {replayed}"
    );
}

#[test]
fn fuzz_syn_walker_cognitive() {
    bolero::check!().for_each(|input: &[u8]| drive(input, ComplexityMetric::Cognitive));
}

#[test]
fn fuzz_syn_walker_cyclomatic() {
    bolero::check!().for_each(|input: &[u8]| drive(input, ComplexityMetric::Cyclomatic));
}
