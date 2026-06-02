//! Filesystem walker — discovers source files for analysis,
//! respecting `.gitignore` and user-provided exclude patterns.
//!
//! Adapter-agnostic: the extension filter is parameter-driven via
//! `AnalyzeOptions::extensions` so crap4rs passes `&["rs"]`, crap4ts
//! passes `&["ts","tsx","js","jsx","mjs","cjs"]`, and future adapters
//! supply their own. AST-purity gate satisfied — `ignore::WalkBuilder`
//! is filesystem-walking machinery with no `syn` / `tree_sitter` /
//! `swc` / `oxc` coupling.
//!
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ignore::WalkBuilder;

/// Walk the source directory and collect all files whose extension
/// matches one in `extensions` (case-sensitive), respecting
/// `.gitignore` and user-provided exclude patterns.
///
/// `extensions` is the bare suffix without the leading dot
/// (`"rs"`, `"ts"`, `"tsx"`). An empty slice matches no files (the
/// caller is expected to short-circuit on
/// `AnalyzeOptions::extensions.is_empty()` upstream, but we don't
/// crash here).
pub fn discover_source_files(
    src: &Path,
    exclude: &[String],
    respect_gitignore: bool,
    extensions: &[&str],
) -> Result<Vec<PathBuf>> {
    let mut builder = WalkBuilder::new(src);
    builder.git_ignore(respect_gitignore);
    apply_exclude_overrides(&mut builder, src, exclude)?;

    let mut files = Vec::new();
    for entry in builder.build() {
        let entry = entry?;
        if entry_matches(&entry, extensions) {
            files.push(entry.into_path());
        }
    }

    // Sort for deterministic output
    files.sort();
    Ok(files)
}

/// Register each user exclude pattern as a walker override. The
/// `ignore` crate's override semantics invert gitignore: a bare glob
/// whitelists, so each pattern is prefixed with `!` to turn it into an
/// exclusion. A no-op when `exclude` is empty.
fn apply_exclude_overrides(
    builder: &mut WalkBuilder,
    src: &Path,
    exclude: &[String],
) -> Result<()> {
    if exclude.is_empty() {
        return Ok(());
    }
    let mut overrides = ignore::overrides::OverrideBuilder::new(src);
    for pattern in exclude {
        overrides
            .add(&format!("!{pattern}"))
            .with_context(|| format!("invalid exclude pattern: {pattern}"))?;
    }
    builder.overrides(overrides.build()?);
    Ok(())
}

/// True when a walk entry is a regular file whose extension is in the
/// allow-list. Directories, symlinks-to-nowhere, and extension-less or
/// wrong-extension files are rejected.
fn entry_matches(entry: &ignore::DirEntry, extensions: &[&str]) -> bool {
    entry.file_type().is_some_and(|ft| ft.is_file())
        && entry
            .path()
            .extension()
            .is_some_and(|ext| extensions.iter().any(|e| ext == *e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discover_source_files_finds_nested_rust_extension() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("lib.rs"), "").unwrap();
        fs::write(src.join("sub").join("mod.rs"), "").unwrap();
        fs::write(src.join("readme.txt"), "").unwrap();

        let files = discover_source_files(&src, &[], false, &["rs"]).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.extension().unwrap() == "rs"));
    }

    #[test]
    fn discover_source_files_sorted_deterministically() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("z.rs"), "").unwrap();
        fs::write(src.join("a.rs"), "").unwrap();
        fs::write(src.join("m.rs"), "").unwrap();

        let files = discover_source_files(&src, &[], false, &["rs"]).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.file_name().unwrap()).collect();
        assert_eq!(names, vec!["a.rs", "m.rs", "z.rs"]);
    }

    /// Walker honors arbitrary extensions, not just `.rs`, so crap4ts
    /// can later land its oxc walker against the same filesystem layer.
    #[test]
    fn discover_source_files_finds_typescript_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.ts"), "").unwrap();
        fs::write(src.join("b.tsx"), "").unwrap();
        fs::write(src.join("sub").join("c.js"), "").unwrap();
        fs::write(src.join("d.rs"), "").unwrap(); // wrong-language sibling
        fs::write(src.join("notes.md"), "").unwrap();

        let files =
            discover_source_files(&src, &[], false, &["ts", "tsx", "js", "jsx", "mjs", "cjs"])
                .unwrap();
        let names: Vec<_> = files
            .iter()
            .filter_map(|f| f.file_name().and_then(|n| n.to_str()))
            .collect();
        assert_eq!(names, vec!["a.ts", "b.tsx", "c.js"]);
        assert!(
            !names.iter().any(|n| n.ends_with(".rs")),
            "walker must not pick up `.rs` when it's not in the extension allow-list"
        );
    }

    /// A `**/*.d.ts` glob in the exclude list drops TypeScript
    /// declaration files while a sibling `app.ts` survives — pins the
    /// `ignore::overrides` contract the crap4ts `forced_excludes`
    /// wiring in `cli::merge_exclude` relies on. The adapter passes
    /// `**/*.d.ts` via `AdapterMeta::forced_excludes`; this test
    /// confirms the walker mechanism honors that glob shape.
    #[test]
    fn discover_source_files_dts_excluded_by_glob() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("app.ts"), "").unwrap();
        fs::write(src.join("types.d.ts"), "").unwrap();
        fs::write(src.join("sub").join("nested.d.ts"), "").unwrap();
        fs::write(src.join("sub").join("nested.ts"), "").unwrap();

        let exclude = vec!["**/*.d.ts".to_string()];
        let files =
            discover_source_files(&src, &exclude, false, &["ts", "tsx", "js", "jsx"]).unwrap();
        let names: Vec<_> = files
            .iter()
            .filter_map(|f| f.file_name().and_then(|n| n.to_str()))
            .collect();
        assert_eq!(names, vec!["app.ts", "nested.ts"]);
        assert!(
            !names.iter().any(|n| n.ends_with(".d.ts")),
            "`**/*.d.ts` glob must drop every declaration file in the tree, including nested ones",
        );
    }

    /// Empty `extensions` returns no files — the only sane behavior
    /// when the caller forgot to set `AnalyzeOptions::extensions`.
    /// `core::ensure_source_files_found` then surfaces the diagnostic.
    #[test]
    fn discover_source_files_empty_extensions_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("lib.rs"), "").unwrap();

        let files = discover_source_files(&src, &[], false, &[]).unwrap();
        assert!(
            files.is_empty(),
            "no extensions configured ⇒ no files (caller surfaces diagnostic upstream)"
        );
    }
}
