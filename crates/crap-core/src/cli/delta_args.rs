//! Build a `DeltaViewSpec` from parsed CLI args. Mirrors
//! `cli::view_args::build_view_spec` for the delta sibling.

use std::collections::BTreeSet;

use super::{Cli, DeltaKindArg};
use crate::domain::delta::{ChangeKind, DeltaFilters, DeltaSortKey, DeltaViewSpec};

/// Map `--delta-top` / `--delta-sort` / `--delta-only` flags into a
/// `DeltaViewSpec`.
///
/// `--delta-top 0` is canonicalised to `None` at this boundary so JSON
/// consumers see effective behaviour, not the literal input. Same
/// pattern as `view_args::build_view_spec`.
pub(super) fn build_delta_view_spec(cli: &Cli) -> DeltaViewSpec {
    // `DeltaViewSpec` and `DeltaFilters` are `#[non_exhaustive]` for
    // cross-crate consumers, but in-crate (post-S4 #136) struct-update
    // syntax is permitted and clippy's `field_reassign_with_default`
    // fires on the previous mutate-after-default pattern. Use
    // struct-update so the cli/domain boundary stays clear.
    let filters = DeltaFilters {
        change_kinds: kinds_from_cli(&cli.filter.delta_only),
        ..DeltaFilters::default()
    };
    DeltaViewSpec {
        filters,
        sort: cli
            .filter
            .delta_sort
            .map(DeltaSortKey::from)
            .unwrap_or_default(),
        limit: limit_from_cli(cli.filter.delta_top),
        ..DeltaViewSpec::default()
    }
}

fn limit_from_cli(top: Option<u32>) -> Option<usize> {
    match top {
        None | Some(0) => None,
        Some(n) => Some(n as usize),
    }
}

fn kinds_from_cli(args: &[DeltaKindArg]) -> Option<BTreeSet<ChangeKind>> {
    if args.is_empty() {
        return None;
    }
    Some(args.iter().copied().map(ChangeKind::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        // Prepend a placeholder arg0 + the required --coverage flag so
        // clap's positional validators don't reject the parse outright.
        let mut full = vec!["crap4rs", "--coverage", "lcov.info"];
        full.extend_from_slice(args);
        Cli::try_parse_from(full).expect("clap parse should succeed")
    }

    #[test]
    fn no_delta_flags_yields_default_spec() {
        let cli = parse(&[]);
        let spec = build_delta_view_spec(&cli);
        assert_eq!(spec.sort, DeltaSortKey::ScoreDelta);
        assert!(spec.limit.is_none());
        assert!(spec.filters.change_kinds.is_none());
    }

    #[test]
    fn delta_top_canonicalises_zero_to_none() {
        let cli = parse(&["--delta-top", "0"]);
        let spec = build_delta_view_spec(&cli);
        assert!(spec.limit.is_none());
    }

    #[test]
    fn delta_top_positive_is_some() {
        let cli = parse(&["--delta-top", "5"]);
        let spec = build_delta_view_spec(&cli);
        assert_eq!(spec.limit, Some(5));
    }

    #[test]
    fn delta_sort_current_crap_maps_through() {
        let cli = parse(&["--delta-sort", "current-crap"]);
        let spec = build_delta_view_spec(&cli);
        assert_eq!(spec.sort, DeltaSortKey::CurrentCrap);
    }

    #[test]
    fn delta_only_added_modified_yields_two_kind_set() {
        let cli = parse(&["--delta-only", "added,modified"]);
        let spec = build_delta_view_spec(&cli);
        let kinds = spec.filters.change_kinds.expect("kinds set");
        assert_eq!(kinds.len(), 2);
        assert!(kinds.contains(&ChangeKind::Added));
        assert!(kinds.contains(&ChangeKind::Modified));
        assert!(!kinds.contains(&ChangeKind::Removed));
    }

    #[test]
    fn delta_only_single_kind() {
        let cli = parse(&["--delta-only", "removed"]);
        let spec = build_delta_view_spec(&cli);
        let kinds = spec.filters.change_kinds.expect("kinds set");
        assert_eq!(kinds.len(), 1);
        assert!(kinds.contains(&ChangeKind::Removed));
    }

    #[test]
    fn delta_only_unknown_token_exits_with_clap_error() {
        let result = Cli::try_parse_from([
            "crap4rs",
            "--coverage",
            "lcov.info",
            "--delta-only",
            "nonsense",
        ]);
        let err = result.expect_err("nonsense should fail clap value parse");
        let stderr = err.to_string();
        assert!(stderr.contains("invalid value 'nonsense' for '--delta-only"));
    }

    #[test]
    fn delta_sort_unknown_token_exits_with_clap_error() {
        let result = Cli::try_parse_from([
            "crap4rs",
            "--coverage",
            "lcov.info",
            "--delta-sort",
            "nonsense",
        ]);
        let err = result.expect_err("nonsense should fail clap value parse");
        let stderr = err.to_string();
        assert!(stderr.contains("invalid value 'nonsense' for '--delta-sort"));
    }

    #[test]
    fn delta_top_negative_is_attributed_to_flag() {
        let result =
            Cli::try_parse_from(["crap4rs", "--coverage", "lcov.info", "--delta-top", "-5"]);
        let err = result.expect_err("negative should fail u32 parse");
        let stderr = err.to_string();
        assert!(stderr.contains("--delta-top"));
    }
}
