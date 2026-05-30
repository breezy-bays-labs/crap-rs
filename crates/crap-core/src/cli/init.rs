//! `init` subcommand — writes the exhaustive annotated config to the
//! current working directory.
//!
//! Lives in crap-core (not the per-adapter binary) so every adapter
//! inherits the subcommand for free via `AdapterMeta`. `init` writes the
//! output of [`crate::adapters::config::render_example_config`] verbatim
//! — the same full-annotated-dump that the committed `crap.example.toml`
//! reference is generated from (a byte-identical sync test keeps the two
//! aligned). The file documents every supported option; users trim it
//! down to their real config.
//!
//! `init` is **non-interactive**: the rendered config is the same
//! deterministic, exhaustive document regardless of flags. (It previously
//! prompted for a threshold preset and auto-detected `src`; the
//! full-dump model — every option present and annotated — replaced the
//! trimmed-starter model in crap-rs#347, so there is nothing left to
//! prompt for.) The `--non-interactive` flag is retained at the CLI for
//! back-compat but no longer changes the output.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::adapters::config::render_example_config;
use crate::cli::AdapterMeta;

/// Handle the `init` subcommand. Writes the exhaustive annotated config to
/// the canonical config file name in the current directory.
///
/// Returns `Ok(())` on success. Bails with an actionable error message
/// when the file already exists and `--force` was not passed.
pub fn handle_init(force: bool, _non_interactive: bool, meta: &AdapterMeta) -> Result<()> {
    handle_init_with_io(
        force,
        meta,
        Path::new(meta.canonical_config_file_name()),
        &mut io::stderr(),
    )
}

/// Inner handler that takes the config-file path + stderr stream as
/// parameters so unit tests can drive the file write against a tempdir
/// without spawning a subprocess.
pub(crate) fn handle_init_with_io<W: Write>(
    force: bool,
    meta: &AdapterMeta,
    config_path: &Path,
    stderr: &mut W,
) -> Result<()> {
    if config_path.exists() && !force {
        bail!(
            "{name} already exists in this directory.\n  hint: pass `--force` to overwrite, or edit the existing file directly",
            name = meta.canonical_config_file_name(),
        );
    }

    let content = render_example_config(meta);

    fs::write(config_path, &content)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    writeln!(
        stderr,
        "✓ wrote {name} (exhaustive annotated config — trim it down to your needs)",
        name = meta.canonical_config_file_name(),
    )
    .ok();
    writeln!(
        stderr,
        "  next: ensure your coverage tool is installed, then run `{name} --help` to see analysis flags.",
        name = meta.tool_name,
    )
    .ok();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::threshold::ThresholdPreset;

    // Generic fake adapter — kept adapter-name-agnostic to satisfy the
    // ast-purity layer 4 gate (which structurally forbids "crap4rs" /
    // "crap4ts" string literals anywhere in crap-core source). The
    // `default_excludes` set mirrors the Rust adapter's because the
    // round-trip + comment-rendering assertions are independent of
    // which patterns are listed; only the count/presence matters.
    fn fake_meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "fake-adapter",
            display_name: "Fake",
            tool_version: "0.5.0",
            long_version: "0.5.0",
            about: "test",
            long_about: "test",
            after_help: "",
            coverage_hint: "test",
            extensions: &["rs"],
            tool_info_uri: "https://example.invalid",
            rule_help_uri: "https://example.invalid",
            config_file_names: &["fake-adapter.toml"],
            config_lang_key: "fake",
            default_excludes: &["tests/**", "benches/**", "examples/**"],
            forced_excludes: &[],
            default_metric: crate::domain::types::ComplexityMetric::Cognitive,
        }
    }

    #[test]
    fn handle_init_writes_exhaustive_annotated_config_in_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fake-adapter.toml");
        let meta = fake_meta();
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(false, &meta, &path, &mut stderr).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        // The full dump is the render_example_config output verbatim.
        assert_eq!(content, render_example_config(&meta));
        // Header banner names the canonical config + tool.
        assert!(content.contains("exhaustive annotated config reference"));
        assert!(content.contains("fake-adapter.toml"));
    }

    #[test]
    fn handle_init_output_is_exhaustive_and_annotated() {
        let meta = fake_meta();
        let out = render_example_config(&meta);
        // Live threshold; preset shown as the commented alternative.
        assert!(out.contains("threshold = 15.0"));
        assert!(out.contains("# preset = \"default\""));
        // Multi-root src array + adapter-flavored excludes.
        assert!(out.contains("src = ["));
        assert!(out.contains("tests/**"));
        // Per-language + output sections all present.
        assert!(out.contains("[language.rust]"));
        assert!(out.contains("[language.typescript]"));
        assert!(out.contains("[output]"));
        assert!(out.contains("title ="));
    }

    #[test]
    fn handle_init_bails_when_file_exists_without_force() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fake-adapter.toml");
        fs::write(&path, "preset = \"lenient\"\n").unwrap();
        let meta = fake_meta();
        let mut stderr: Vec<u8> = Vec::new();
        let err = handle_init_with_io(false, &meta, &path, &mut stderr)
            .expect_err("init should bail when file exists without --force");
        let msg = format!("{err:#}");
        let expected = format!("{} already exists", meta.canonical_config_file_name());
        assert!(msg.contains(&expected), "got: {msg}");
        assert!(msg.contains("--force"), "got: {msg}");
        // file content unchanged
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "preset = \"lenient\"\n");
    }

    #[test]
    fn handle_init_overwrites_with_force() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fake-adapter.toml");
        fs::write(&path, "preset = \"lenient\"\n").unwrap();
        let meta = fake_meta();
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(true, &meta, &path, &mut stderr).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, render_example_config(&meta));
        assert!(!content.contains("preset = \"lenient\""));
    }

    #[test]
    fn generated_config_round_trips_through_loader() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fake-adapter.toml");
        let meta = fake_meta();
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(false, &meta, &path, &mut stderr).unwrap();
        let config =
            crate::adapters::config::load_config(&path).expect("init's generated TOML must load");
        // The exhaustive dump sets threshold live (preset is the commented
        // alternative) and a multi-root src; it round-trips cleanly.
        assert!(config.threshold.is_some());
        assert_eq!(config.preset, None::<ThresholdPreset>);
        assert!(!config.src.is_empty());
        assert!(!config.language.is_empty());
    }
}
