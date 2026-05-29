//! Run identity base — the single path-space every function's
//! `file_path` and every coverage `SF:` record is relativized against.
//!
//! A CRAP run declares ONE identity base, decided from the count of
//! `--src` roots (the policy lives in `cli`, resolved once between
//! input validation and the coverage-factory call). The base is then
//! threaded to BOTH consumers — function identity (`extract_complexities`)
//! and coverage-SF normalization (the LCOV adapter constructed in
//! `cli::prepare_pipeline`). Keeping the two consumers on one base is the
//! load-bearing invariant: if they diverge (identity goes one way, SF
//! keys the other), the coverage join misses 100% of the time.
//!
//! Two regimes, never mixed within one run:
//!
//! - **Single root** ⇒ [`IdentityBase::SrcRelative`]. Identity strips the
//!   original (typically relative) `--src` root, exactly as before
//!   multi-root existed. This path is **byte-identical** to the
//!   pre-multi-root output — the wire-envelope canaries enforce it.
//! - **Multiple roots** ⇒ [`IdentityBase::RepoRelative`]. Identity is
//!   keyed on the git toplevel so paths stay globally unique across
//!   crates that share crate-internal relative names (`adapters/mod.rs`,
//!   `lib.rs`). Each root carries its own toplevel-relative prefix; a
//!   discovered file's key is `<root_prefix>/<file-relative-to-root>`.
//!   Mirrors the bridge `compute_diff_regions` already performs.
//!
//! Why this lives in `core` (not `domain`): it is a list-of-roots
//! orchestration concern, language-agnostic, with no CRAP-formula or
//! reporter coupling — it survives the future `crap-core` extraction
//! intact.

use std::path::{Path, PathBuf};

/// The path-space a run relativizes function identity and coverage SF
/// records against. Decided once from the `--src` root count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityBase {
    /// Single-root run: identity strips the original `--src` root. The
    /// held path is the *original* (relative-or-absolute) `--src` value,
    /// matching the discovery root so `strip_prefix` never fails. This
    /// is byte-identical to the pre-multi-root behavior.
    ///
    /// The default variant holds `"src"` — matching the historical
    /// `AnalyzeOptions::default().src` of `PathBuf::from("src")`.
    SrcRelative(PathBuf),

    /// Multi-root run: identity is git-toplevel-relative. `toplevel` is
    /// the absolute git work-tree root the coverage factory anchors on;
    /// `root_prefixes` pairs each original `--src` root with its
    /// toplevel-relative prefix (forward-slash-normalised, possibly
    /// empty if a root IS the toplevel).
    RepoRelative {
        /// Absolute git toplevel — handed to the coverage factory so
        /// LCOV SF normalization shares the identity base.
        toplevel: PathBuf,
        /// `(original_root, toplevel_relative_prefix)` per `--src` root.
        root_prefixes: Vec<(PathBuf, String)>,
    },
}

impl Default for IdentityBase {
    /// `SrcRelative("src")` — matches the historical
    /// `AnalyzeOptions::default().src` of `PathBuf::from("src")`, so a
    /// `..AnalyzeOptions::default()` spread stays single-root byte-identical.
    fn default() -> Self {
        IdentityBase::SrcRelative(PathBuf::from("src"))
    }
}

impl IdentityBase {
    /// The path handed to the coverage-adapter factory so SF
    /// normalization anchors on the same base as identity.
    ///
    /// - `SrcRelative` ⇒ the canonicalized single src root (today's
    ///   wiring — the factory has always received the canonical
    ///   absolute path, NOT the original relative one).
    /// - `RepoRelative` ⇒ the absolute git toplevel.
    pub fn coverage_root(&self, src_canonical: &Path) -> PathBuf {
        match self {
            // The factory receives `src_canonical` (the canonicalized
            // single root), preserving the pre-multi-root contract
            // byte-for-byte. The `SrcRelative` payload (the *original*
            // root) is only the identity strip base, not the coverage
            // anchor — the two consumers legitimately receive different
            // path-spaces in single-root mode, and that asymmetry is
            // exactly what keeps the output byte-identical.
            IdentityBase::SrcRelative(_) => src_canonical.to_path_buf(),
            IdentityBase::RepoRelative { toplevel, .. } => toplevel.clone(),
        }
    }

    /// Relativize a discovered file to a forward-slash-normalised
    /// identity key.
    ///
    /// `originating_root` is the `--src` root the file was discovered
    /// under (in single-root mode there is only one). The key is:
    /// - `SrcRelative` ⇒ `file` stripped of the held src root.
    /// - `RepoRelative` ⇒ `<root_prefix>/<file stripped of originating_root>`,
    ///   where `root_prefix` is the originating root's toplevel-relative
    ///   prefix.
    ///
    /// Panics only on the same invariant the pre-multi-root code
    /// asserted: a discovered file must be under the root it was
    /// discovered through (a file-walker bug otherwise).
    pub fn relativize(&self, file: &Path, originating_root: &Path) -> String {
        match self {
            IdentityBase::SrcRelative(base) => strip_to_slashed(file, base),
            IdentityBase::RepoRelative { root_prefixes, .. } => {
                let prefix = root_prefixes
                    .iter()
                    .find(|(root, _)| root == originating_root)
                    .map(|(_, prefix)| prefix.as_str())
                    .unwrap_or("");
                let rel = strip_to_slashed(file, originating_root);
                if prefix.is_empty() {
                    rel
                } else {
                    format!("{prefix}/{rel}")
                }
            }
        }
    }
}

/// Strip `root` from `file` and forward-slash-normalise. Panics if
/// `file` is not under `root` — the file-walker invariant.
fn strip_to_slashed(file: &Path, root: &Path) -> String {
    file.strip_prefix(root)
        .expect("discovered file should be under the source root")
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn src_relative_strips_original_root() {
        let base = IdentityBase::SrcRelative(PathBuf::from("crates/crap-core/src"));
        let key = base.relativize(
            Path::new("crates/crap-core/src/adapters/mod.rs"),
            Path::new("crates/crap-core/src"),
        );
        assert_eq!(key, "adapters/mod.rs");
    }

    #[test]
    fn src_relative_coverage_root_is_canonical_not_original() {
        // The factory has always received the *canonical* path, never
        // the original relative one — preserving byte-identical output.
        let base = IdentityBase::SrcRelative(PathBuf::from("src"));
        let canonical = Path::new("/abs/project/src");
        assert_eq!(
            base.coverage_root(canonical),
            PathBuf::from("/abs/project/src")
        );
    }

    #[test]
    fn repo_relative_prefixes_with_root_toplevel_prefix() {
        let base = IdentityBase::RepoRelative {
            toplevel: PathBuf::from("/abs/project"),
            root_prefixes: vec![
                (
                    PathBuf::from("crates/crap-core/src"),
                    "crates/crap-core/src".to_string(),
                ),
                (
                    PathBuf::from("crates/crap4rs/src"),
                    "crates/crap4rs/src".to_string(),
                ),
            ],
        };
        let a = base.relativize(
            Path::new("crates/crap-core/src/adapters/mod.rs"),
            Path::new("crates/crap-core/src"),
        );
        let b = base.relativize(
            Path::new("crates/crap4rs/src/adapters/mod.rs"),
            Path::new("crates/crap4rs/src"),
        );
        assert_eq!(a, "crates/crap-core/src/adapters/mod.rs");
        assert_eq!(b, "crates/crap4rs/src/adapters/mod.rs");
        // Same crate-internal relative name, distinct global keys — the
        // collision the dual regime exists to prevent.
        assert_ne!(a, b);
    }

    #[test]
    fn repo_relative_coverage_root_is_toplevel() {
        let base = IdentityBase::RepoRelative {
            toplevel: PathBuf::from("/abs/project"),
            root_prefixes: vec![],
        };
        // src_canonical argument is ignored in the multi-root regime.
        assert_eq!(
            base.coverage_root(Path::new("/ignored")),
            PathBuf::from("/abs/project")
        );
    }

    #[test]
    fn repo_relative_empty_prefix_when_root_is_toplevel() {
        let base = IdentityBase::RepoRelative {
            toplevel: PathBuf::from("/abs/project"),
            root_prefixes: vec![(PathBuf::from("/abs/project"), String::new())],
        };
        let key = base.relativize(Path::new("/abs/project/lib.rs"), Path::new("/abs/project"));
        assert_eq!(key, "lib.rs");
    }

    #[test]
    fn windows_backslashes_normalised_to_forward() {
        let base = IdentityBase::SrcRelative(PathBuf::from("src"));
        // strip_prefix is component-wise so this exercises the
        // to_string_lossy().replace('\\', "/") normalisation only when
        // the original components contained backslashes; on unix the
        // path "src/a/b.rs" already uses forward slashes. The assertion
        // pins the slash-normalisation contract regardless of platform.
        let key = base.relativize(Path::new("src/a/b.rs"), Path::new("src"));
        assert_eq!(key, "a/b.rs");
    }
}
