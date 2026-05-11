//! crap4ts adapters — TypeScript-specific bindings for the language-
//! agnostic `crap_core` analyzer.
//!
//! Two stubs live here; both runtime-panic via `unimplemented!()` and
//! will be filled in by the follow-up TypeScript-adapter pipeline:
//!
//! - **`walker`**: `oxc`-based AST walker that will extract per-function
//!   cognitive / cyclomatic complexity from `.ts`/`.tsx` sources.
//! - **`coverage`**: Istanbul JSON coverage parser
//!   (`coverage-final.json`) that will convert vitest / jest coverage
//!   into `ParseOutput<IstanbulParseDiagnostic>`.
//!
//! Both modules are TypeScript / oxc-toolchain coupled and live in
//! `crap4ts/src/adapters/` rather than `crap-core` — see the
//! `ast-purity` CI gate that bans `oxc` imports from `crap-core/src/`.

pub mod coverage;
pub mod walker;
