//! Sync guard for the committed JSON Schema artifact (`crap.schema.json`).
//!
//! `crap.schema.json` at the repo root is the editor `$schema` target
//! for `crap.toml`. It is **generated** from the annotated
//! [`crap_core::adapters::config::ConfigSchema`] type via
//! `config_json_schema()`, never hand-edited. This test asserts the
//! committed file is byte-identical to a fresh regeneration, so the
//! schema can never silently drift from the config type (the field set,
//! the `///` descriptions). "Documentation rots; CI doesn't."
//!
//! To regenerate after a deliberate schema change, run:
//!   `cargo run -p crap-core --bin crap-render -- --emit-config-schema`
//! is NOT the path — instead run the helper that writes the file (see
//! the failure message), or regenerate inline:
//!   write `config_json_schema()` output to `crap.schema.json` at the
//!   repo root and commit it alongside the schema-type change.

use std::path::PathBuf;

/// Resolve the repo-root `crap.schema.json`. The crate manifest dir is
/// `<repo>/crates/crap-core`, so the artifact is two levels up.
fn committed_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("crap.schema.json")
}

#[test]
fn committed_schema_matches_generated() {
    let path = committed_schema_path();
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read committed schema at {}: {e}\n  \
             hint: regenerate it by writing `crap_core::adapters::config::config_json_schema()` \
             output to crap.schema.json at the repo root and committing it",
            path.display()
        )
    });
    let generated = crap_core::adapters::config::config_json_schema();
    assert_eq!(
        committed.trim_end(),
        generated.trim_end(),
        "crap.schema.json is stale.\n  \
         fix: regenerate crap.schema.json from ConfigSchema (write \
         `crap_core::adapters::config::config_json_schema()` output to the repo-root \
         crap.schema.json) and commit it in the same change that touched the config type."
    );
}

#[test]
fn generated_schema_has_nonempty_descriptions_for_documented_fields() {
    // Doc-completeness on the schema side: every documented field must
    // surface a non-empty `description`. Spot-check the top-level fields
    // that carry `///` docs (the full per-field completeness sweep over
    // the rendered example lands with the example generator in #347).
    let schema = crap_core::adapters::config::config_json_schema();
    for needle in [
        "Custom numeric CRAP cutoff.",
        "Named threshold preset",
        "Complexity metric",
        "Source root(s) the analyzer walks.",
        "Per-language override sections",
        "Output-shaping settings",
    ] {
        assert!(
            schema.contains(needle),
            "schema missing expected field description {needle:?}:\n{schema}"
        );
    }
}
