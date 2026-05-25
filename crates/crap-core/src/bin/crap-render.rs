//! `crap-render` — multi-language CRAP HTML renderer.
//!
//! Composes per-adapter JSON envelopes into a unified HTML report
//! with a Language/Combined toggle. Used by the composite scorecard
//! action in multi-language mode; also invocable manually for
//! debugging.
//!
//! ## CLI shape
//!
//! ```text
//! crap-render --input <LANG>=<FILE> [--input <LANG>=<FILE>...] \
//!             [--format html] [--output <PATH>] [--threshold <N>]
//! ```
//!
//! `--input <LANG>=<FILE>` pairs each envelope path with the language
//! key it represents (`rust`, `typescript`, etc.). The language
//! identity is supplied via CLI rather than read from the envelope
//! because the JSON wire shape's `language` field is currently
//! hard-coded "rust" by every emitting binary (a pre-existing bug
//! tracked elsewhere) — pairing the language at the CLI sidesteps
//! the field and stays N-adapter agnostic for future adapters.
//!
//! ## Schema-version validation
//!
//! Each envelope's `schema_version` is validated on parse. The
//! renderer accepts `schema_version ∈ {1, 2}` (mirrors the baseline
//! loader's accepted range in `adapters::baseline`). Out-of-range
//! values fail fast with an actionable error message naming the
//! envelope path and the offending value.
//!
//! ## Duplicate adapter guard
//!
//! Two envelopes with the same `(tool_name, language)` tuple are
//! refused — accidentally passing two `crap4rs.json` would otherwise
//! double-render the same data; the error message points at the
//! collision.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use clap::{ArgAction, Parser, ValueEnum};
use serde::Deserialize;

use crap_core::adapters::reporters::{HtmlMultiOptions, format_html_multi};
use crap_core::core::compose::compose_multi_lang;
use crap_core::domain::multi_lang::LanguageBlock;
use crap_core::domain::types::{AnalysisResult, ComplexityMetric};
use crap_core::domain::view::{self, ViewSpec};

/// Schema versions this renderer accepts on input envelopes.
///
/// Mirrors the baseline loader's range
/// (`adapters::baseline::SUPPORTED_SCHEMA_VERSIONS`); when production
/// envelopes bump past this range, the renderer fails fast with an
/// actionable error rather than producing a mangled combined view.
const SUPPORTED_SCHEMA_VERSIONS: &[u32] = &[1, 2];

#[derive(Parser, Debug)]
#[command(
    name = "crap-render",
    about = "Compose per-language CRAP analysis envelopes into a unified HTML report",
    version
)]
struct Cli {
    /// Input envelope paired with its language key, formatted as
    /// `<LANG>=<FILE>` (e.g. `rust=crap4rs.json`). Specify multiple
    /// times for multi-language reports. The language key drives the
    /// segmented Language nav and URL hash routing in the rendered
    /// HTML.
    #[arg(long = "input", action = ArgAction::Append, required = true)]
    inputs: Vec<String>,

    /// Output format. Only `html` is supported in v0.7.0; the option
    /// exists so future formats (markdown, json wrappers) extend the
    /// CLI without a breaking change.
    #[arg(long, default_value = "html", value_enum)]
    format: Format,

    /// Path to write the rendered output. Omit to write to stdout.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Workspace-level CRAP threshold echoed in the scope banner and
    /// fallback for per-adapter threshold display when an envelope
    /// did not carry one. Defaults to the maximum across all
    /// per-envelope thresholds.
    #[arg(long)]
    threshold: Option<f64>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    Html,
}

/// Parsed envelope ready for composition.
#[derive(Debug)]
struct ParsedEnvelope {
    language: String,
    result: AnalysisResult,
    metric: ComplexityMetric,
    threshold: f64,
    tool_version: String,
}

/// Minimal envelope shape for deserialization. Mirrors the relevant
/// subset of `JsonEnvelope` in `crap-core::adapters::reporters::json`
/// — we only read the fields the renderer needs.
#[derive(Deserialize)]
struct WireEnvelope {
    schema_version: u32,
    #[serde(default)]
    tool_version: String,
    metric: ComplexityMetric,
    threshold: f64,
    result: AnalysisResult,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    if cli.inputs.is_empty() {
        bail!("at least one --input <LANG>=<FILE> argument is required");
    }

    let mut parsed: Vec<ParsedEnvelope> = Vec::with_capacity(cli.inputs.len());
    for spec in &cli.inputs {
        parsed.push(parse_input_spec(spec)?);
    }

    // Duplicate-language guard: refuse two envelopes for the same
    // language key. This catches the common operator mistake of
    // passing two crap4rs.json by accident.
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for env in &parsed {
        if !seen.insert(env.language.as_str()) {
            bail!(
                "duplicate input for language '{}'. Each language key may appear at most once.",
                env.language
            );
        }
    }

    let threshold = cli.threshold.unwrap_or_else(|| {
        parsed
            .iter()
            .map(|e| e.threshold)
            .fold(f64::NEG_INFINITY, f64::max)
            .max(0.0)
    });

    // Build LanguageBlock list. The view + delta lifetimes borrow
    // from each ParsedEnvelope's result; we therefore keep `parsed`
    // alive for the duration of rendering.
    let blocks: Vec<LanguageBlock<'_>> = parsed
        .iter()
        .map(|env| LanguageBlock {
            tool_name: tool_name_for_language(&env.language).to_string(),
            display_name: display_name_for_language(&env.language).to_string(),
            language: env.language.clone(),
            tool_version: env.tool_version.clone(),
            metric: env.metric,
            threshold: env.threshold,
            view: view::apply(&env.result, ViewSpec::default()),
            delta: None,
        })
        .collect();

    let multi = compose_multi_lang(blocks);

    let rendered = match cli.format {
        Format::Html => format_html_multi(&multi, threshold, HtmlMultiOptions::default()),
    };

    match cli.output {
        Some(path) => fs::write(&path, rendered)
            .with_context(|| format!("writing rendered output to {}", path.display()))?,
        None => print!("{rendered}"),
    }

    Ok(())
}

fn parse_input_spec(spec: &str) -> Result<ParsedEnvelope> {
    let (language, path_str) = spec
        .split_once('=')
        .ok_or_else(|| anyhow!("invalid --input spec '{spec}': expected '<LANG>=<FILE>'"))?;
    let language = language.trim();
    let path = PathBuf::from(path_str.trim());

    if language.is_empty() {
        bail!("invalid --input spec '{spec}': language key must not be empty");
    }
    if path.as_os_str().is_empty() {
        bail!("invalid --input spec '{spec}': file path must not be empty");
    }

    let json = fs::read_to_string(&path)
        .with_context(|| format!("reading envelope at {}", path.display()))?;
    let envelope: WireEnvelope = serde_json::from_str(&json)
        .with_context(|| format!("parsing JSON envelope at {}", path.display()))?;

    if !SUPPORTED_SCHEMA_VERSIONS.contains(&envelope.schema_version) {
        bail!(
            "envelope {} has schema_version {}, expected one of {:?}. Upgrade the emitting adapter or downgrade crap-render to a version that supports this schema.",
            path.display(),
            envelope.schema_version,
            SUPPORTED_SCHEMA_VERSIONS
        );
    }

    Ok(ParsedEnvelope {
        language: language.to_string(),
        result: envelope.result,
        metric: envelope.metric,
        threshold: envelope.threshold,
        tool_version: envelope.tool_version,
    })
}

/// Conventional tool name for a language key. Used to populate
/// `LanguageBlock.tool_name` when the envelope's `tool_name` field
/// is absent (which is every envelope today — the wire shape
/// doesn't yet carry tool_name; tracked separately).
fn tool_name_for_language(language: &str) -> &str {
    match language {
        "rust" => "crap4rs",
        "typescript" => "crap4ts",
        // Future adapters: the convention is `crap4<key>`. Unknown
        // languages fall through to the raw key so the renderer
        // doesn't paper over identity drift.
        other => other,
    }
}

/// Human-readable display label for a language key.
fn display_name_for_language(language: &str) -> &str {
    match language {
        "rust" => "Rust",
        "typescript" => "TypeScript",
        // Capitalize the first letter for unknown keys so the
        // fallback at least looks like a proper noun.
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_input_spec_rejects_missing_equals() {
        let err = parse_input_spec("crap4rs.json").unwrap_err();
        assert!(err.to_string().contains("expected '<LANG>=<FILE>'"));
    }

    #[test]
    fn parse_input_spec_rejects_empty_language() {
        let err = parse_input_spec("=crap4rs.json").unwrap_err();
        assert!(err.to_string().contains("language key must not be empty"));
    }

    #[test]
    fn parse_input_spec_rejects_empty_path() {
        let err = parse_input_spec("rust=").unwrap_err();
        assert!(err.to_string().contains("file path must not be empty"));
    }

    #[test]
    fn tool_name_mapping_handles_known_languages() {
        assert_eq!(tool_name_for_language("rust"), "crap4rs");
        assert_eq!(tool_name_for_language("typescript"), "crap4ts");
    }

    #[test]
    fn tool_name_mapping_passes_through_unknown_languages() {
        assert_eq!(tool_name_for_language("go"), "go");
    }

    #[test]
    fn display_name_mapping_passes_through_unknown_languages() {
        assert_eq!(display_name_for_language("go"), "go");
    }
}
