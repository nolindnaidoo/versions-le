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

use std::path::{Component, Path, PathBuf, Prefix};

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
    hidden || name == ".github" || !name.starts_with('.')
}

/// A leading byte-order mark is three invisible bytes that Notepad, Excel
/// and a PowerShell redirect all add. Before a `{` it makes a JSON parser
/// reject the whole document, which is indistinguishable from a manifest
/// with no dependencies in it.
const BYTE_ORDER_MARK: char = '\u{feff}';

pub(crate) fn read(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut text =
        String::from_utf8(bytes).map_err(|_| format!("{}: not UTF-8 text", path.display()))?;
    // Removed in place rather than by copying the tail into a second
    // string: a manifest can be tens of megabytes, and every one of them
    // would otherwise pay for the three bytes almost none of them carry.
    if text.starts_with(BYTE_ORDER_MARK) {
        text.replace_range(..BYTE_ORDER_MARK.len_utf8(), "");
    }
    Ok(text)
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
///
/// A Windows **prefix** is the one component whose own text carries
/// separators, and it is the case that got through: `canonicalize`
/// returns an extended-length path on Windows, so every absolute path
/// arrived as `\\?\C:\…` and left as `\\?\C:/…` — backslashes in the one
/// string this function promises has none.
///
/// The `match` below is **exhaustive on purpose**, and that is the guard
/// rather than the fix. Only Windows ever parses a `Prefix` out of a
/// string, so a version of this function that quietly ignored one was
/// green on two of the three legs; an exhaustive match compiles on every
/// platform, so deleting the prefix arm is a build failure on all of
/// them. A wildcard here would put the bug back.
pub(crate) fn normalise(path: &Path) -> String {
    let mut out = String::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push_str(&designator(prefix.kind())),
            // Guarded rather than unconditional: a UNC designator already
            // ends in a share name, and `//server/share//a` would be a
            // second identity for one file.
            Component::RootDir => {
                if !out.ends_with('/') {
                    out.push('/');
                }
            }
            Component::CurDir | Component::ParentDir | Component::Normal(_) => {
                if !out.is_empty() && !out.ends_with('/') {
                    out.push('/');
                }
                out.push_str(&component.as_os_str().to_string_lossy());
            }
        }
    }
    out
}

/// The forward-slashed designator for a Windows path prefix.
///
/// **A file gets one identity.** Windows spells the same file several
/// ways — `C:\a`, `c:\a`, and the `\\?\C:\a` that `canonicalize`
/// returns — and a label that kept them apart would report one manifest
/// under two names, which is the "one file contradicting itself"
/// problem `qualify` exists to prevent. So every disk prefix collapses
/// to an upper-cased `C:`, verbatim marker dropped: drive letters are
/// case-insensitive on Windows, so two spellings of one drive are one
/// drive.
///
/// **A UNC prefix keeps its host and share**, as `//server/share`. That
/// is the opposite mistake and the more dangerous one: dropping them
/// would collapse `\\a\share\Cargo.toml` and `\\b\share\Cargo.toml` into
/// a single label, and the report would claim two different files were
/// the same one — a fabricated conflict, which is the output this crate
/// cannot afford. `\\?\UNC\server\share` is the same share as
/// `\\server\share`, so both give the same designator.
///
/// A device namespace (`\\.\COM1`) and a non-disk verbatim path
/// (`\\?\Volume{…}`) are not places a manifest is read from. Neither is
/// invented into something else: each keeps its namespace marker with
/// the separators turned round, so it stays recognisable and cannot
/// collide with a UNC share — `?` and `.` are not legal host names.
fn designator(prefix: Prefix<'_>) -> String {
    match prefix {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            format!("{}:", letter.to_ascii_uppercase() as char)
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            format!("//{}/{}", server.to_string_lossy(), share.to_string_lossy())
        }
        Prefix::DeviceNS(name) => format!("//./{}", name.to_string_lossy()),
        Prefix::Verbatim(name) => format!("//?/{}", name.to_string_lossy()),
    }
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

    /// Regression, twice over. The root of an absolute path is itself a
    /// component spelled `/`, so joining components produced `//Users/…`;
    /// and on Windows `canonicalize` returns an extended-length path, so
    /// the same label came back as
    /// `\\?\C:/Users/runneradmin/…/Cargo.toml` — red on the Windows leg
    /// alone, because no other platform has a prefix component at all.
    ///
    /// The assertion is written for both: one separator run, no
    /// backslash, and a start this platform can actually produce. The
    /// exact shapes are pinned by `every_windows_prefix_shape_…` below,
    /// which needs no Windows to run.
    #[test]
    fn an_absolute_file_root_is_labelled_with_one_separator_run() {
        let tree = TempTree::new("discover-absolute");
        let file = tree.write("Cargo.toml", "");
        let labelled = label(&file, &file, false);
        assert!(!labelled.contains('\\'), "{labelled}");
        assert!(!labelled.contains("//"), "{labelled}");
        assert!(labelled.ends_with("/Cargo.toml"), "{labelled}");

        // Rooted where a temporary directory is rooted, drive-designated
        // where it is on a drive, and nothing else. A temp directory is
        // never a UNC share on any runner this ships to.
        let rooted = labelled.starts_with('/');
        let drive = labelled.split_once(":/").is_some_and(|(head, _)| {
            head.len() == 1 && head.starts_with(|c: char| c.is_ascii_uppercase())
        });
        assert!(rooted || drive, "{labelled}");
    }

    /// **Every prefix shape Windows can parse, built as a literal.**
    ///
    /// The defect this pins was asserted before and still shipped: the
    /// property "a label carries no backslash" was only ever tested
    /// against paths the running machine could produce, and only Windows
    /// produces a prefix. `Prefix` is constructible on every platform
    /// even though only Windows parses one, so these run everywhere —
    /// which is the whole point.
    #[test]
    fn every_windows_prefix_shape_is_one_forward_slashed_designator() {
        use std::ffi::OsStr;

        let server = OsStr::new("server");
        let share = OsStr::new("share");
        for (prefix, expected) in [
            (Prefix::Disk(b'C'), "C:"),
            (Prefix::Disk(b'c'), "C:"),
            (Prefix::VerbatimDisk(b'C'), "C:"),
            (Prefix::UNC(server, share), "//server/share"),
            (Prefix::VerbatimUNC(server, share), "//server/share"),
            (Prefix::DeviceNS(OsStr::new("COM1")), "//./COM1"),
            (Prefix::Verbatim(OsStr::new("Volume{0}")), "//?/Volume{0}"),
        ] {
            let designated = designator(prefix);
            assert_eq!(designated, expected, "{prefix:?}");
            assert!(!designated.contains('\\'), "{prefix:?} kept a backslash");
        }
    }

    /// One file, one identity. `canonicalize` hands back the verbatim
    /// spelling and a caller hands back the plain one; a label that told
    /// them apart would report one manifest twice.
    #[test]
    fn a_verbatim_drive_and_a_plain_drive_are_the_same_identity() {
        assert_eq!(
            designator(Prefix::VerbatimDisk(b'C')),
            designator(Prefix::Disk(b'C'))
        );
        assert_eq!(
            designator(Prefix::VerbatimUNC(
                std::ffi::OsStr::new("server"),
                std::ffi::OsStr::new("share")
            )),
            designator(Prefix::UNC(
                std::ffi::OsStr::new("server"),
                std::ffi::OsStr::new("share")
            ))
        );
    }

    /// Two different shares must not become one label — that would be a
    /// fabricated conflict between two files that never met.
    #[test]
    fn two_unc_shares_keep_two_identities() {
        use std::ffi::OsStr;

        let one = designator(Prefix::UNC(OsStr::new("alpha"), OsStr::new("share")));
        let other = designator(Prefix::UNC(OsStr::new("beta"), OsStr::new("share")));
        assert_ne!(one, other);
        assert_ne!(
            one,
            designator(Prefix::UNC(OsStr::new("alpha"), OsStr::new("other")))
        );
    }

    /// The integration the literals above cannot reach: only Windows
    /// parses a prefix out of a string, so only Windows can prove
    /// `normalise` puts the designator back together with the rest.
    #[cfg(windows)]
    #[test]
    fn windows_paths_normalise_to_one_label_per_file() {
        for (raw, expected) in [
            (r"\\?\C:\a\Cargo.toml", "C:/a/Cargo.toml"),
            (r"C:\a\Cargo.toml", "C:/a/Cargo.toml"),
            (r"c:\a\Cargo.toml", "C:/a/Cargo.toml"),
            (
                r"\\server\share\a\Cargo.toml",
                "//server/share/a/Cargo.toml",
            ),
            (
                r"\\?\UNC\server\share\a\Cargo.toml",
                "//server/share/a/Cargo.toml",
            ),
            (r"a\b\Cargo.toml", "a/b/Cargo.toml"),
            (r".github\workflows\ci.yml", ".github/workflows/ci.yml"),
        ] {
            assert_eq!(normalise(Path::new(raw)), expected, "{raw}");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn windows_paths_normalise_to_one_label_per_file() {
        // Only Windows parses `\\?\C:\a` into a prefix and a root; here it
        // is one ordinary filename. The designator logic is covered on
        // every platform by the three tests above.
        eprintln!("SKIPPED windows_paths_normalise_to_one_label_per_file: not a Windows target");
    }

    /// The prefix-free half of the same property, pinned as literals so
    /// it does not depend on what the temporary directory happens to
    /// look like on the machine running the suite.
    ///
    /// A leading `.` survives on purpose: it is what the caller typed,
    /// and `label` has its own rule for a root spelled `.`. A rooted
    /// path keeps exactly one leading slash — the original regression.
    #[test]
    fn prefix_free_paths_normalise_to_what_the_caller_typed() {
        for (raw, expected) in [
            ("a/b/Cargo.toml", "a/b/Cargo.toml"),
            ("./a/Cargo.toml", "./a/Cargo.toml"),
            ("Cargo.toml", "Cargo.toml"),
            ("/a/b/Cargo.toml", "/a/b/Cargo.toml"),
            ("/Cargo.toml", "/Cargo.toml"),
            ("a//b/Cargo.toml", "a/b/Cargo.toml"),
            ("../a/Cargo.toml", "../a/Cargo.toml"),
        ] {
            assert_eq!(normalise(Path::new(raw)), expected, "{raw}");
        }
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
