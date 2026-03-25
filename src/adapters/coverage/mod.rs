//! LCOV coverage parser adapter.
//!
//! Parses `cargo-llvm-cov --lcov` output into per-file, per-line hit data.
//! Only uses SF (source file) and DA (line data) records — FN/FNDA records
//! are ignored because function matching uses line ranges from syn, not
//! LCOV function names (which are mangled Rust symbols).

// TODO: Session 2 — implement LCOV parser
