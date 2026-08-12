//! Finding the manifests in a tree.
//!
//! **This and `scan.rs` are the only modules allowed to touch the
//! filesystem.** The detection layer takes file contents and returns
//! entries, which is what lets the whole of it be tested from the corpus
//! with no temporary directories and no flake.
//!
//! Directories are walked with ripgrep's `ignore`, so "what this tool
//! finds" and "what ripgrep finds" are the same answer. A file named
//! explicitly is always read, whatever the ignore rules say.

use std::path::{Path, PathBuf};

use crate::detect::heuristics::{
    Ecosystem, ManifestKind, basename, is_vendored, manifest_kind, should_exclude,
};

#[derive(Debug, Clone)]
pub(crate) struct DiscoverOptions {
    /// Descend hidden **directories** other than `.github`.
    ///
    /// Workflows live in a hidden directory by definition, so a walk
    /// that skipped every dotted directory would find no CI pins at all
    /// — and the CI half is half the point of this tool.
    pub(crate) hidden: bool,
    pub(crate) respect_ignore: bool,
    pub(crate) exclude: Vec<String>,
    /// Empty means every ecosystem.
    pub(crate) ecosystems: Vec<Ecosystem>,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        Self {
            hidden: false,
            respect_ignore: true,
            exclude: Vec::new(),
            ecosystems: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) path: PathBuf,
    pub(crate) kind: ManifestKind,
}

/// The manifests under `root`, in a stable order.
///
/// The sort is not cosmetic: `ignore` makes no ordering guarantee, and a
/// report whose lines move between two runs over an unchanged tree
/// cannot be diffed — which is most of what a report in CI is for.
pub(crate) fn discover(root: &Path, options: &DiscoverOptions) -> Result<Vec<Found>, String> {
    let metadata =
        std::fs::metadata(root).map_err(|error| format!("{}: {error}", root.display()))?;
    if metadata.is_file() {
        // A file named explicitly is read whatever the ignore rules say:
        // intent beats configuration.
        let kind = manifest_kind(&normalise(root))
            .ok_or_else(|| format!("{}: not a manifest this tool reads", root.display()))?;
        return Ok(vec![Found {
            path: root.to_path_buf(),
            kind,
        }]);
    }

    let mut builder = ignore::WalkBuilder::new(root);
    builder
        // Dotfiles are visible at all times; `hidden` decides whether
        // the walk *descends* hidden directories, which is the question
        // a caller actually has.
        .hidden(false)
        .git_ignore(options.respect_ignore)
        .git_global(options.respect_ignore)
        .git_exclude(options.respect_ignore)
        .ignore(options.respect_ignore)
        .parents(options.respect_ignore)
        .follow_links(false)
        .filter_entry({
            let descend_hidden = options.hidden;
            move |entry| {
                let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
                !is_dir || descend(entry.path(), descend_hidden)
            }
        });

    let mut found = Vec::new();
    for entry in builder.build() {
        let entry = entry.map_err(|error| format!("{}: {error}", root.display()))?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.into_path();
        let relative = label(&path, root, false);
        let Some(kind) = manifest_kind(&relative) else {
            continue;
        };
        if should_exclude(&relative, &options.exclude) || !wanted(kind, &options.ecosystems) {
            continue;
        }
        found.push(Found { path, kind });
    }

    found.sort_by(|left, right| left.path.cmp(&right.path));
    found.dedup();
    Ok(found)
}

fn wanted(kind: ManifestKind, ecosystems: &[Ecosystem]) -> bool {
    ecosystems.is_empty() || ecosystems.contains(&kind.ecosystem())
}

fn descend(path: &Path, hidden: bool) -> bool {
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    if is_vendored(&name) {
        return false;
    }
    // `.github` is always descended: the workflows are in it, and a tool
    // that needed a flag to see them would ship with the CI half off.
    hidden || name == ".github" || !name.starts_with('.') || name == ".."
}

pub(crate) fn read(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let text =
        String::from_utf8(bytes).map_err(|_| format!("{}: not UTF-8 text", path.display()))?;
    // A leading byte-order mark is three invisible bytes that Notepad,
    // Excel and a PowerShell redirect all add. Before a `{` it makes a
    // JSON parser reject the whole document, which is indistinguishable
    // from a manifest with no dependencies in it.
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
}

/// How a manifest is named in the report.
///
/// Always relative to its root, always with forward slashes: the
/// separator reaches the report, so leaving it to the platform means the
/// same repository describes itself as `api/Cargo.toml` on one machine
/// and `api\Cargo.toml` on another, and anything diffing two runs sees a
/// change that is not one.
///
/// `qualify` prefixes the root, and is on only when more than one root
/// was named — two trees each holding a top-level `Cargo.toml` would
/// otherwise contribute two sites with the same label, which reads as
/// one file contradicting itself.
pub(crate) fn label(path: &Path, root: &Path, qualify: bool) -> String {
    if root.is_file() {
        return normalise(root);
    }
    let relative = path
        .strip_prefix(root)
        .map_or_else(|_| normalise(path), normalise);
    let root = normalise(root);
    if !qualify || root == "." || root.is_empty() {
        return relative;
    }
    format!("{}/{relative}", root.trim_end_matches('/'))
}

/// A path written with forward slashes.
///
/// Not `components().join("/")`: the root of an absolute path is itself
/// a component spelled `/`, so joining produces `//Users/…`. A label
/// that is not the path the caller typed is a label they cannot paste
/// back into a shell.
fn normalise(path: &Path) -> String {
    let mut out = String::new();
    for component in path.components() {
        let text = component.as_os_str().to_string_lossy();
        if matches!(text.as_ref(), "/" | "\\") {
            out.push('/');
            continue;
        }
        if !out.is_empty() && !out.ends_with('/') {
            out.push('/');
        }
        out.push_str(&text);
    }
    out
}

/// The basename of a path, for a diagnostic that has nowhere else to
/// point.
pub(crate) fn name_of(path: &Path) -> String {
    basename(&normalise(path)).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempTree;

    fn labels(found: &[Found], root: &Path) -> Vec<String> {
        found
            .iter()
            .map(|item| label(&item.path, root, false))
            .collect()
    }

    #[test]
    fn the_manifests_are_found_and_nothing_else_is() {
        let tree = TempTree::new("discover-basic");
        tree.write("package.json", "{}");
        tree.write("Cargo.toml", "");
        tree.write("crates/api/Cargo.toml", "");
        tree.write("README.md", "not a manifest");
        tree.write("Cargo.lock", "");
        let found = discover(tree.path(), &DiscoverOptions::default()).expect("walks");
        assert_eq!(
            labels(&found, tree.path()),
            ["Cargo.toml", "crates/api/Cargo.toml", "package.json"]
        );
    }

    /// The whole CI half depends on this. `.github` is a hidden
    /// directory, and a walk that skipped it would find no workflow.
    #[test]
    fn the_workflow_directory_is_walked_without_asking() {
        let tree = TempTree::new("discover-workflows");
        tree.write(".github/workflows/ci.yml", "jobs:\n");
        tree.write(".github/dependabot.yml", "version: 2\n");
        let found = discover(tree.path(), &DiscoverOptions::default()).expect("walks");
        assert_eq!(labels(&found, tree.path()), [".github/workflows/ci.yml"]);
    }

    #[test]
    fn other_hidden_directories_are_skipped_unless_asked_for() {
        let tree = TempTree::new("discover-hidden");
        tree.write("Cargo.toml", "");
        tree.write(".cache/Cargo.toml", "");
        assert_eq!(
            discover(tree.path(), &DiscoverOptions::default())
                .expect("walks")
                .len(),
            1
        );
        assert_eq!(
            discover(
                tree.path(),
                &DiscoverOptions {
                    hidden: true,
                    ..DiscoverOptions::default()
                }
            )
            .expect("walks")
            .len(),
            2
        );
    }

    /// A `package.json` per installed package is the resolver's output,
    /// not this repository's constraints.
    #[test]
    fn node_modules_is_never_walked() {
        let tree = TempTree::new("discover-vendored");
        tree.write("package.json", "{}");
        tree.write("node_modules/left-pad/package.json", "{}");
        let found = discover(tree.path(), &DiscoverOptions::default()).expect("walks");
        assert_eq!(labels(&found, tree.path()), ["package.json"]);
    }

    #[test]
    fn an_ecosystem_filter_narrows_the_walk() {
        let tree = TempTree::new("discover-filter");
        tree.write("package.json", "{}");
        tree.write("Cargo.toml", "");
        let found = discover(
            tree.path(),
            &DiscoverOptions {
                ecosystems: vec![Ecosystem::Cargo],
                ..DiscoverOptions::default()
            },
        )
        .expect("walks");
        assert_eq!(labels(&found, tree.path()), ["Cargo.toml"]);
    }

    #[test]
    fn ignored_files_are_skipped() {
        let tree = TempTree::new("discover-ignore");
        tree.mkdir(".git");
        tree.write(".gitignore", "generated/\n");
        tree.write("Cargo.toml", "");
        tree.write("generated/Cargo.toml", "");
        let found = discover(tree.path(), &DiscoverOptions::default()).expect("walks");
        assert_eq!(labels(&found, tree.path()), ["Cargo.toml"]);
    }

    #[test]
    fn an_exclude_pattern_hides_a_manifest() {
        let tree = TempTree::new("discover-exclude");
        tree.write("Cargo.toml", "");
        tree.write("examples/demo/Cargo.toml", "");
        let found = discover(
            tree.path(),
            &DiscoverOptions {
                exclude: vec!["examples/**".to_string()],
                ..DiscoverOptions::default()
            },
        )
        .expect("walks");
        assert_eq!(labels(&found, tree.path()), ["Cargo.toml"]);
    }

    #[test]
    fn a_named_file_is_the_whole_walk() {
        let tree = TempTree::new("discover-one");
        let file = tree.write("Cargo.toml", "");
        let found = discover(&file, &DiscoverOptions::default()).expect("walks");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ManifestKind::CargoToml);
    }

    #[test]
    fn a_named_file_that_is_not_a_manifest_is_refused_by_name() {
        let tree = TempTree::new("discover-not-a-manifest");
        let file = tree.write("README.md", "");
        let error = discover(&file, &DiscoverOptions::default()).expect_err("a refusal");
        assert!(error.contains("README.md"), "{error}");
    }

    #[test]
    fn a_missing_root_is_refused_by_name() {
        let tree = TempTree::new("discover-missing");
        let error = discover(&tree.path().join("nope"), &DiscoverOptions::default())
            .expect_err("a refusal");
        assert!(error.contains("nope"), "{error}");
    }

    #[test]
    fn the_order_is_stable_between_runs() {
        let tree = TempTree::new("discover-order");
        for name in ["z/Cargo.toml", "a/Cargo.toml", "m/package.json"] {
            tree.write(name, "");
        }
        let first = discover(tree.path(), &DiscoverOptions::default()).expect("walks");
        let again = discover(tree.path(), &DiscoverOptions::default()).expect("walks");
        assert_eq!(first, again);
        assert_eq!(
            labels(&first, tree.path()),
            ["a/Cargo.toml", "m/package.json", "z/Cargo.toml"]
        );
    }

    /// Two trees each holding a top-level `Cargo.toml` must not
    /// contribute two sites with the same label — that reads as one file
    /// contradicting itself.
    #[test]
    fn more_than_one_root_qualifies_the_label() {
        let tree = TempTree::new("discover-label");
        let file = tree.write("api/Cargo.toml", "");
        let root = tree.path().join("api");
        assert_eq!(label(&file, &root, false), "Cargo.toml");
        assert!(label(&file, &root, true).ends_with("api/Cargo.toml"));
        assert!(!label(&file, &root, true).contains('\\'));
    }

    /// Regression: the root of an absolute path is itself a component
    /// spelled `/`, so joining components produced `//Users/…` — a
    /// label nobody can paste back into a shell.
    #[test]
    fn an_absolute_file_root_is_labelled_with_one_leading_slash() {
        let tree = TempTree::new("discover-absolute");
        let file = tree.write("Cargo.toml", "");
        let labelled = label(&file, &file, false);
        assert!(labelled.starts_with('/'), "{labelled}");
        assert!(!labelled.starts_with("//"), "{labelled}");
        assert!(labelled.ends_with("/Cargo.toml"), "{labelled}");
    }

    #[test]
    fn a_byte_order_mark_is_not_part_of_the_document() {
        let tree = TempTree::new("discover-bom");
        let file = tree.write("package.json", "\u{feff}{ \"name\": \"a\" }");
        let text = read(&file).expect("reads");
        assert!(text.starts_with('{'), "{text:?}");
    }

    #[test]
    fn a_file_that_is_not_text_is_refused_by_name() {
        let tree = TempTree::new("discover-binary");
        let file = tree.path().join("package.json");
        std::fs::write(&file, [0xff, 0xfe, 0x00]).expect("a file");
        let error = read(&file).expect_err("a refusal");
        assert!(error.contains("not UTF-8"), "{error}");
    }
}
