//! Core orchestration — language-agnostic pieces that anchor the
//! analyzer pipeline. Today this is just the filesystem walker; S4 will
//! relocate the generic `analyze<P: ParseDiagnostic>` orchestrator here
//! when the Rust adapter's `core::mod` is parameterized away from
//! `LcovParseDiagnostic`.

pub mod walker;
