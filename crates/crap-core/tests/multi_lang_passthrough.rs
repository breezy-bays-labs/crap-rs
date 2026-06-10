//! Byte-identity smoke for the multi-language renderer's single-
//! language passthrough invariant (crap-rs#315).
//!
//! When `crap-render` is invoked with exactly one envelope,
//! `format_html_multi` short-circuits to `format_html`. The output
//! must be byte-identical to what the underlying adapter binary
//! produces via `--format html` on the same analysis. This file
//! locks that invariant in-process at the library level — the BDD
//! harness covers the CLI shell.
//!
//! In-process check (not shell-out): build a synthetic
//! `AnalysisResult` once, then assert
//! `format_html_multi(MultiLangContext::single(block), threshold, opts) ==
//!   format_html(&view, None, threshold, &meta, metric)`
//! byte-for-byte. Avoids the temp-file dance the CLI test needs and
//! is the rigorous library-level invariant.

use crap_core::adapters::reporters::test_fixtures::{
    TEST_TOOL_VERSION, make_multi_function_result, make_view_default,
};
use crap_core::adapters::reporters::{HtmlMultiOptions, format_html, format_html_multi};
use crap_core::cli::AdapterMeta;
use crap_core::core::compose::compose_multi_lang;
use crap_core::domain::multi_lang::LanguageBlock;
use crap_core::domain::types::ComplexityMetric;

/// Mirror of the in-module `test_meta` helper in
/// `crates/crap-core/src/adapters/reporters/html.rs`; reproduced
/// here because the tests/* crate cannot import private helpers.
fn test_meta() -> AdapterMeta {
    AdapterMeta {
        tool_name: "test-adapter",
        display_name: "Test",
        tool_version: TEST_TOOL_VERSION,
        long_version: TEST_TOOL_VERSION,
        about: "test",
        long_about: "test",
        after_help: "",
        coverage_hint: "test",
        extensions: &["rs"],
        tool_info_uri: "https://example.com/test-adapter",
        rule_help_uri: "https://example.com/test-adapter#crap",
        config_file_names: &["test-adapter.toml"],
        config_lang_key: "test",
        default_excludes: &[],
        forced_excludes: &[],
        default_metric: ComplexityMetric::Cognitive,
    }
}

#[test]
fn single_language_passthrough_is_byte_identical_to_direct_format_html() {
    let result = make_multi_function_result();
    let view = make_view_default(&result);

    // Direct path: what every existing single-binary consumer of
    // `--format html` sees today.
    // `None, None` for `[output]` title/subtitle: the multi-language
    // passthrough never threads a scorecard label (crap-rs#352, D20), so
    // the direct comparison must also pass `None` to stay byte-identical.
    let direct = format_html(
        &view,
        None,
        8.0,
        &test_meta(),
        ComplexityMetric::Cognitive,
        None,
        None,
    );

    // Multi-language passthrough: construct a single-element
    // `MultiLangContext` and route through `format_html_multi`.
    // `format_html_multi`'s short-circuit on `multi.languages.len() == 1`
    // must produce the exact same string.
    let block = LanguageBlock {
        tool_name: "test-adapter".to_string(),
        display_name: "Test".to_string(),
        language: "rust".to_string(),
        tool_version: TEST_TOOL_VERSION.to_string(),
        metric: ComplexityMetric::Cognitive,
        threshold: 8.0,
        view: make_view_default(&result),
        delta: None,
        delta_disabled_reason: None,
    };
    let multi = compose_multi_lang(vec![block]);
    let unified = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());

    assert_eq!(
        direct, unified,
        "single-language passthrough must be byte-identical to format_html — back-compat invariant"
    );
}
