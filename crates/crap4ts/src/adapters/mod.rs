//! crap4ts adapters — TypeScript-specific bindings for the language-
//! agnostic `crap_core` analyzer.
//!
//! Two adapter modules live here:
//!
//! - **`walker`**: `oxc`-based AST walker that extracts per-function
//!   cyclomatic complexity from TypeScript and JavaScript sources.
//! - **`coverage`**: Istanbul JSON coverage parser
//!   (`coverage-final.json`) that converts jest / vitest / nyc coverage
//!   into `ParseOutput<IstanbulParseDiagnostic>`.
//!
//! Both modules are TypeScript / oxc-toolchain coupled and live in
//! `crap4ts/src/adapters/` rather than `crap-core` — see the
//! `ast-purity` CI gate that bans `oxc` imports from `crap-core/src/`.

pub mod coverage;
pub mod walker;
