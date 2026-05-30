//! Guards for the committed annotated example (`crap.example.toml`).
//!
//! `crap.example.toml` at the repo root is the exhaustive, fully-annotated
//! config reference. It is **generated** from the annotated
//! [`crap_core::adapters::config::ConfigSchema`] type via
//! `render_example_config()` (the same function `crap4rs init` /
//! `crap4ts init` write verbatim), never hand-edited, and is NOT loaded by
//! the tool — it exists purely as the canonical option reference.
//!
//! Three guards, one per failure mode:
//!   1. **sync** — the committed file is byte-identical to a fresh render
//!      (catches "committed copy went stale vs the generator").
//!   2. **round-trip** — the rendered example parses, with every optional
//!      field present (except exactly one of the mutually-exclusive
//!      `{preset, threshold}` pair, at every level) and every collection
//!      non-empty (catches "a field was wired into the type but is missing
//!      from the rendered output").
//!   3. **doc-completeness** — every schema field's `///` prose appears as
//!      a comment in the rendered output (catches "a field renders with no
//!      annotation" — which sync + round-trip both miss identically).

use std::path::PathBuf;

use crap_core::adapters::config::{all_schema_field_docs, load_config, render_example_config};
use crap_core::cli::AdapterMeta;
use crap_core::domain::types::ComplexityMetric;

/// A rust-flavored adapter meta — the committed `crap.example.toml` is
/// `render_example_config(<rust meta>)` (breadboard D1). `config_lang_key`
/// drives which `[language.<name>]` section the example highlights.
///
/// MUST mirror the real `crap4rs` `AdapterMeta` (`crates/crap4rs/src/main.rs`)
/// on the three fields `render_example_config` actually reads —
/// `tool_name` ("crap4rs"), `config_file_names[0]` ("crap.toml"), and
/// `default_excludes` — so the committed example equals what `crap4rs init`
/// would write. If crap4rs's `default_excludes` ever changes, update this
/// fixture (and regenerate `crap.example.toml`) in the same change.
fn rust_meta() -> AdapterMeta {
    AdapterMeta {
        tool_name: "crap4rs",
        display_name: "Rust",
        tool_version: "0.0.0",
        long_version: "0.0.0",
        about: "",
        long_about: "",
        after_help: "",
        coverage_hint: "",
        extensions: &["rs"],
        tool_info_uri: "https://example.invalid",
        rule_help_uri: "https://example.invalid",
        config_file_names: &["crap.toml", "crap4rs.toml"],
        config_lang_key: "rust",
        default_excludes: &["tests/**", "benches/**", "examples/**"],
        forced_excludes: &[],
        default_metric: ComplexityMetric::Cognitive,
    }
}

/// Resolve the repo-root `crap.example.toml`. The crate manifest dir is
/// `<repo>/crates/crap-core`, so the artifact is two levels up.
fn committed_example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("crap.example.toml")
}

#[test]
fn committed_example_matches_generated() {
    let path = committed_example_path();
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read committed example at {}: {e}\n  \
             hint: regenerate it — write `render_example_config(<rust meta>)` output to \
             crap.example.toml at the repo root and commit it",
            path.display()
        )
    });
    let generated = render_example_config(&rust_meta());
    assert_eq!(
        committed, generated,
        "crap.example.toml is stale.\n  \
         fix: regenerate it (render `render_example_config()` with the rust adapter meta \
         to crap.example.toml at the repo root) and commit it in the same change that \
         touched the config schema."
    );
}

#[test]
fn generated_example_round_trips_with_every_field_populated() {
    // The example must parse, and the parse must show every optional field
    // present (except one of the mutually-exclusive {preset, threshold}
    // pair) and every collection non-empty. This catches a field wired
    // into the type but absent from the rendered output. We assert the
    // *parsed projection* (FileConfig), not equality with the default.
    let rendered = render_example_config(&rust_meta());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crap.example.toml");
    std::fs::write(&path, &rendered).unwrap();
    let parsed = load_config(&path).expect("the exhaustive example must parse without error");

    // Top level: threshold live, preset NOT (mutual exclusion → preset is a
    // commented alternative, so it parses to None by design).
    assert!(
        parsed.threshold.is_some(),
        "threshold must be live in the example"
    );
    assert!(
        parsed.preset.is_none(),
        "preset is the commented alternative to threshold (mutually exclusive)"
    );
    assert!(parsed.metric.is_some(), "metric must be set");
    assert!(!parsed.src.is_empty(), "src must be non-empty");
    assert!(
        parsed.exclude.as_ref().is_some_and(|e| !e.is_empty()),
        "exclude must be a non-empty list"
    );
    assert!(
        !parsed.overrides.is_empty(),
        "overrides must be non-empty (at least one [[overrides]])"
    );
    assert!(!parsed.views.is_empty(), "views must be non-empty");
    assert!(!parsed.language.is_empty(), "language must be non-empty");

    // Output table: every field present.
    assert!(parsed.output.annotation_limit.is_some());
    assert!(parsed.output.title.is_some());
    assert!(parsed.output.subtitle.is_some());

    // Each view preset: every optional field present, coverage range valid.
    for (name, vp) in &parsed.views {
        assert!(vp.top.is_some(), "views.{name}.top");
        assert!(vp.min_coverage.is_some(), "views.{name}.min_coverage");
        assert!(vp.max_coverage.is_some(), "views.{name}.max_coverage");
        assert!(vp.sort.is_some(), "views.{name}.sort");
        assert!(vp.only_failing.is_some(), "views.{name}.only_failing");
        assert!(vp.no_fail.is_some(), "views.{name}.no_fail");
        assert!(vp.group_by.is_some(), "views.{name}.group_by");
        assert!(vp.minimal_view.is_some(), "views.{name}.minimal_view");
        assert!(
            vp.min_coverage.unwrap() <= vp.max_coverage.unwrap(),
            "views.{name}: min_coverage must not exceed max_coverage"
        );
    }

    // Each language section: metric + exclude present; exactly one of
    // {preset, threshold} set (recursive mutual-exclusion carve-out).
    for (name, lc) in &parsed.language {
        assert!(lc.metric.is_some(), "language.{name}.metric");
        assert!(
            lc.exclude.as_ref().is_some_and(|e| !e.is_empty()),
            "language.{name}.exclude non-empty"
        );
        assert!(
            lc.threshold.is_some() ^ lc.preset.is_some(),
            "language.{name}: exactly one of {{preset, threshold}} must be set"
        );
    }
}

#[test]
fn every_schema_field_doc_appears_in_generated_example() {
    // Doc-completeness: every field's `///` prose (the single doc source)
    // must surface as a comment in the rendered example. Without this,
    // sync + round-trip both miss an *absent* comment identically (a field
    // could render with no annotation). The render walks the schema's
    // `documented` docs; this asserts each one reaches the output.
    let rendered = render_example_config(&rust_meta());
    for (label, doc) in all_schema_field_docs() {
        // The doc may be multi-line; assert the first line (the part that
        // becomes the leading comment's first line) is present.
        let first_line = doc.lines().next().unwrap_or("").trim();
        assert!(
            !first_line.is_empty(),
            "schema field {label} has an empty `///` doc — every field must be documented"
        );
        assert!(
            rendered.contains(first_line),
            "schema field {label} doc not found in rendered example.\n  \
             missing prose: {first_line:?}\n  \
             every field's `///` must appear as a comment in crap.example.toml"
        );
    }
}
