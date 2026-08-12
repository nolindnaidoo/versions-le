//! What counts as a manifest, which ecosystem it speaks, and whether a
//! workflow value is evidently a version at all.
//!
//! One place, so discovery and the readers cannot disagree about which
//! files exist.

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

/// The five grammars this tool models.
///
/// **Conflict analysis never crosses one of these lines.** An npm
/// `semver` and a Cargo `semver` are unrelated packages that happen to
/// share a word, and a bare `"1.0.200"` means "exactly 1.0.200" in one
/// and "anything below 2" in the other. Comparing across ecosystems
/// would be comparing two different questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Ecosystem {
    Cargo,
    Ci,
    Go,
    Npm,
    Python,
}

impl Ecosystem {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Ci => "ci",
            Self::Go => "go",
            Self::Npm => "npm",
            Self::Python => "python",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "cargo" => Some(Self::Cargo),
            "ci" => Some(Self::Ci),
            "go" => Some(Self::Go),
            "npm" => Some(Self::Npm),
            "python" => Some(Self::Python),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManifestKind {
    PackageJson,
    CargoToml,
    PyprojectToml,
    GoMod,
    Workflow,
}

impl ManifestKind {
    pub(crate) fn ecosystem(self) -> Ecosystem {
        match self {
            Self::PackageJson => Ecosystem::Npm,
            Self::CargoToml => Ecosystem::Cargo,
            Self::PyprojectToml => Ecosystem::Python,
            Self::GoMod => Ecosystem::Go,
            Self::Workflow => Ecosystem::Ci,
        }
    }
}

pub(crate) fn basename(filepath: &str) -> &str {
    filepath.rsplit(['/', '\\']).next().unwrap_or(filepath)
}

/// Which manifest a path is, if it is one.
///
/// The workflow test is on the **directory**, not the extension: a
/// repository is full of YAML that is not a workflow, and reading a
/// Kubernetes manifest for `uses:` lines would invent entries out of
/// nothing.
pub(crate) fn manifest_kind(filepath: &str) -> Option<ManifestKind> {
    let path = filepath.replace('\\', "/");
    if is_workflow(&path) {
        return Some(ManifestKind::Workflow);
    }
    match basename(&path) {
        "package.json" => Some(ManifestKind::PackageJson),
        "Cargo.toml" => Some(ManifestKind::CargoToml),
        "pyproject.toml" => Some(ManifestKind::PyprojectToml),
        "go.mod" => Some(ManifestKind::GoMod),
        _ => None,
    }
}

fn is_workflow(path: &str) -> bool {
    let inside = path.starts_with(".github/workflows/") || path.contains("/.github/workflows/");
    inside && (path.ends_with(".yml") || path.ends_with(".yaml"))
}

/// Directories never worth walking, whatever the ignore rules say.
///
/// `node_modules` is the load-bearing one: it holds a `package.json` per
/// installed package, so a tree without a `.gitignore` would answer with
/// ten thousand manifests nobody wrote. They are also not this
/// repository's constraints — they are the resolver's output.
pub(crate) fn is_vendored(name: &str) -> bool {
    matches!(name, "node_modules" | ".git" | "vendor")
}

/// The exclude patterns, compiled once.
///
/// **Once, not once per file.** This was a fresh matcher built for every
/// (file, pattern) pair, so a thousand-file tree paid for a thousand
/// compilations of the same pattern — work that is quadratic in nothing
/// anyone can see.
///
/// The syntax is `globset`'s, which is ripgrep's, which is the syntax a
/// person auditing a repository already has in their head — the same
/// argument that chose `ignore` for the walk. `*` and `?` stop at a `/`
/// and `**` crosses one; a pattern with a `/` matches the root-relative
/// path and one without matches the basename, gitignore-style.
#[derive(Debug, Default)]
pub(crate) struct Excludes {
    paths: GlobSet,
    names: GlobSet,
}

impl Excludes {
    pub(crate) fn new(patterns: &[String]) -> Self {
        let mut paths = GlobSetBuilder::new();
        let mut names = GlobSetBuilder::new();
        for pattern in patterns {
            // A pattern that will not compile excludes **nothing**,
            // rather than everything: a typo in a config must not
            // silently hide the manifests this tool exists to compare.
            let Ok(glob) = GlobBuilder::new(pattern).literal_separator(true).build() else {
                continue;
            };
            if pattern.contains('/') {
                paths.add(glob);
            } else {
                names.add(glob);
            }
        }
        Self {
            paths: paths.build().unwrap_or_else(|_| GlobSet::empty()),
            names: names.build().unwrap_or_else(|_| GlobSet::empty()),
        }
    }

    pub(crate) fn matches(&self, filepath: &str) -> bool {
        let path = filepath.replace('\\', "/");
        self.paths.is_match(&path) || self.names.is_match(basename(&path))
    }
}

/// Whether a string found outside a typed manifest key is evidently a
/// version at all.
///
/// A workflow is YAML, and `<tool>-version:` is a naming convention
/// rather than a schema. `${{ matrix.node }}`, a filename, or a list is
/// not a version, and reading one as a version would put a fabricated
/// entry into the comparison. Anything that fails this gate becomes an
/// `ambiguous_version_string` refusal instead.
pub(crate) fn looks_like_a_version(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() || text.contains("${{") || text.contains(',') || text.starts_with('[') {
        return false;
    }
    // `v` first: a leading `v` is only evidence when a digit follows it,
    // and the other openings are all one character wide.
    if let Some(rest) = text.strip_prefix(['v', 'V']) {
        return rest.starts_with(|c: char| c.is_ascii_digit());
    }
    if text.starts_with(|c: char| c.is_ascii_digit())
        || text.starts_with(['^', '~', '>', '<', '=', '*'])
    {
        return true;
    }
    // The moving channel names, which are refused later with a reason
    // rather than dropped here without one.
    matches!(
        text,
        "latest" | "stable" | "nightly" | "beta" | "current" | "node" | "lts"
    ) || text.starts_with("lts/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_names_are_recognised() {
        for (path, kind) in [
            ("package.json", ManifestKind::PackageJson),
            ("crates/api/Cargo.toml", ManifestKind::CargoToml),
            ("pyproject.toml", ManifestKind::PyprojectToml),
            ("services/go.mod", ManifestKind::GoMod),
            (".github/workflows/ci.yml", ManifestKind::Workflow),
            (
                "repo/.github/workflows/release.yaml",
                ManifestKind::Workflow,
            ),
        ] {
            assert_eq!(manifest_kind(path), Some(kind), "{path}");
        }
    }

    /// A repository is full of YAML that is not a workflow, and reading
    /// one for `uses:` lines would invent entries out of nothing.
    #[test]
    fn yaml_outside_the_workflow_directory_is_not_a_manifest() {
        for path in [
            "deploy/k8s.yml",
            "docker-compose.yaml",
            ".github/dependabot.yml",
            "Cargo.lock",
            "package-lock.json",
            "requirements.txt",
        ] {
            assert_eq!(manifest_kind(path), None, "{path}");
        }
    }

    #[test]
    fn a_manifest_name_is_matched_exactly() {
        for path in ["package.json.bak", "my-package.json", "Cargo.toml.orig"] {
            assert_eq!(manifest_kind(path), None, "{path}");
        }
    }

    #[test]
    fn every_ecosystem_round_trips_through_its_name() {
        for ecosystem in [
            Ecosystem::Cargo,
            Ecosystem::Ci,
            Ecosystem::Go,
            Ecosystem::Npm,
            Ecosystem::Python,
        ] {
            assert_eq!(Ecosystem::parse(ecosystem.name()), Some(ecosystem));
        }
        assert_eq!(Ecosystem::parse("maven"), None);
    }

    #[test]
    fn the_resolver_output_directories_are_never_walked() {
        assert!(is_vendored("node_modules"));
        assert!(!is_vendored("packages"));
    }

    fn excludes(patterns: &[&str], filepath: &str) -> bool {
        let patterns: Vec<String> = patterns.iter().map(|p| (*p).to_string()).collect();
        Excludes::new(&patterns).matches(filepath)
    }

    #[test]
    fn a_bare_pattern_matches_the_basename_anywhere() {
        assert!(excludes(&["package.json"], "examples/package.json"));
        assert!(!excludes(&["package.json"], "Cargo.toml"));
    }

    #[test]
    fn a_pattern_with_a_slash_matches_the_path() {
        assert!(excludes(&["examples/**"], "examples/a/package.json"));
        assert!(!excludes(&["examples/**"], "src/package.json"));
    }

    /// A typo in a config must not silently hide the manifests this
    /// exists to compare. `[` is an unterminated character class, and
    /// the pattern beside it still works.
    #[test]
    fn an_uncompilable_pattern_excludes_nothing() {
        assert!(!excludes(&["["], "package.json"));
        assert!(!excludes(&["["], "["));
        assert!(excludes(&["[", "Cargo.toml"], "Cargo.toml"));
    }

    /// The glob syntax, pinned. `*` and `?` stop at a separator and `**`
    /// crosses one; a character class and an alternation mean what they
    /// do in every other glob a person writes.
    #[test]
    fn the_glob_syntax_is_the_one_ripgrep_uses() {
        assert!(excludes(&["*.json"], "a/package.json"));
        assert!(!excludes(&["src/*.json"], "src/a/package.json"));
        assert!(excludes(&["src/**/*.json"], "src/a/b/package.json"));
        assert!(excludes(&["**/vendor/**"], "a/vendor/b/Cargo.toml"));
        assert!(excludes(&["Cargo.???l"], "Cargo.toml"));
        assert!(!excludes(&["Cargo.?"], "Cargo.toml"));
        assert!(excludes(&["[Cc]argo.toml"], "Cargo.toml"));
        assert!(excludes(
            &["{examples,fixtures}/**"],
            "fixtures/a/Cargo.toml"
        ));
        assert!(!excludes(&["{examples,fixtures}/**"], "src/a/Cargo.toml"));
    }

    #[test]
    fn a_version_shaped_string_passes_the_evidence_gate() {
        for text in [
            "20", "1.1.0", "v4", ">=18", "^1.2.3", "*", "stable", "lts/*",
        ] {
            assert!(looks_like_a_version(text), "{text}");
        }
    }

    /// The gate that keeps a matrix expression out of the comparison.
    #[test]
    fn anything_else_is_not_evidence_of_a_version() {
        for text in [
            "${{ matrix.node }}",
            ".nvmrc",
            "[18, 20, 22]",
            "18, 20",
            "",
            "file",
        ] {
            assert!(!looks_like_a_version(text), "{text}");
        }
    }
}
