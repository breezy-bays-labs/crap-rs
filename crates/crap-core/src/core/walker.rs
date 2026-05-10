//! Filesystem walker — discovers Rust source files for analysis,
//! respecting `.gitignore` and user-provided exclude patterns.
//!
//! Lives in `crap-core` even though it carries a hardcoded `.rs`
//! extension filter today: the only AST-purity gate that matters here
//! is "no `syn` / `quote` / `tree_sitter` / `swc` / `oxc` imports."
//! `ignore::WalkBuilder` is purely filesystem-walking machinery and
//! satisfies that gate. The `.rs` literal is a function-body wart that
//! `analyze<P: ParseDiagnostic>` will parameterize in S4 once the
//! parse-diagnostic type can carry the language-specific extension(s)
//! it consumes.
//!
//! Extracted from `crates/crap4rs/src/core/mod.rs::discover_rust_files`
//! during S3 (crap4rs#135) so that S4's relocation of `core::analyze` to
//! `crap-core` doesn't need to upward-import from the Rust adapter.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ignore::WalkBuilder;

/// Walk the source directory and collect all `.rs` files, respecting
/// .gitignore and user-provided exclude patterns.
pub fn discover_rust_files(
    src: &Path,
    exclude: &[String],
    respect_gitignore: bool,
) -> Result<Vec<PathBuf>> {
    let mut builder = WalkBuilder::new(src);
    builder.git_ignore(respect_gitignore);

    // Add exclude patterns as overrides
    if !exclude.is_empty() {
        let mut overrides = ignore::overrides::OverrideBuilder::new(src);
        for pattern in exclude {
            overrides
                .add(&format!("!{pattern}"))
                .with_context(|| format!("invalid exclude pattern: {pattern}"))?;
        }
        builder.overrides(overrides.build()?);
    }

    let mut files = Vec::new();
    for entry in builder.build() {
        let entry = entry?;
        if entry.file_type().is_some_and(|ft| ft.is_file())
            && entry.path().extension().is_some_and(|ext| ext == "rs")
        {
            files.push(entry.into_path());
        }
    }

    // Sort for deterministic output
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discover_rust_files_finds_nested() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("lib.rs"), "").unwrap();
        fs::write(src.join("sub").join("mod.rs"), "").unwrap();
        fs::write(src.join("readme.txt"), "").unwrap();

        let files = discover_rust_files(&src, &[], false).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.extension().unwrap() == "rs"));
    }

    #[test]
    fn discover_rust_files_sorted_deterministically() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("z.rs"), "").unwrap();
        fs::write(src.join("a.rs"), "").unwrap();
        fs::write(src.join("m.rs"), "").unwrap();

        let files = discover_rust_files(&src, &[], false).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.file_name().unwrap()).collect();
        assert_eq!(names, vec!["a.rs", "m.rs", "z.rs"]);
    }
}
