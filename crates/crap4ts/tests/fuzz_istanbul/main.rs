//! Q4 fuzz: panic-freedom of the Istanbul JSON coverage parser over arbitrary
//! input (raw-bytes arm).
//!
//! Same stable, no-nightly model as `fuzz_syn_walker` / `fuzz_oxc_walker` (see
//! those headers for the bolero-on-stable rationale and the explicit-corpus-
//! replay workaround). `parse_str` is metric-agnostic, so this is a single
//! fuzz target plus the deterministic corpus-replay regression test.
//!
//! Robustness contract: `parse_str` must never PANIC (or abort) on arbitrary
//! input. `Err` (e.g. a serde_json parse failure on non-JSON bytes, or a
//! recursion-limit rejection on deeply nested input) is correct, expected
//! behavior — only a panic/abort is a bug. The raw arm exercises the
//! three-path parse cascade and the inherited (but never directly asserted)
//! serde_json nesting-recursion guard on both the typed and `Value` seams.
//!
//! Boundary-Rule note: the crap4ts Istanbul parser surface has ZERO existing
//! property/fuzz tests (only example-based unit + cucumber coverage), so this
//! target is purely additive — it does not duplicate any lower-level
//! invariant. (The structured `#[derive(Arbitrary)]` arm that reaches the
//! coverage-mapping / OOM / traversal logic past the JSON parse gate lands in
//! the follow-up sub-issue.)

use crap4ts::adapters::coverage::IstanbulCoverage;

/// Drive one input through the Istanbul parser. The contract is
/// panic-freedom: `Ok` and `Err` are both acceptable; only a panic/abort is a
/// bug. The root is a fixed sentinel — `parse_str` does no filesystem I/O, so
/// the path only feeds in-memory `SF`/path normalization.
fn drive(input: &[u8]) {
    let src = String::from_utf8_lossy(input);
    let parser = IstanbulCoverage::new(std::path::PathBuf::from("."));
    let _ = parser.parse_str(&src);
}

/// Deterministic per-PR regression net: replay every committed corpus seed and
/// crash regression through the parser.
#[test]
fn replay_committed_corpus_and_crashes() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fuzz_istanbul");
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
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
            {
                continue;
            }
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|e| panic!("failed to read seed {}: {e}", path.display()));
            drive(&bytes);
            replayed += 1;
        }
    }
    assert!(
        replayed >= 4,
        "expected >= 4 committed corpus/crash seeds to replay, found {replayed}"
    );
}

#[test]
fn fuzz_istanbul_raw() {
    bolero::check!().for_each(|input: &[u8]| drive(input));
}
