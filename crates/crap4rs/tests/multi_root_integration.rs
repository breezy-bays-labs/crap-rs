//! Integration tests for multi-root `--src` analysis (crap-rs#336).
//!
//! Exercises the α' dual-regime identity base end-to-end with the real
//! syn complexity adapter + real LCOV parser:
//!
//! - **Union**: `analyze([A, B])` discovers every function under both
//!   roots; order-independent; overlapping roots dedup.
//! - **Single-root back-compat**: one root yields src-relative identity
//!   (`lib.rs`), byte-identical to the pre-multi-root path.
//! - **Multi-root identity**: git-toplevel-relative keys
//!   (`crate-a/src/lib.rs`) so same-named files in different roots stay
//!   distinct.
//! - **No coverage bleed**: same relative path in two roots joins to its
//!   own coverage, never the sibling's.
//!
//! The BDD contract these mirror: `tests/features/multi_root_src.feature`.

use crap4rs::adapters::complexity::SynComplexityAdapter;
use crap4rs::adapters::coverage::LcovParser;
use crap4rs::core::identity::IdentityBase;
use crap4rs::core::{AnalyzeOptions, analyze};
use crap4rs::domain::threshold::ThresholdConfig;
use crap4rs::domain::types::ComplexityMetric;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Lay out a throwaway git repo with two crate-like roots that share a
/// crate-internal relative path (`adapters/mod.rs`) plus distinct files.
/// Returns the repo root (git toplevel).
fn scaffold_two_roots() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // crate-a/src/{lib.rs, adapters/mod.rs}
    let a_src = root.join("crate-a").join("src");
    std::fs::create_dir_all(a_src.join("adapters")).unwrap();
    std::fs::write(
        a_src.join("lib.rs"),
        "pub fn a_only(x: i32) -> i32 { if x > 0 { x } else { -x } }\n",
    )
    .unwrap();
    std::fs::write(
        a_src.join("adapters").join("mod.rs"),
        "pub fn shared_a() -> u8 { 1 }\n",
    )
    .unwrap();

    // crate-b/src/{lib.rs, adapters/mod.rs}
    let b_src = root.join("crate-b").join("src");
    std::fs::create_dir_all(b_src.join("adapters")).unwrap();
    std::fs::write(
        b_src.join("lib.rs"),
        "pub fn b_only(y: i32) -> i32 { y * 2 }\n",
    )
    .unwrap();
    std::fs::write(
        b_src.join("adapters").join("mod.rs"),
        "pub fn shared_b() -> u8 { 2 }\n",
    )
    .unwrap();

    // git init so git_toplevel resolves to `root`.
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@t.t"]);
    git(root, &["config", "user.name", "t"]);

    tmp
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed");
}

/// Build the RepoRelative identity base for two roots under `toplevel`.
fn repo_relative_base(toplevel: &Path, roots: &[PathBuf]) -> IdentityBase {
    let canonical_top = toplevel.canonicalize().unwrap();
    let root_prefixes = roots
        .iter()
        .map(|r| {
            let prefix = r
                .strip_prefix(&canonical_top)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (r.clone(), prefix)
        })
        .collect();
    IdentityBase::RepoRelative {
        toplevel: canonical_top,
        root_prefixes,
    }
}

/// LCOV covering both roots' files, keyed git-toplevel-relative, with
/// DIFFERENT hit counts on the two `adapters/mod.rs` so a bleed is
/// detectable. crate-a's shared fn is fully covered; crate-b's is not.
fn shared_lcov(toplevel: &Path) -> String {
    let mut s = String::new();
    // crate-a/src/adapters/mod.rs — covered (hits=1)
    s.push_str(&format!(
        "SF:{}\n",
        toplevel.join("crate-a/src/adapters/mod.rs").display()
    ));
    s.push_str("DA:1,1\nend_of_record\n");
    // crate-b/src/adapters/mod.rs — uncovered (hits=0)
    s.push_str(&format!(
        "SF:{}\n",
        toplevel.join("crate-b/src/adapters/mod.rs").display()
    ));
    s.push_str("DA:1,0\nend_of_record\n");
    // lib.rs files — both covered
    s.push_str(&format!(
        "SF:{}\n",
        toplevel.join("crate-a/src/lib.rs").display()
    ));
    s.push_str("DA:1,1\nend_of_record\n");
    s.push_str(&format!(
        "SF:{}\n",
        toplevel.join("crate-b/src/lib.rs").display()
    ));
    s.push_str("DA:1,1\nend_of_record\n");
    s
}

fn multi_root_options(
    roots: Vec<PathBuf>,
    coverage: PathBuf,
    base: IdentityBase,
) -> AnalyzeOptions {
    AnalyzeOptions {
        src: roots,
        coverage,
        identity_base: base,
        threshold_config: ThresholdConfig {
            global: 100.0,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        extensions: vec!["rs".to_string()],
        ..AnalyzeOptions::default()
    }
}

#[test]
fn multi_root_unions_functions_with_toplevel_keys() {
    let tmp = scaffold_two_roots();
    let top = tmp.path().canonicalize().unwrap();
    let a = top.join("crate-a/src");
    let b = top.join("crate-b/src");

    let lcov = shared_lcov(&top);
    let lcov_path = top.join("lcov.info");
    std::fs::write(&lcov_path, &lcov).unwrap();

    let base = repo_relative_base(&top, &[a.clone(), b.clone()]);
    // Coverage parser anchored on the toplevel (the multi-root base).
    let cx = SynComplexityAdapter::new();
    let cov = LcovParser::new(top.clone());
    let opts = multi_root_options(vec![a, b], lcov_path, base);

    let result = analyze(&opts, &cx, &cov).unwrap().result;

    let keys: Vec<&str> = result
        .functions
        .iter()
        .map(|v| v.scored.identity.file_path.as_str())
        .collect();

    // Every root's functions present, keyed git-toplevel-relative.
    assert!(keys.contains(&"crate-a/src/lib.rs"), "keys: {keys:?}");
    assert!(keys.contains(&"crate-b/src/lib.rs"), "keys: {keys:?}");
    assert!(
        keys.contains(&"crate-a/src/adapters/mod.rs"),
        "keys: {keys:?}"
    );
    assert!(
        keys.contains(&"crate-b/src/adapters/mod.rs"),
        "keys: {keys:?}"
    );
    // 4 functions across both roots.
    assert_eq!(result.functions.len(), 4, "keys: {keys:?}");
}

#[test]
fn multi_root_union_is_order_independent() {
    let tmp = scaffold_two_roots();
    let top = tmp.path().canonicalize().unwrap();
    let a = top.join("crate-a/src");
    let b = top.join("crate-b/src");
    let lcov_path = top.join("lcov.info");
    std::fs::write(&lcov_path, shared_lcov(&top)).unwrap();

    let run = |roots: Vec<PathBuf>| {
        let base = repo_relative_base(&top, &roots);
        let cx = SynComplexityAdapter::new();
        let cov = LcovParser::new(top.clone());
        let opts = multi_root_options(roots, lcov_path.clone(), base);
        let mut keys: Vec<String> = analyze(&opts, &cx, &cov)
            .unwrap()
            .result
            .functions
            .iter()
            .map(|v| v.scored.identity.file_path.clone())
            .collect();
        keys.sort();
        keys
    };

    let ab = run(vec![a.clone(), b.clone()]);
    let ba = run(vec![b, a]);
    assert_eq!(ab, ba, "union must be order-independent");
}

#[test]
fn multi_root_no_coverage_bleed_on_shared_relative_path() {
    let tmp = scaffold_two_roots();
    let top = tmp.path().canonicalize().unwrap();
    let a = top.join("crate-a/src");
    let b = top.join("crate-b/src");
    let lcov_path = top.join("lcov.info");
    std::fs::write(&lcov_path, shared_lcov(&top)).unwrap();

    let base = repo_relative_base(&top, &[a.clone(), b.clone()]);
    let cx = SynComplexityAdapter::new();
    let cov = LcovParser::new(top.clone());
    let opts = multi_root_options(vec![a, b], lcov_path, base);

    let result = analyze(&opts, &cx, &cov).unwrap().result;

    // crate-a/src/adapters/mod.rs is covered (DA:1,1 → 100%);
    // crate-b/src/adapters/mod.rs is uncovered (DA:1,0 → 0%).
    // A bleed would attribute a's coverage to b or vice versa.
    let cov_of = |key: &str| {
        result
            .functions
            .iter()
            .find(|v| v.scored.identity.file_path == key)
            .unwrap_or_else(|| panic!("function not found: {key}"))
            .scored
            .coverage_percent
    };

    assert_eq!(
        cov_of("crate-a/src/adapters/mod.rs"),
        100.0,
        "crate-a shared file must keep its own (full) coverage"
    );
    assert_eq!(
        cov_of("crate-b/src/adapters/mod.rs"),
        0.0,
        "crate-b shared file must keep its own (zero) coverage — no bleed"
    );
}

/// CI emits git-toplevel-RELATIVE `SF:` paths (`crates/crap-core/src/...`),
/// NOT the absolute paths local `cargo llvm-cov` produces. The α'
/// "both sides share the git-toplevel base natively" claim is what makes
/// the production scorecard green in CI — exercise it explicitly so a
/// `normalize_path` regression that breaks relative-SF anchoring can't
/// slip past the absolute-SF fixtures above.
#[test]
fn multi_root_joins_relative_sf_paths_no_bleed() {
    let tmp = scaffold_two_roots();
    let top = tmp.path().canonicalize().unwrap();
    let a = top.join("crate-a/src");
    let b = top.join("crate-b/src");

    // Toplevel-RELATIVE SF keys, mirroring CI's workspace lcov shape.
    let lcov = "SF:crate-a/src/adapters/mod.rs\nDA:1,1\nend_of_record\n\
                SF:crate-b/src/adapters/mod.rs\nDA:1,0\nend_of_record\n\
                SF:crate-a/src/lib.rs\nDA:1,1\nend_of_record\n\
                SF:crate-b/src/lib.rs\nDA:1,1\nend_of_record\n";
    let lcov_path = top.join("rel.info");
    std::fs::write(&lcov_path, lcov).unwrap();

    let base = repo_relative_base(&top, &[a.clone(), b.clone()]);
    let cx = SynComplexityAdapter::new();
    let cov = LcovParser::new(top.clone());
    let opts = multi_root_options(vec![a, b], lcov_path, base);

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    let cov_of = |key: &str| {
        result
            .functions
            .iter()
            .find(|v| v.scored.identity.file_path == key)
            .unwrap_or_else(|| panic!("function not found: {key}"))
            .scored
            .coverage_percent
    };
    // Relative-SF join lands on the right toplevel-relative key, no bleed.
    assert_eq!(cov_of("crate-a/src/adapters/mod.rs"), 100.0);
    assert_eq!(cov_of("crate-b/src/adapters/mod.rs"), 0.0);
}

#[test]
fn single_root_yields_src_relative_identity() {
    // Back-compat: a single root via the Vec API + SrcRelative base
    // produces src-relative keys (`lib.rs`), exactly as before
    // multi-root existed.
    let tmp = scaffold_two_roots();
    let top = tmp.path().canonicalize().unwrap();
    let a = top.join("crate-a/src");

    // src-relative lcov, anchored on the single root.
    let lcov = "SF:lib.rs\nDA:1,1\nend_of_record\n\
                SF:adapters/mod.rs\nDA:1,1\nend_of_record\n";
    let lcov_path = top.join("single.info");
    std::fs::write(&lcov_path, lcov).unwrap();

    let base = IdentityBase::SrcRelative(a.clone());
    let cx = SynComplexityAdapter::new();
    let cov = LcovParser::new(a.clone());
    let opts = multi_root_options(vec![a], lcov_path, base);

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    let keys: Vec<&str> = result
        .functions
        .iter()
        .map(|v| v.scored.identity.file_path.as_str())
        .collect();

    assert!(keys.contains(&"lib.rs"), "keys: {keys:?}");
    assert!(keys.contains(&"adapters/mod.rs"), "keys: {keys:?}");
    // NOT git-toplevel-relative in single-root mode.
    assert!(
        !keys.iter().any(|k| k.starts_with("crate-a/")),
        "single-root must be src-relative, got: {keys:?}"
    );
}
