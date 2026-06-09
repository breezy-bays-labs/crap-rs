//! `crap-render` — multi-language CRAP HTML renderer.
//!
//! Composes per-adapter JSON envelopes into a unified HTML report
//! with a Language/Combined toggle and a Current/Delta View axis.
//! Used by the composite scorecard action in multi-language mode;
//! also invocable manually for debugging.
//!
//! ## CLI shape
//!
//! ```text
//! crap-render --input <LANG>=<FILE> [--input <LANG>=<FILE>...] \
//!             [--baseline <LANG>=<FILE> ...]                   \
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
//! `--baseline <LANG>=<FILE>` is the optional baseline counterpart.
//! Each baseline envelope is composed against the matching `--input`
//! envelope (same `<LANG>` key) to produce a per-language delta. When
//! supplied, the rendered HTML carries the Current/Delta View axis
//! within each language panel and a cross-adapter Combined Delta
//! ranking; languages without a baseline render the Delta tab
//! disabled, signalling the asymmetric state to reviewers without
//! suppressing other languages' deltas.
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
//! collision. The same guard applies to baseline envelopes.

use std::fs::{self, File};
use std::io::BufReader;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use clap::{ArgAction, Parser, ValueEnum};
use serde::Deserialize;

use crap_core::adapters::reporters::{HtmlMultiOptions, format_html_multi};
use crap_core::core::compose::compose_multi_lang;
use crap_core::domain::delta::{self, AnalysisDelta, DeltaViewSpec};
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

    /// Optional baseline envelope paired with its language key,
    /// formatted as `<LANG>=<FILE>`. When supplied, the matching
    /// language (by `<LANG>` key) gains a Current/Delta View axis in
    /// its panel and contributes to the cross-adapter Combined Delta
    /// ranking. A baseline whose language key has no matching
    /// `--input` is an error. Languages without a baseline still
    /// render normally; their Delta tab is disabled with a tooltip
    /// pointing reviewers to supply one.
    #[arg(long = "baseline", action = ArgAction::Append)]
    baselines: Vec<String>,

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
    /// Effective threshold-border epsilon carried from the envelope
    /// (crap-rs#379). Applied when this envelope is the *input* side of a
    /// recomputed delta, so the combined panel's border-jitter signal
    /// matches the gate that produced the envelope. `0.0` ⇒ band off.
    epsilon: f64,
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
    /// Effective threshold-border epsilon the emitting run was configured
    /// with (crap-rs#379). `serde(default)` → `0.0` for envelopes that omit
    /// it (every pre-#379 envelope, and any epsilon-off run), so the
    /// recomputed delta stays byte-identical to the old behavior there.
    #[serde(default)]
    epsilon: f64,
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
    // Parse + validate every `--input` and `--baseline` spec up front.
    // Extracted so `run` itself stays a flat compose-and-render
    // pipeline (the validation loops carried the bulk of its cognitive
    // complexity).
    let (parsed, baselines) = parse_and_validate_envelopes(&cli)?;

    let threshold = cli.threshold.unwrap_or_else(|| {
        parsed
            .iter()
            .map(|e| e.threshold)
            .fold(f64::NEG_INFINITY, f64::max)
            .max(0.0)
    });

    // Compose per-language `AnalysisDelta`s ahead of `LanguageBlock`
    // construction so the rendered `DeltaView`s can borrow into
    // owned storage that lives at least as long as the render call.
    // The deltas vector is kept in 1:1 order alignment with
    // `parsed`; `None` means this language has no baseline (Delta
    // tab will render disabled in the report).
    let analysis_deltas: Vec<Option<AnalysisDelta>> = parsed
        .iter()
        .map(|env| {
            baselines
                .iter()
                .find(|b| b.language == env.language)
                // Recompute the delta carrying the *input* envelope's
                // effective epsilon (crap-rs#379) — the current run's
                // configured border band, not the baseline's. With epsilon
                // threaded, the recomputed `DeltaSummary.border_jitter_active`
                // / `border_jitter_suppressed` match the gate that produced
                // the envelope, so the combined panel can show an active band
                // even at zero suppressed. `epsilon == 0.0` (the common case)
                // is byte-identical to the old bare `delta::compute`.
                .map(|b| {
                    delta::compute_with_epsilon(b.result.clone(), env.result.clone(), env.epsilon)
                })
        })
        .collect();

    // Build LanguageBlock list. The view + delta lifetimes borrow
    // from each ParsedEnvelope's result; we therefore keep `parsed`
    // and `analysis_deltas` alive for the duration of rendering.
    let blocks: Vec<LanguageBlock<'_>> = parsed
        .iter()
        .zip(analysis_deltas.iter())
        .map(|(env, maybe_delta)| LanguageBlock {
            tool_name: tool_name_for_language(&env.language).to_string(),
            display_name: display_name_for_language(&env.language).to_string(),
            language: env.language.clone(),
            tool_version: env.tool_version.clone(),
            metric: env.metric,
            threshold: env.threshold,
            view: view::apply(&env.result, ViewSpec::default()),
            delta: maybe_delta
                .as_ref()
                .map(|d| delta::apply(d, DeltaViewSpec::default())),
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

/// Parse and validate every `--input` / `--baseline` spec.
///
/// Returns the parsed input envelopes and baseline envelopes. Enforces,
/// in order: at least one `--input`; no duplicate input language key;
/// no duplicate baseline language key; every baseline pairs with an
/// input by language key. Kept separate from `run` so the orchestrator
/// stays a flat pipeline — the error strings are stable and pinned by
/// the `crap_render_cli` characterization tests.
fn parse_and_validate_envelopes(cli: &Cli) -> Result<(Vec<ParsedEnvelope>, Vec<ParsedEnvelope>)> {
    if cli.inputs.is_empty() {
        bail!("at least one --input <LANG>=<FILE> argument is required");
    }

    let parsed = parse_all_specs(&cli.inputs, "input")?;
    guard_unique_languages(&parsed, "input")?;

    // Parse optional baselines. Each baseline pairs by `<LANG>` key
    // with one of the `--input` envelopes; a baseline whose key has
    // no matching input is an operator error (typo / wrong file).
    let baselines = parse_all_specs(&cli.baselines, "baseline")?;
    guard_unique_languages(&baselines, "baseline")?;
    guard_baselines_pair_with_inputs(&baselines, &parsed)?;

    Ok((parsed, baselines))
}

/// Parse every `<LANG>=<FILE>` spec in `specs` into a `ParsedEnvelope`.
/// `kind` (`"input"` / `"baseline"`) flows into per-spec error messages
/// so the operator sees which flag to fix.
fn parse_all_specs(specs: &[String], kind: &str) -> Result<Vec<ParsedEnvelope>> {
    let mut parsed: Vec<ParsedEnvelope> = Vec::with_capacity(specs.len());
    for spec in specs {
        parsed.push(parse_input_spec(spec, kind)?);
    }
    Ok(parsed)
}

/// Refuse a baseline whose language key has no matching `--input`. A
/// baseline only composes against an input of the same key, so an
/// orphan baseline is an operator error (typo / wrong file).
fn guard_baselines_pair_with_inputs(
    baselines: &[ParsedEnvelope],
    inputs: &[ParsedEnvelope],
) -> Result<()> {
    for env in baselines {
        if !inputs.iter().any(|e| e.language == env.language) {
            bail!(
                "--baseline for language '{}' has no matching --input. Each baseline must pair with an input by language key.",
                env.language
            );
        }
    }
    Ok(())
}

/// Refuse two envelopes for the same language key. `kind` is `"input"`
/// or `"baseline"`, naming the offending stream in the error so the
/// operator knows which flag to fix (this catches the common mistake of
/// passing two crap4rs.json by accident).
fn guard_unique_languages(envelopes: &[ParsedEnvelope], kind: &str) -> Result<()> {
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for env in envelopes {
        if !seen.insert(env.language.as_str()) {
            bail!(
                "duplicate {kind} for language '{}'. Each language key may appear at most once.",
                env.language
            );
        }
    }
    Ok(())
}

fn parse_input_spec(spec: &str, kind: &str) -> Result<ParsedEnvelope> {
    let (language, path_str) = spec
        .split_once('=')
        .ok_or_else(|| anyhow!("invalid --{kind} spec '{spec}': expected '<LANG>=<FILE>'"))?;
    let language = language.trim();
    let path = PathBuf::from(path_str.trim());

    if language.is_empty() {
        bail!("invalid --{kind} spec '{spec}': language key must not be empty");
    }
    if path.as_os_str().is_empty() {
        bail!("invalid --{kind} spec '{spec}': file path must not be empty");
    }

    // Stream the envelope via BufReader so very large analyses don't
    // require loading the full file into memory before parsing.
    // `from_reader` reads forward through the JSON document; on a
    // typical workspace this is functionally equivalent to slurping
    // but bounds peak memory in the high-input case.
    let file =
        File::open(&path).with_context(|| format!("opening envelope at {}", path.display()))?;
    let envelope: WireEnvelope = serde_json::from_reader(BufReader::new(file))
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
        epsilon: envelope.epsilon,
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
        let err = parse_input_spec("crap4rs.json", "input").unwrap_err();
        assert!(err.to_string().contains("expected '<LANG>=<FILE>'"));
    }

    #[test]
    fn parse_input_spec_rejects_empty_language() {
        let err = parse_input_spec("=crap4rs.json", "input").unwrap_err();
        assert!(err.to_string().contains("language key must not be empty"));
    }

    #[test]
    fn parse_input_spec_rejects_empty_path() {
        let err = parse_input_spec("rust=", "input").unwrap_err();
        assert!(err.to_string().contains("file path must not be empty"));
    }

    /// Error messages use the supplied kind ("input" / "baseline")
    /// so operators see the exact flag they need to fix.
    #[test]
    fn parse_input_spec_error_messages_reference_supplied_kind() {
        let err = parse_input_spec("missing_equals", "baseline").unwrap_err();
        assert!(
            err.to_string().contains("--baseline"),
            "error should reference the baseline flag, got: {err}"
        );
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
