//! `init` subcommand — generates a starter config TOML in the current
//! working directory.
//!
//! Lives in crap-core (not the per-adapter binary) so every adapter
//! inherits the subcommand for free via `AdapterMeta`. The generator
//! is parameterized on three meta fields:
//!
//! - `config_file_names` — `init` writes the canonical first entry
//!   (`crap.toml` for both adapters); legacy fallbacks are discovery-only
//! - `tool_name` — used in header comments + next-step hint
//! - `default_excludes` — per-ecosystem ignore patterns
//!
//! Auto-detect rules for `src`:
//!   1. `src/` exists → use `"src"` (single-crate Rust, common TS layout)
//!   2. `crates/` exists → use `"crates"` (Cargo workspace)
//!   3. Neither → fall back to `"src"` with a hint comment so users
//!      see the toggle point.
//!
//! Threshold preset: defaults to `default` (15) non-interactively;
//! interactively reads one line from stdin and maps the first char
//! (`s|S` → strict, `l|L` → lenient, anything else → default). No TTY
//! detection — CI users pass `--non-interactive` to skip the prompt;
//! tests pipe input via `Stdio::piped()`.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::cli::AdapterMeta;
use crate::domain::threshold::ThresholdPreset;
use crate::domain::types::ComplexityMetric;

/// Handle the `init` subcommand. Writes a starter config to the canonical
/// config file name in the current directory.
///
/// Returns `Ok(())` on success. Bails with an actionable error message
/// when the file already exists and `--force` was not passed.
pub fn handle_init(force: bool, non_interactive: bool, meta: &AdapterMeta) -> Result<()> {
    handle_init_with_io(
        force,
        non_interactive,
        meta,
        Path::new(meta.canonical_config_file_name()),
        &mut io::stdin().lock(),
        &mut io::stderr(),
    )
}

/// Inner handler that takes the config-file path + I/O streams as
/// parameters so unit tests can drive the prompt + file write against
/// a tempdir without spawning a subprocess.
pub(crate) fn handle_init_with_io<R: BufRead, W: Write>(
    force: bool,
    non_interactive: bool,
    meta: &AdapterMeta,
    config_path: &Path,
    stdin: &mut R,
    stderr: &mut W,
) -> Result<()> {
    if config_path.exists() && !force {
        bail!(
            "{name} already exists in this directory.\n  hint: pass `--force` to overwrite, or edit the existing file directly",
            name = meta.canonical_config_file_name(),
        );
    }

    let preset = if non_interactive {
        ThresholdPreset::Default
    } else {
        prompt_threshold_preset(meta.default_metric, stdin, stderr)?
    };

    let detection = detect_src_layout();
    let content = render_config(meta, preset, &detection);

    fs::write(config_path, &content)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    writeln!(
        stderr,
        "✓ wrote {name} (preset = \"{preset_str}\", src = \"{src}\")",
        name = meta.canonical_config_file_name(),
        preset_str = preset_str(preset),
        src = detection.src_path,
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

/// Result of `detect_src_layout` — the path string we'll write into
/// the TOML, plus whether the value came from a real directory or the
/// fallback. The fallback flag drives the hint comment emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SrcDetection {
    pub src_path: String,
    pub is_fallback: bool,
}

/// Auto-detect the source directory. `src/` wins; `crates/` second;
/// otherwise fall back to `"src"` with `is_fallback = true` so the
/// generator emits a hint comment for the user to adjust.
pub(crate) fn detect_src_layout() -> SrcDetection {
    if Path::new("src").is_dir() {
        SrcDetection {
            src_path: "src".to_string(),
            is_fallback: false,
        }
    } else if Path::new("crates").is_dir() {
        SrcDetection {
            src_path: "crates".to_string(),
            is_fallback: false,
        }
    } else {
        SrcDetection {
            src_path: "src".to_string(),
            is_fallback: true,
        }
    }
}

/// Map a `ThresholdPreset` to the string form `FileConfig` expects in
/// the TOML's `preset = "..."` field (lowercase variant name).
fn preset_str(preset: ThresholdPreset) -> &'static str {
    match preset {
        ThresholdPreset::Strict => "strict",
        ThresholdPreset::Default => "default",
        ThresholdPreset::Lenient => "lenient",
    }
}

/// Render the starter config TOML. Hand-templated rather than serialized
/// via the `toml` crate because the generated file is intentionally
/// commented — `toml::ser` drops comments and we'd have to post-process
/// the output anyway. A constant template with three substitutions
/// (preset, src, excludes) is simpler and gives us complete control
/// over comment placement.
pub(crate) fn render_config(
    meta: &AdapterMeta,
    preset: ThresholdPreset,
    detection: &SrcDetection,
) -> String {
    let mut out = String::with_capacity(1024);

    // Header — anchors generated files so future audits can grep for
    // "generated by `<tool> init`" to find untouched starter configs.
    out.push_str("# ");
    out.push_str(meta.canonical_config_file_name());
    out.push_str(" — generated by `");
    out.push_str(meta.tool_name);
    out.push_str(" init`\n");
    out.push_str("# Edit freely; the analyzer re-reads this file on every run.\n\n");

    // Threshold preset — `preset` and `threshold` are mutually
    // exclusive in `FileConfig`; we emit `preset` (the configurable
    // unit) and let resolution map it to a number. The cutoffs differ
    // by complexity metric (a cyclomatic count and a cognitive count
    // for the same function differ in magnitude), so the numbers shown
    // are this adapter's metric, sourced from the one calibration
    // table — never re-hardcoded here.
    let (strict, default, lenient, metric_name) = preset_display(meta.default_metric);
    out.push_str("# Threshold preset (cutoffs are for the ");
    out.push_str(metric_name);
    out.push_str(" metric):\n");
    out.push_str(&format!(
        "#   strict ({strict})  — high-quality libraries, safety-critical code\n"
    ));
    out.push_str(&format!(
        "#   default ({default}) — typical projects (balanced)\n"
    ));
    out.push_str(&format!(
        "#   lenient ({lenient}) — legacy or transitional code\n"
    ));
    out.push_str("# Use `threshold = N` instead to set a custom numeric cutoff.\n");
    out.push_str("preset = \"");
    out.push_str(preset_str(preset));
    out.push_str("\"\n\n");

    // Source root.
    out.push_str("# Source root the analyzer walks.\n");
    if detection.is_fallback {
        out.push_str("# (auto-detect found no `src/` or `crates/` directory — adjust if your sources live elsewhere)\n");
    }
    out.push_str("src = \"");
    out.push_str(&detection.src_path);
    out.push_str("\"\n\n");

    // Excludes — emitted as a single commented-out array so users can
    // uncomment + tweak in one step. Per-adapter defaults supplied via
    // `AdapterMeta.default_excludes`.
    out.push_str("# Glob patterns matched against project-relative file paths.\n");
    out.push_str("# Uncomment to ignore these directories (one common starting set):\n");
    out.push_str("# exclude = [\n");
    for pattern in meta.default_excludes {
        out.push_str("#     \"");
        out.push_str(pattern);
        out.push_str("\",\n");
    }
    out.push_str("# ]\n");

    out
}

/// The three preset cutoffs plus the metric's display name, derived
/// from the one calibration table. Both the generated-config comment
/// and the interactive prompt render these, so neither re-hardcodes
/// the numbers — a calibration change can never desync the two
/// user-facing surfaces, and a cutoff calibrated for one metric is
/// never shown for the other (whose scores have a different magnitude).
fn preset_display(metric: ComplexityMetric) -> (f64, f64, f64, &'static str) {
    (
        ThresholdPreset::Strict.threshold(metric),
        ThresholdPreset::Default.threshold(metric),
        ThresholdPreset::Lenient.threshold(metric),
        match metric {
            ComplexityMetric::Cyclomatic => "cyclomatic",
            ComplexityMetric::Cognitive => "cognitive",
        },
    )
}

/// Read one line from stdin and map the first character to a
/// `ThresholdPreset`. Empty/whitespace/anything-else → `Default`.
/// EOF (closed stdin) is treated as empty input — Returns `Default`.
/// The displayed cutoffs are calibrated for `metric` (the adapter's
/// default complexity metric), so the interactive numbers match what
/// the generated config will resolve to.
fn prompt_threshold_preset<R: BufRead, W: Write>(
    metric: ComplexityMetric,
    stdin: &mut R,
    stderr: &mut W,
) -> Result<ThresholdPreset> {
    let (strict, default, lenient, metric_name) = preset_display(metric);
    write!(
        stderr,
        "Threshold preset ({metric_name} metric)?\n  (s)trict  = {strict}  high-quality libs\n  (d)efault = {default}  typical projects\n  (l)enient = {lenient}  legacy code\n[d]: "
    )
    .ok();
    stderr.flush().ok();

    let mut buf = String::new();
    stdin
        .read_line(&mut buf)
        .context("failed to read threshold preset from stdin")?;
    Ok(parse_preset_input(&buf))
}

/// Pure parser for the prompt input — exposed for unit testing. Picks
/// up the first non-whitespace character; `s`/`S` → strict, `l`/`L` →
/// lenient, anything else (empty, `d`, garbage) → default.
pub(crate) fn parse_preset_input(input: &str) -> ThresholdPreset {
    match input.trim().chars().next() {
        Some('s' | 'S') => ThresholdPreset::Strict,
        Some('l' | 'L') => ThresholdPreset::Lenient,
        _ => ThresholdPreset::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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
            default_excludes: &["tests/**", "benches/**", "examples/**"],
            // `init` doesn't render forced_excludes (they're load-bearing
            // at analysis time, not template scaffolding); empty here
            // keeps the round-trip assertions focused on what `init`
            // actually emits.
            forced_excludes: &[],
            // Cognitive matches the pre-W2.5 fallthrough — init's
            // round-trip / comment-rendering assertions don't probe
            // the metric default; this keeps the field meaningful
            // without coupling init tests to W2.5 semantics.
            default_metric: crate::domain::types::ComplexityMetric::Cognitive,
        }
    }

    #[test]
    fn parse_preset_input_strict_variants() {
        assert_eq!(parse_preset_input("s"), ThresholdPreset::Strict);
        assert_eq!(parse_preset_input("S"), ThresholdPreset::Strict);
        assert_eq!(parse_preset_input("s\n"), ThresholdPreset::Strict);
        assert_eq!(parse_preset_input("  s  "), ThresholdPreset::Strict);
        assert_eq!(parse_preset_input("strict"), ThresholdPreset::Strict);
    }

    #[test]
    fn parse_preset_input_lenient_variants() {
        assert_eq!(parse_preset_input("l"), ThresholdPreset::Lenient);
        assert_eq!(parse_preset_input("L"), ThresholdPreset::Lenient);
        assert_eq!(parse_preset_input("lenient"), ThresholdPreset::Lenient);
    }

    #[test]
    fn parse_preset_input_defaults_on_empty_or_garbage() {
        assert_eq!(parse_preset_input(""), ThresholdPreset::Default);
        assert_eq!(parse_preset_input("\n"), ThresholdPreset::Default);
        assert_eq!(parse_preset_input("   "), ThresholdPreset::Default);
        assert_eq!(parse_preset_input("d"), ThresholdPreset::Default);
        assert_eq!(parse_preset_input("D"), ThresholdPreset::Default);
        assert_eq!(parse_preset_input("xyz"), ThresholdPreset::Default);
        assert_eq!(parse_preset_input("42"), ThresholdPreset::Default);
    }

    #[test]
    fn render_config_includes_preset_and_src() {
        let meta = fake_meta();
        let detection = SrcDetection {
            src_path: "src".to_string(),
            is_fallback: false,
        };
        let out = render_config(&meta, ThresholdPreset::Strict, &detection);
        assert!(out.contains("preset = \"strict\""), "preset line missing");
        assert!(out.contains("src = \"src\""), "src line missing");
    }

    #[test]
    fn render_config_emits_fallback_hint_only_when_detection_failed() {
        let meta = fake_meta();
        let detected = SrcDetection {
            src_path: "src".to_string(),
            is_fallback: false,
        };
        let fallback = SrcDetection {
            src_path: "src".to_string(),
            is_fallback: true,
        };
        let with_detect = render_config(&meta, ThresholdPreset::Default, &detected);
        let with_fallback = render_config(&meta, ThresholdPreset::Default, &fallback);
        assert!(!with_detect.contains("adjust if your sources live elsewhere"));
        assert!(with_fallback.contains("adjust if your sources live elsewhere"));
    }

    #[test]
    fn render_config_emits_commented_excludes_from_meta() {
        let meta = fake_meta();
        let detection = SrcDetection {
            src_path: "src".to_string(),
            is_fallback: false,
        };
        let out = render_config(&meta, ThresholdPreset::Default, &detection);
        assert!(out.contains("# exclude = ["));
        assert!(out.contains("tests/**"));
        assert!(out.contains("benches/**"));
        assert!(out.contains("examples/**"));
    }

    #[test]
    fn render_config_includes_header_and_threshold_descriptions() {
        let meta = fake_meta();
        let detection = SrcDetection {
            src_path: "src".to_string(),
            is_fallback: false,
        };
        let out = render_config(&meta, ThresholdPreset::Default, &detection);
        // Header line names the meta's canonical config name verbatim —
        // pattern stays generic so we don't hardcode adapter names in
        // crap-core.
        let expected_header = format!("# {}", meta.canonical_config_file_name());
        assert!(
            out.contains(&expected_header),
            "header line missing; got:\n{out}",
        );
        assert!(out.contains("Threshold preset"));
        assert!(out.contains("strict (8)"));
        assert!(out.contains("default (15)"));
        assert!(out.contains("lenient (25)"));
    }

    #[test]
    fn prompt_reads_strict_from_piped_stdin() {
        let mut stdin = Cursor::new(b"s\n");
        let mut stderr: Vec<u8> = Vec::new();
        let preset =
            prompt_threshold_preset(ComplexityMetric::Cognitive, &mut stdin, &mut stderr).unwrap();
        assert_eq!(preset, ThresholdPreset::Strict);
    }

    #[test]
    fn prompt_defaults_when_stdin_is_empty() {
        let mut stdin = Cursor::new(b"");
        let mut stderr: Vec<u8> = Vec::new();
        let preset =
            prompt_threshold_preset(ComplexityMetric::Cognitive, &mut stdin, &mut stderr).unwrap();
        assert_eq!(preset, ThresholdPreset::Default);
    }

    #[test]
    fn prompt_numbers_track_the_metric() {
        // The interactive prompt must show the cutoffs that route via
        // the adapter's metric. Both columns are flat-equal post-#272
        // (8/15/25); the prompt still emits the cyclomatic-column
        // values, proving the metric-keyed routing path is exercised
        // even when columns agree.
        let mut stdin = Cursor::new(b"\n");
        let mut stderr: Vec<u8> = Vec::new();
        prompt_threshold_preset(ComplexityMetric::Cyclomatic, &mut stdin, &mut stderr).unwrap();
        let shown = String::from_utf8(stderr).unwrap();
        assert!(shown.contains("cyclomatic metric"), "prompt: {shown}");
        assert!(shown.contains("(s)trict  = 8"), "prompt: {shown}");
        assert!(shown.contains("(d)efault = 15"), "prompt: {shown}");
        assert!(shown.contains("(l)enient = 25"), "prompt: {shown}");
    }

    #[test]
    fn handle_init_writes_default_config_in_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("crap4rs.toml");
        let meta = fake_meta();
        let mut stdin = Cursor::new(b"");
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(false, true, &meta, &path, &mut stdin, &mut stderr).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("preset = \"default\""));
        assert!(content.contains("src = \"src\""));
    }

    #[test]
    fn handle_init_bails_when_file_exists_without_force() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("crap4rs.toml");
        fs::write(&path, "preset = \"lenient\"\n").unwrap();
        let meta = fake_meta();
        let mut stdin = Cursor::new(b"");
        let mut stderr: Vec<u8> = Vec::new();
        let err = handle_init_with_io(false, true, &meta, &path, &mut stdin, &mut stderr)
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
        let path = tmp.path().join("crap4rs.toml");
        fs::write(&path, "preset = \"lenient\"\n").unwrap();
        let meta = fake_meta();
        let mut stdin = Cursor::new(b"");
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(true, true, &meta, &path, &mut stdin, &mut stderr).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("preset = \"default\""));
        assert!(!content.contains("preset = \"lenient\""));
    }

    #[test]
    fn handle_init_interactive_reads_preset_from_stdin() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("crap4rs.toml");
        let meta = fake_meta();
        let mut stdin = Cursor::new(b"s\n");
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(false, false, &meta, &path, &mut stdin, &mut stderr).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("preset = \"strict\""), "got: {content}");
    }

    #[test]
    fn generated_config_round_trips_through_loader() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("crap4rs.toml");
        let meta = fake_meta();
        let mut stdin = Cursor::new(b"");
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(false, true, &meta, &path, &mut stdin, &mut stderr).unwrap();
        let config =
            crate::adapters::config::load_config(&path).expect("init's generated TOML must load");
        assert_eq!(
            config.preset,
            Some(ThresholdPreset::Default),
            "loaded preset should match"
        );
        assert_eq!(config.src.as_deref(), Some(Path::new("src")));
    }
}
