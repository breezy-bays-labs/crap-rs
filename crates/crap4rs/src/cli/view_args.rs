//! CLI → ViewSpec adapter.
//!
//! V1b wires `--only-failing` through `Filters::only_failing`. W2 layers
//! `--min/--max-coverage` through `Filters::coverage_range` (this module);
//! `--top` and `--sort-by` follow without further changes to
//! `cli/mod.rs::run_inner`.

use anyhow::{Result, bail};

use super::{Cli, GroupByArg, SortKeyArg};
use crap4rs::adapters::config::{FileConfig, ViewPreset};
use crap4rs::domain::view::{CoverageRange, CoverageRangeError, ViewSpec};
// `GroupKey` flows from CLI to `ViewSpec` via `cli::GroupByArg::Into<GroupKey>`.

/// Build a `ViewSpec` from the parsed CLI.
///
/// `Filters` and `ViewSpec` are `#[non_exhaustive]` (per ADR D3 — they
/// reserve namespace for future filters/sort keys), so we mutate fields
/// on a default rather than using a struct literal.
///
/// Assumes `validate_view_args` already ran and accepted the input —
/// the `CoverageRange::new` call below cannot fail in `run_inner`.
pub(super) fn build_view_spec(cli: &Cli) -> ViewSpec {
    let mut spec = ViewSpec::default();
    spec.filters.only_failing = cli.filter.only_failing;
    if let Some((lo, hi)) = resolve_coverage_bounds(cli) {
        spec.filters.coverage_range = CoverageRange::new(lo, hi).ok();
    }
    // `Some(0)` and `None` are both "no limit" (per `domain::view::truncate_to`);
    // canonicalise at the boundary so JSON consumers see effective behaviour
    // rather than the user's literal input.
    spec.limit = cli.filter.top.and_then(|n| (n > 0).then_some(n as usize));
    spec.sort = cli.filter.sort_by.map(Into::into).unwrap_or_default();
    spec.group_by = cli.filter.group_by.map(Into::into);
    spec
}

/// Validate view-specific args and translate domain errors to flag-attributed
/// CLI prose.
///
/// `CoverageRange::new` (in `domain::view`) is the single source of truth for
/// validity rules. We pre-check each bound for OutOfRange so the message can
/// name the offending flag, then rely on `CoverageRange::new` for the
/// relational `MinExceedsMax` check.
pub(super) fn validate_view_args(cli: &Cli) -> Result<()> {
    let Some((lo, hi)) = resolve_coverage_bounds(cli) else {
        return Ok(());
    };

    if cli
        .filter
        .min_coverage
        .is_some_and(|v| !(0.0..=100.0).contains(&v))
    {
        bail!("--min-coverage must be in [0, 100]");
    }
    if cli
        .filter
        .max_coverage
        .is_some_and(|v| !(0.0..=100.0).contains(&v))
    {
        bail!("--max-coverage must be in [0, 100]");
    }

    match CoverageRange::new(lo, hi) {
        Ok(_) => Ok(()),
        Err(CoverageRangeError::MinExceedsMax { .. }) => {
            bail!("--min-coverage must not exceed --max-coverage")
        }
        // OutOfRange is unreachable: per-bound checks above handle it. The
        // wildcard catches future `#[non_exhaustive]` variants too — domain
        // errors must be translated to flag-attributed prose at the CLI edge.
        Err(e) => bail!("invalid --min-coverage / --max-coverage: {e}"),
    }
}

/// Returns the resolved `(min, max)` pair iff at least one bound was passed.
/// Unspecified sides default to 0.0 / 100.0 — the natural domain of the
/// percentage scale (cli_ergonomics.feature:79-86).
fn resolve_coverage_bounds(cli: &Cli) -> Option<(f64, f64)> {
    match (cli.filter.min_coverage, cli.filter.max_coverage) {
        (None, None) => None,
        (lo, hi) => Some((lo.unwrap_or(0.0), hi.unwrap_or(100.0))),
    }
}

/// Resolve `--view <NAME>` against `file_config.views`, then apply.
///
/// - If `--view` was not passed, returns `Ok(())` (no-op).
/// - If `--view ci` was passed but no config file is loaded OR the config
///   has no preset by that name: bail with `exit 2` ("unknown preset")
///   listing the available preset names. Empty list yields a hint that
///   `crap4rs.toml` defines no `[views]` blocks.
pub(super) fn resolve_view_preset(cli: &mut Cli, file_config: Option<&FileConfig>) -> Result<()> {
    let Some(name) = cli.input.view.clone() else {
        return Ok(());
    };
    let views = file_config.map(|c| &c.views);
    let preset = views.and_then(|v| v.get(&name));
    match preset {
        Some(preset) => {
            apply_preset_to_cli(cli, preset);
            Ok(())
        }
        None => bail!("{}", unknown_preset_message(&name, views)),
    }
}

fn unknown_preset_message(
    name: &str,
    views: Option<&std::collections::HashMap<String, ViewPreset>>,
) -> String {
    match views {
        Some(map) if !map.is_empty() => {
            let mut names: Vec<&str> = map.keys().map(String::as_str).collect();
            names.sort_unstable();
            format!(
                "unknown view preset `{name}`\n  available presets: {}",
                names.join(", ")
            )
        }
        Some(_) => format!(
            "unknown view preset `{name}`\n  hint: crap4rs.toml defines no [views.<name>] blocks"
        ),
        None => format!(
            "unknown view preset `{name}`\n  hint: --view requires a crap4rs.toml with a [views.{name}] block"
        ),
    }
}

/// Fold a saved-view preset (issue #80) into the parsed CLI before
/// `validate_view_args` and `build_view_spec` run.
///
/// **Override priority:** defaults < preset < CLI flags. For
/// `Option<T>` fields, CLI takes precedence iff `Some` (`cli.or(preset)`).
/// For bare `bool` fields, OR-merge: an explicit CLI flag can add to the
/// preset's `true` value but cannot subtract — bare clap booleans cannot
/// distinguish "user didn't pass it" from "explicitly false," so the CLI
/// can only opt INTO behaviour. Documented in `--help` and CHANGELOG.
pub(super) fn apply_preset_to_cli(cli: &mut Cli, preset: &ViewPreset) {
    // ── FilterArgs Options: CLI explicit value wins ───────────────
    if cli.filter.top.is_none() {
        cli.filter.top = preset.top;
    }
    if cli.filter.min_coverage.is_none() {
        cli.filter.min_coverage = preset.min_coverage;
    }
    if cli.filter.max_coverage.is_none() {
        cli.filter.max_coverage = preset.max_coverage;
    }
    if cli.filter.sort_by.is_none() {
        cli.filter.sort_by = preset.sort.map(SortKeyArg::from);
    }
    if cli.filter.group_by.is_none() {
        cli.filter.group_by = preset.group_by.map(GroupByArg::from);
    }
    // ── bools: OR-merge (CLI can opt-in but not opt-out) ──────────
    cli.filter.only_failing |= preset.only_failing.unwrap_or(false);
    cli.output.no_fail |= preset.no_fail.unwrap_or(false);
    cli.output.minimal_view |= preset.minimal_view.unwrap_or(false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crap4rs::domain::view::{GroupKey, SortKey};

    fn parse(args: &[&str]) -> Cli {
        let mut full = vec!["crap4rs"];
        full.extend_from_slice(args);
        <Cli as clap::Parser>::try_parse_from(full).expect("test args parse")
    }

    fn full_preset() -> ViewPreset {
        ViewPreset {
            top: Some(20),
            min_coverage: Some(0.0),
            max_coverage: Some(90.0),
            sort: Some(SortKey::Coverage),
            only_failing: Some(true),
            no_fail: Some(true),
            group_by: Some(GroupKey::File),
            minimal_view: Some(true),
        }
    }

    #[test]
    fn full_preset_no_cli_overrides_applies_every_field() {
        let mut cli = parse(&["--coverage", "lcov.info"]);
        apply_preset_to_cli(&mut cli, &full_preset());
        assert_eq!(cli.filter.top, Some(20));
        assert_eq!(cli.filter.min_coverage, Some(0.0));
        assert_eq!(cli.filter.max_coverage, Some(90.0));
        assert!(matches!(cli.filter.sort_by, Some(SortKeyArg::Coverage)));
        assert!(matches!(cli.filter.group_by, Some(GroupByArg::File)));
        assert!(cli.filter.only_failing);
        assert!(cli.output.no_fail);
        assert!(cli.output.minimal_view);
    }

    #[test]
    fn partial_preset_leaves_unspecified_at_default() {
        let mut cli = parse(&["--coverage", "lcov.info"]);
        let preset = ViewPreset {
            top: Some(5),
            ..ViewPreset::default()
        };
        apply_preset_to_cli(&mut cli, &preset);
        assert_eq!(cli.filter.top, Some(5));
        assert_eq!(cli.filter.min_coverage, None);
        assert_eq!(cli.filter.max_coverage, None);
        assert!(cli.filter.sort_by.is_none());
        assert!(cli.filter.group_by.is_none());
        assert!(!cli.filter.only_failing);
        assert!(!cli.output.no_fail);
        assert!(!cli.output.minimal_view);
    }

    #[test]
    fn cli_top_wins_over_preset_top() {
        let mut cli = parse(&["--coverage", "lcov.info", "--top", "5"]);
        apply_preset_to_cli(&mut cli, &full_preset());
        assert_eq!(
            cli.filter.top,
            Some(5),
            "CLI explicit --top must override preset top=20"
        );
    }

    #[test]
    fn cli_min_coverage_wins_over_preset() {
        let mut cli = parse(&["--coverage", "lcov.info", "--min-coverage", "70"]);
        apply_preset_to_cli(&mut cli, &full_preset());
        assert_eq!(cli.filter.min_coverage, Some(70.0));
        // Preset's max_coverage still applied (CLI didn't pass it).
        assert_eq!(cli.filter.max_coverage, Some(90.0));
    }

    #[test]
    fn cli_sort_by_wins_over_preset_sort() {
        let mut cli = parse(&["--coverage", "lcov.info", "--sort-by", "path"]);
        apply_preset_to_cli(&mut cli, &full_preset());
        assert!(matches!(cli.filter.sort_by, Some(SortKeyArg::Path)));
    }

    #[test]
    fn cli_group_by_wins_over_preset_group_by() {
        // Today only `file` is a valid value, so the assertion is identity-on
        // `Some(File)`. The branch matters for forward-compat: when a second
        // variant lands, this test catches a bug where preset clobbers CLI.
        let mut cli = parse(&["--coverage", "lcov.info", "--group-by", "file"]);
        apply_preset_to_cli(&mut cli, &full_preset());
        assert!(matches!(cli.filter.group_by, Some(GroupByArg::File)));
    }

    #[test]
    fn bool_or_merge_preset_true_cli_false_yields_true() {
        let mut cli = parse(&["--coverage", "lcov.info"]);
        let preset = ViewPreset {
            only_failing: Some(true),
            no_fail: Some(true),
            minimal_view: Some(true),
            ..ViewPreset::default()
        };
        apply_preset_to_cli(&mut cli, &preset);
        assert!(cli.filter.only_failing);
        assert!(cli.output.no_fail);
        assert!(cli.output.minimal_view);
    }

    #[test]
    fn bool_or_merge_cli_true_preset_false_yields_true() {
        let mut cli = parse(&[
            "--coverage",
            "lcov.info",
            "--only-failing",
            "--no-fail",
            "--minimal-view",
        ]);
        let preset = ViewPreset {
            only_failing: Some(false),
            no_fail: Some(false),
            minimal_view: Some(false),
            ..ViewPreset::default()
        };
        apply_preset_to_cli(&mut cli, &preset);
        assert!(cli.filter.only_failing, "CLI true survives preset false");
        assert!(cli.output.no_fail);
        assert!(cli.output.minimal_view);
    }

    #[test]
    fn bool_or_merge_both_true_idempotent() {
        let mut cli = parse(&["--coverage", "lcov.info", "--only-failing"]);
        let preset = ViewPreset {
            only_failing: Some(true),
            ..ViewPreset::default()
        };
        apply_preset_to_cli(&mut cli, &preset);
        assert!(cli.filter.only_failing);
    }

    #[test]
    fn bool_or_merge_preset_silent_leaves_cli_alone() {
        // `Option::None` means "preset is silent" — must not flip a CLI bool.
        let mut cli = parse(&["--coverage", "lcov.info", "--only-failing"]);
        let preset = ViewPreset::default();
        apply_preset_to_cli(&mut cli, &preset);
        assert!(cli.filter.only_failing, "CLI true survives silent preset");
    }

    #[test]
    fn cli_max_coverage_only_keeps_preset_min() {
        // Mixed precedence: CLI sets `max`, preset supplies `min`.
        let mut cli = parse(&["--coverage", "lcov.info", "--max-coverage", "50"]);
        let preset = ViewPreset {
            min_coverage: Some(10.0),
            max_coverage: Some(90.0),
            ..ViewPreset::default()
        };
        apply_preset_to_cli(&mut cli, &preset);
        assert_eq!(cli.filter.min_coverage, Some(10.0), "preset min applied");
        assert_eq!(cli.filter.max_coverage, Some(50.0), "CLI max wins");
    }

    // ── resolve_view_preset (#80) ──────────────────────────────────────

    fn config_with_preset(name: &str, preset: ViewPreset) -> FileConfig {
        let mut views = std::collections::HashMap::new();
        views.insert(name.to_string(), preset);
        FileConfig {
            views,
            ..FileConfig::default()
        }
    }

    #[test]
    fn resolve_view_preset_no_flag_is_noop() {
        let mut cli = parse(&["--coverage", "lcov.info"]);
        let cfg = config_with_preset("ci", full_preset());
        resolve_view_preset(&mut cli, Some(&cfg)).unwrap();
        // No `--view` passed → preset must NOT be applied.
        assert_eq!(cli.filter.top, None);
        assert!(cli.filter.sort_by.is_none());
        assert!(!cli.filter.only_failing);
    }

    #[test]
    fn resolve_view_preset_resolves_named_preset() {
        let mut cli = parse(&["--coverage", "lcov.info", "--view", "ci"]);
        let cfg = config_with_preset("ci", full_preset());
        resolve_view_preset(&mut cli, Some(&cfg)).unwrap();
        assert_eq!(cli.filter.top, Some(20));
        assert!(cli.filter.only_failing);
    }

    #[test]
    fn resolve_view_preset_cli_overrides_resolved_preset() {
        let mut cli = parse(&["--coverage", "lcov.info", "--view", "ci", "--top", "5"]);
        let cfg = config_with_preset("ci", full_preset());
        resolve_view_preset(&mut cli, Some(&cfg)).unwrap();
        assert_eq!(cli.filter.top, Some(5), "CLI --top wins over preset");
        // Other preset fields still applied.
        assert!(cli.filter.only_failing);
    }

    #[test]
    fn resolve_view_preset_unknown_name_lists_available() {
        let mut cli = parse(&["--coverage", "lcov.info", "--view", "wat"]);
        let mut views = std::collections::HashMap::new();
        views.insert("ci".to_string(), ViewPreset::default());
        views.insert("investigate".to_string(), ViewPreset::default());
        let cfg = FileConfig {
            views,
            ..FileConfig::default()
        };
        let err = resolve_view_preset(&mut cli, Some(&cfg)).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("unknown view preset"),
            "expected unknown-preset error, got: {msg}"
        );
        assert!(msg.contains("wat"), "must echo the bad name: {msg}");
        assert!(msg.contains("ci"), "must list `ci`: {msg}");
        assert!(
            msg.contains("investigate"),
            "must list `investigate`: {msg}"
        );
        // Sorted alphabetically — `ci` before `investigate`.
        assert!(
            msg.find("ci").unwrap() < msg.find("investigate").unwrap(),
            "available list must be sorted: {msg}"
        );
    }

    #[test]
    fn resolve_view_preset_no_config_file_explains_requirement() {
        let mut cli = parse(&["--coverage", "lcov.info", "--view", "ci"]);
        let err = resolve_view_preset(&mut cli, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unknown view preset"), "got: {msg}");
        assert!(
            msg.contains("crap4rs.toml"),
            "should mention config file: {msg}"
        );
    }

    #[test]
    fn resolve_view_preset_empty_views_block_hints_no_blocks_defined() {
        let mut cli = parse(&["--coverage", "lcov.info", "--view", "ci"]);
        let cfg = FileConfig::default();
        let err = resolve_view_preset(&mut cli, Some(&cfg)).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unknown view preset"));
        assert!(msg.contains("no [views"), "empty-block hint missing: {msg}");
    }
}
