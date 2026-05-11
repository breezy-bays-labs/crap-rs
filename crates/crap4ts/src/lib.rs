//! crap4ts — TypeScript adapter binding for the language-agnostic
//! `crap_core` analyzer.
//!
//! ALPHA: the oxc complexity walker and Istanbul JSON coverage parser
//! are stubs that `unimplemented!()`. The real TS pipeline lands in a
//! follow-up; this crate ships the structural shell so the workspace
//! layout, cdylib + bin crate-type combo, license allowlist, and CI
//! surface are settled before the walker work begins.
//!
//! Unlike `crap4rs`, this crate has no v0.4 history and therefore no
//! backward-compat shim modules — its public surface starts fresh at
//! v2.0.0-alpha.1.

pub mod adapters;
pub mod parse_diagnostic;

// ── napi-rs cdylib entry point ───────────────────────────────────────
//
// Behind the `napi-binding` feature so the standalone `crap4ts` bin
// (which links the lib via `use crap4ts::...`) doesn't pull in
// unresolved Node-provided `_napi_*` symbols at link time.
// `napi_build`'s macOS `-undefined dynamic_lookup` directive only
// covers the cdylib crate-type — the bin target would fail to link
// otherwise. Cdylib consumers build with `cargo build --package
// crap4ts --features napi-binding`.
//
// A single exported function exists so the `.node` artifact produced
// by the cdylib has something to bind to (and `cargo build --features
// napi-binding` exercises the napi_derive proc-macro chain). No
// analysis logic is wired through napi yet — real bindings ship with
// the walker.

#[cfg(feature = "napi-binding")]
use napi_derive::napi;

/// Returns a human-readable alpha-status string. Exposed to JS as
/// `alphaStatus()` (napi-rs converts snake_case → camelCase) when the
/// `napi-binding` feature is enabled. Always callable from Rust so
/// non-napi consumers (and tests) can verify the message.
#[cfg(feature = "napi-binding")]
#[napi]
pub fn alpha_status() -> String {
    alpha_status_message().to_string()
}

/// Backing constant for the alpha-status message. Lifted out so the
/// non-napi path (the bin's default build, unit tests) can assert on
/// the same string without touching napi at all.
pub const fn alpha_status_message() -> &'static str {
    "crap4ts@2 alpha — oxc walker not yet implemented"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_status_message_is_stable() {
        assert_eq!(
            alpha_status_message(),
            "crap4ts@2 alpha — oxc walker not yet implemented"
        );
    }
}
