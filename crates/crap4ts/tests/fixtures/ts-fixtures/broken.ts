// W1.2 parse-failure fixture: deliberately malformed TS to trigger
// `Err(CrapError::SourceParse)`. The orchestrator at
// crap-core/src/core/mod.rs:286-310 catches this and increments
// AnalysisDiagnostics.files_unparseable so the run continues.
//
// Multiple unclosed brace levels make this irrecoverable for the
// oxc parser, so `ret.errors` is guaranteed non-empty (a single
// `function foo(` could recover via tail-of-file synthesis).
function foo(x: number {{{
