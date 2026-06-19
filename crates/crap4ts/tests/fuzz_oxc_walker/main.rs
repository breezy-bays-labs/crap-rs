//! Q4 fuzz: robustness of the oxc TS/JS walker over arbitrary source.
//!
//! Same stable, no-nightly model as `fuzz_syn_walker` (see that target's
//! header for the bolero-on-stable rationale and the explicit-corpus-replay
//! workaround). Two layers, both on the existing nextest lane:
//!
//! 1. `replay_committed_corpus_and_crashes` — deterministic regression net
//!    over the committed `tests/fuzz_oxc_walker/{corpus,crashes}` seeds.
//! 2. `fuzz_oxc_walker` — bolero `check!` bounded-random discovery, escalating
//!    unchanged to the nightly `cargo bolero fuzz` lane.
//!
//! crap4ts 2.0.0 ships `--metric cyclomatic` only (`Cognitive` returns
//! `MetricNotSupported` before the parser runs), so the lane drives Cyclomatic
//! — the metric that actually walks. Contracts under test:
//! - **Panic-freedom.** The walker must never panic or abort on arbitrary
//!   input. `Err` (parse failure on non-TS bytes) is acceptable; only a
//!   panic/abort is a bug. This covers the span→line/column conversion crash
//!   class the survey rated HIGH (cf. the existing
//!   `unicode_identifiers_do_not_panic_in_span_to_column_conversion` smoke).
//! - **Well-formedness** (hard structural invariants, not heuristics):
//!   complexity >= 1, 1-based non-inverted spans, contributor increment >= 1,
//!   and every contributor line within its function's `[start_line, end_line]`
//!   span. A violation is a real walker bug (misattribution, inverted/OOB
//!   span, or a bad span→line conversion), exactly the class fuzzing exists to
//!   surface — not a false positive. If a future input trips it, the fix + the
//!   minimized crash seed land in the same PR.
//!
//! Boundary-Rule note: `walker_proptest.rs` is a *correctness oracle* over a
//! constrained, always-parseable grammar (`prop_recursive(4, 24, 4)`) that
//! returns early on rejected input. It cannot reach arbitrary bytes, recursion
//! past oxc's parser guard, or inverted/OOB spans — this target does.

use crap_core::domain::types::{ComplexityMetric, FunctionComplexity};
use crap_core::ports::ComplexityPort;
use crap4ts::adapters::walker::OxcWalker;

/// Assert structural well-formedness of every discovered function. These are
/// hard invariants of the walker's output, not heuristics — a failure is a
/// genuine walker defect (misattribution, an inverted/OOB span, or a
/// zero/negative line from span→line conversion), exactly the class fuzzing
/// exists to surface.
fn assert_well_formed(funcs: &[FunctionComplexity]) {
    for f in funcs {
        let name = &f.identity.qualified_name;
        // Domain invariant: complexity is always >= 1.
        assert!(
            f.complexity >= 1,
            "complexity {} < 1 for `{name}`",
            f.complexity
        );
        let start = f.identity.span.start_line;
        let end = f.identity.span.end_line;
        // 1-based, non-inverted span.
        assert!(start >= 1, "start_line {start} < 1 for `{name}`");
        assert!(end >= start, "inverted span [{start}, {end}] for `{name}`");
        for c in &f.contributors {
            // Cyclomatic contributors always add exactly 1 — never 0.
            assert!(
                c.increment >= 1,
                "contributor increment {} < 1 for `{name}`",
                c.increment
            );
            // Keystone: every contributor sits inside its function's span.
            assert!(
                c.line >= start && c.line <= end,
                "contributor at line {} falls outside fn span [{start}, {end}] for `{name}`",
                c.line,
            );
        }
    }
}

/// Drive one input through the walker. crap4ts 2.0.0 ships `--metric
/// cyclomatic` only — `Cognitive` returns `MetricNotSupported` immediately and
/// never reaches the parser/walker, so fuzzing it would be a no-op. We drive
/// Cyclomatic, the metric that actually parses + walks. `.tsx` selects the
/// JSX-capable `SourceType` (the widest grammar). Panic-freedom is implicit
/// (any panic fails the test); well-formedness is checked on success.
fn drive(input: &[u8]) {
    let src = String::from_utf8_lossy(input);
    if let Ok(funcs) = OxcWalker::new().extract(&src, "fuzz.tsx", ComplexityMetric::Cyclomatic) {
        assert_well_formed(&funcs);
    }
}

/// Deterministic per-PR regression net: replay every committed corpus seed and
/// crash regression through the walker.
#[test]
fn replay_committed_corpus_and_crashes() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fuzz_oxc_walker");
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
        replayed >= 3,
        "expected >= 3 committed corpus/crash seeds to replay, found {replayed}"
    );
}

#[test]
fn fuzz_oxc_walker() {
    bolero::check!().for_each(|input: &[u8]| drive(input));
}
