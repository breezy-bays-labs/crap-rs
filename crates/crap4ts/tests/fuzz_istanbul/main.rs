//! Q4 fuzz: panic-freedom of the Istanbul JSON coverage parser over arbitrary
//! input — a raw-bytes arm and a structured arm.
//!
//! Same stable, no-nightly model as `fuzz_syn_walker` / `fuzz_oxc_walker` (see
//! those headers for the bolero-on-stable rationale and the explicit-corpus-
//! replay workaround). All arms run as ordinary tests on the existing nextest
//! lane.
//!
//! Robustness contract (every arm): `parse_str` must never PANIC (or abort) on
//! its input. `Err` (a serde_json parse failure on non-JSON bytes, or a
//! recursion-limit rejection on deeply nested input) is correct, expected
//! behavior — only a panic/abort is a bug.
//!
//! - **Raw-bytes arm** (`fuzz_istanbul_raw`): feeds arbitrary bytes through
//!   `parse_str`. Exercises the three-path parse cascade and the inherited
//!   serde_json nesting-recursion guard on both the typed and `Value` seams.
//!   Most random inputs fail at the JSON parse gate (an `Err`), so this arm is
//!   mostly a parser-robustness check.
//! - **Structured arm** (`fuzz_istanbul_structured`): generates a
//!   schema-shaped `HashMap<String, GenFile>` (mirroring the parser's
//!   `IstanbulCoverageFile` deserialization shape), serializes it to JSON, and
//!   feeds *that* to `parse_str`. Because the input parses, this arm reaches
//!   PAST the JSON gate into the coverage-mapping logic the raw arm starves:
//!   the `with_capacity` allocation, the `s`/`statementMap` join, the branch
//!   fan-out, and path normalization. Generation uses bolero's native
//!   `TypeGenerator` derive (no extra `arbitrary` dependency — the derive ships
//!   in bolero's prelude and is the idiomatic generator for this fuzzer).
//!
//! Boundary-Rule note: the crap4ts Istanbul parser surface has ZERO existing
//! property/fuzz tests (only example-based unit + cucumber coverage), so both
//! arms are purely additive — they do not duplicate any lower-level invariant.

use std::collections::HashMap;

use serde::Serialize;

use crap4ts::adapters::coverage::IstanbulCoverage;

fn parser() -> IstanbulCoverage {
    // Root is the crate dir (an absolute path, matching production where
    // `--src` is canonicalized) so the prefix-strip / suffix-match path
    // normalization is actually exercised rather than short-circuited by a
    // relative root.
    IstanbulCoverage::new(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

/// Drive raw bytes through the parser. `Ok`/`Err` both acceptable; only a
/// panic/abort is a bug.
fn drive(input: &[u8]) {
    let src = String::from_utf8_lossy(input);
    let _ = parser().parse_str(&src);
}

// ── Structured generation: wrapper types mirroring the parser's
// `IstanbulCoverageFile` deserialization shape (camelCase on the wire), so a
// generated value serializes to JSON that `parse_str` accepts and maps.
// `serde`/`serde_json` are crap4ts `[dependencies]`, available to this test.

#[derive(Debug, bolero::TypeGenerator, Serialize)]
struct GenPosition {
    line: u32,
}

#[derive(Debug, bolero::TypeGenerator, Serialize)]
struct GenStatementLoc {
    start: GenPosition,
}

#[derive(Debug, bolero::TypeGenerator, Serialize)]
struct GenBranchLoc {
    loc: GenStatementLoc,
    line: Option<u32>,
}

#[derive(Debug, bolero::TypeGenerator, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenFile {
    path: String,
    s: HashMap<String, u64>,
    statement_map: HashMap<String, GenStatementLoc>,
    b: HashMap<String, Vec<u64>>,
    branch_map: HashMap<String, GenBranchLoc>,
}

/// Serialize a generated Istanbul-shaped map and drive it through the parser.
fn drive_structured(files: &HashMap<String, GenFile>) {
    if let Ok(json) = serde_json::to_string(files) {
        let _ = parser().parse_str(&json);
    }
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

#[test]
fn fuzz_istanbul_structured() {
    bolero::check!()
        .with_type::<HashMap<String, GenFile>>()
        .for_each(drive_structured);
}
