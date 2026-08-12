//! One analysis end to end — the only path either surface calls.
//!
//! `cli.rs` and `mcp/` both come through here, so a check can only be
//! written once. `tests/contracts.rs` asserts the two agree.
//!
//! The report is one object for the whole run, not one line per
//! manifest: the answer is about the *set* of manifests, which is the
//! entire value of a cross-file validator.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::detect::compare::{self, Finding};
use crate::detect::heuristics::{Ecosystem, ManifestKind};
use crate::detect::parser::{self, Entry, Refusal};
use crate::discover::{self, DiscoverOptions};

/// The report's shape version. Bumped when a consumer parsing it would
/// have to change; carried from day one so there is never a report a
/// reader has to sniff.
const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ManifestSummary {
    pub(crate) path: String,
    pub(crate) ecosystem: Ecosystem,
    pub(crate) entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Diagnostic {
    pub(crate) severity: String,
    pub(crate) code: String,
    pub(crate) file: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Summary {
    pub(crate) manifests: usize,
    pub(crate) entries: usize,
    pub(crate) findings: usize,
    pub(crate) refusals: usize,
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
    pub(crate) infos: usize,
}

/// **No timestamp.** A report is a thing to diff — against the previous
/// run, against a baseline in review — and a timestamp makes every run
/// differ from every other, which defeats the only reason to keep one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Report {
    pub(crate) schema: u32,
    pub(crate) manifests: Vec<ManifestSummary>,
    pub(crate) findings: Vec<Finding>,
    pub(crate) refusals: Vec<Refusal>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) summary: Summary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum FailOn {
    /// Exit 1 on anything the tool calls a defect, and stay quiet about
    /// the choices — a floating pin is `info` and does not fail a build
    /// that has decided to accept one.
    #[default]
    Conflict,
    /// Exit 1 on any finding at all.
    Any,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ScanOptions {
    pub(crate) discover: DiscoverOptions,
}

/// One document as either surface supplies it: what kind of manifest it
/// is, what it is called in the report, and its text.
pub(crate) struct Document {
    pub(crate) kind: ManifestKind,
    pub(crate) label: String,
    pub(crate) content: String,
}

/// Analyse the manifests under each root.
pub(crate) fn scan(roots: &[PathBuf], options: &ScanOptions) -> Result<Report, String> {
    let qualify = roots.len() > 1;
    let mut documents = Vec::new();
    let mut unread = Vec::new();

    for root in roots {
        for found in discover::discover(root, &options.discover)? {
            let label = discover::label(&found.path, root, qualify);
            // One unreadable manifest does not abort the analysis of the
            // rest — a `package.json` that is not text says nothing
            // about whether the real ones agree. It is named in the
            // diagnostics so the answer is not quietly narrower than it
            // looks.
            match discover::read(&found.path) {
                Ok(content) => documents.push(Document {
                    kind: found.kind,
                    label,
                    content,
                }),
                Err(message) => unread.push((discover::name_of(&found.path), message)),
            }
        }
    }

    let mut report = report_for(&documents);
    report
        .diagnostics
        .extend(unread.into_iter().map(|(file, message)| Diagnostic {
            severity: "error".to_string(),
            code: "unreadable".to_string(),
            file,
            message,
        }));
    report.summary = summarise(&report);
    Ok(report)
}

pub(crate) fn report_for(documents: &[Document]) -> Report {
    let mut entries: Vec<Entry> = Vec::new();
    let mut refusals: Vec<Refusal> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut manifests: Vec<ManifestSummary> = Vec::new();

    for document in documents {
        let parsed = parser::parse(document.kind, &document.label, &document.content);
        manifests.push(ManifestSummary {
            path: document.label.clone(),
            ecosystem: document.kind.ecosystem(),
            entries: parsed.entries.len(),
        });
        diagnostics.extend(parsed.errors.iter().map(|error| Diagnostic {
            severity: "error".to_string(),
            code: error.kind.clone(),
            file: document.label.clone(),
            message: error.message.clone(),
        }));
        entries.extend(parsed.entries);
        refusals.extend(parsed.refusals);
    }

    let (findings, crossings) = compare::analyse(&entries);
    refusals.extend(crossings);
    let refusals = merge_refusals(refusals);

    let mut report = Report {
        schema: SCHEMA,
        manifests,
        findings,
        refusals,
        diagnostics,
        summary: Summary {
            entries: entries.len(),
            ..Summary::default()
        },
    };
    report.summary = summarise(&report);
    report
}

/// One refusal per (reason, name), carrying every site.
///
/// The readers emit a refusal per entry, because that is where the
/// evidence is. Sixteen rows saying `actions/checkout` is SHA-pinned is
/// the same one fact sixteen times, and a report a reader skims past is
/// a report that hid its own findings. Findings are grouped the same way
/// for the same reason.
fn merge_refusals(refusals: Vec<Refusal>) -> Vec<Refusal> {
    let mut merged: Vec<Refusal> = Vec::new();
    for refusal in refusals {
        let same = merged.iter_mut().find(|held| {
            held.reason == refusal.reason
                && held.name == refusal.name
                && held.ecosystem == refusal.ecosystem
                && held.message == refusal.message
        });
        match same {
            Some(held) => held.sites.extend(refusal.sites),
            None => merged.push(refusal),
        }
    }
    merged.sort_by(|left, right| {
        left.reason
            .cmp(&right.reason)
            .then_with(|| left.name.cmp(&right.name))
    });
    merged
}

/// The counts, derived from the report rather than tallied alongside it,
/// so a summary can never disagree with the lists it summarises. Only
/// `entries` is carried in, because an entry the comparison ignored
/// leaves no trace in any of the lists.
fn summarise(report: &Report) -> Summary {
    let severity = |wanted: &str| {
        report
            .findings
            .iter()
            .filter(|finding| finding.severity == wanted)
            .count()
            + report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == wanted)
                .count()
    };
    Summary {
        manifests: report.manifests.len(),
        entries: report.summary.entries,
        findings: report.findings.len(),
        refusals: report.refusals.len(),
        errors: severity("error"),
        warnings: severity("warning"),
        infos: severity("info"),
    }
}

/// 0 clean, 1 findings, 2 the question could not be answered.
///
/// **This crate has a verdict**, and the exit code is most of the
/// product. `--strict` is the pipeline that wants none of the tool's
/// silence: a refusal or an unreadable manifest means part of the tree
/// went unanalysed, and exit 2 says so rather than letting a narrower
/// answer read as a clean one.
///
/// The refusals that *are* the answer never trigger it — see
/// `Refusal::is_the_answer`, which is the one place that list lives.
pub(crate) fn exit_code(report: &Report, fail_on: FailOn, strict: bool) -> u8 {
    let unanalysed = !report.diagnostics.is_empty()
        || report
            .refusals
            .iter()
            .any(|refusal| !refusal.is_the_answer());
    if strict && unanalysed {
        return 2;
    }
    let failing = report.findings.iter().any(|finding| match fail_on {
        FailOn::Conflict => finding.severity != "info",
        FailOn::Any => true,
    });
    u8::from(failing)
}

/// Classify a path the caller named, for the surfaces that take paths
/// rather than a tree.
///
/// Through `discover::normalise` rather than its own `components()` walk:
/// the two were the same job written twice, and the copy here joined a
/// Windows prefix and its root separator into `\\?\C:/\/a/…` before
/// classifying it. Nothing broke — `manifest_kind` only reads the
/// basename and looks for `/.github/workflows/` — but a second spelling
/// of one path is a divergence waiting for a rule that does care.
pub(crate) fn manifest_of(path: &Path) -> Option<ManifestKind> {
    crate::detect::heuristics::manifest_kind(&discover::normalise(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempTree;

    fn roots(tree: &TempTree) -> Vec<PathBuf> {
        vec![tree.path().to_path_buf()]
    }

    fn scan_tree(tree: &TempTree) -> Report {
        scan(&roots(tree), &ScanOptions::default()).expect("scans")
    }

    fn codes(report: &Report) -> Vec<&str> {
        report
            .findings
            .iter()
            .map(|finding| finding.code.as_str())
            .collect()
    }

    #[test]
    fn a_tree_that_agrees_is_clean_and_exits_zero() {
        let tree = TempTree::new("scan-clean");
        tree.write("a/Cargo.toml", "[dependencies]\nserde = \"1.0.200\"\n");
        tree.write("b/Cargo.toml", "[dependencies]\nserde = \"1.0.200\"\n");
        let report = scan_tree(&tree);
        assert!(report.findings.is_empty(), "{:?}", report.findings);
        assert_eq!(report.summary.manifests, 2);
        assert_eq!(exit_code(&report, FailOn::Conflict, false), 0);
    }

    #[test]
    fn a_drifted_dependency_exits_one() {
        let tree = TempTree::new("scan-drift");
        tree.write("a/Cargo.toml", "[dependencies]\nserde = \"1.0.200\"\n");
        tree.write("b/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        let report = scan_tree(&tree);
        assert_eq!(codes(&report), ["constraint-conflict"]);
        assert_eq!(exit_code(&report, FailOn::Conflict, false), 1);
    }

    /// No manifests at all is not a failure. There is nothing to be in
    /// conflict with, and failing a build over it would be the tool
    /// inventing a problem.
    #[test]
    fn no_manifests_is_clean() {
        let tree = TempTree::new("scan-empty");
        tree.write("README.md", "nothing here");
        let report = scan_tree(&tree);
        assert_eq!(report.summary.manifests, 0);
        assert_eq!(exit_code(&report, FailOn::Conflict, false), 0);
    }

    /// A floating pin is a choice with a cost, so it is visible without
    /// being fatal — until a repository says otherwise.
    #[test]
    fn a_floating_pin_fails_only_under_fail_on_any() {
        let tree = TempTree::new("scan-floating");
        tree.write("package.json", r#"{ "dependencies": { "a": "latest" } }"#);
        let report = scan_tree(&tree);
        assert_eq!(codes(&report), ["floating-pin"]);
        assert_eq!(exit_code(&report, FailOn::Conflict, false), 0);
        assert_eq!(exit_code(&report, FailOn::Any, false), 1);
    }

    /// The refusal must be visible in the report even though it changes
    /// no finding — otherwise a narrower answer reads as a clean one.
    #[test]
    fn a_refusal_is_carried_and_only_strict_makes_it_fatal() {
        let tree = TempTree::new("scan-refusal");
        tree.write(
            "Cargo.toml",
            "[dependencies]\nshared = { workspace = true }\n",
        );
        let report = scan_tree(&tree);
        assert_eq!(report.refusals.len(), 1);
        assert_eq!(report.refusals[0].reason, "unknown_grammar");
        assert_eq!(exit_code(&report, FailOn::Conflict, false), 0);
        assert_eq!(exit_code(&report, FailOn::Conflict, true), 2);
    }

    /// Regression: one refusal per occurrence made a real repository's
    /// report sixteen rows of the same fact, which is a report a reader
    /// skims past.
    #[test]
    fn refusals_for_one_name_merge_into_one_row_with_every_site() {
        let tree = TempTree::new("scan-merge");
        tree.write(
            ".github/workflows/a.yml",
            "  - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683\n",
        );
        tree.write(
            ".github/workflows/b.yml",
            "  - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683\n",
        );
        let report = scan_tree(&tree);
        assert_eq!(report.refusals.len(), 1);
        assert_eq!(report.refusals[0].name, "actions/checkout");
        assert_eq!(report.refusals[0].sites.len(), 2);
    }

    /// `cross_ecosystem` is not a failure to answer — it *is* the
    /// answer, so it never trips `--strict`.
    #[test]
    fn a_cross_ecosystem_refusal_never_fails_the_run() {
        let tree = TempTree::new("scan-cross");
        tree.write("Cargo.toml", "[dependencies]\nsemver = \"1.0.28\"\n");
        tree.write("package.json", r#"{ "dependencies": { "semver": "7" } }"#);
        let report = scan_tree(&tree);
        assert_eq!(report.refusals.len(), 1);
        assert_eq!(report.refusals[0].reason, "cross_ecosystem");
        assert_eq!(exit_code(&report, FailOn::Conflict, true), 0);
    }

    #[test]
    fn a_manifest_that_does_not_parse_is_a_diagnostic_naming_it() {
        let tree = TempTree::new("scan-broken");
        tree.write("package.json", "{ not json");
        tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        let report = scan_tree(&tree);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].file, "package.json");
        assert_eq!(exit_code(&report, FailOn::Conflict, true), 2);
    }

    #[test]
    fn the_report_carries_its_schema_and_no_timestamp() {
        let tree = TempTree::new("scan-schema");
        tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        let rendered = serde_json::to_string(&scan_tree(&tree)).expect("a report serializes");
        assert!(rendered.contains("\"schema\":1"), "{rendered}");
        for absent in ["timestamp", "lastChecked", "generatedAt"] {
            assert!(!rendered.contains(absent), "{rendered}");
        }
    }

    /// Two trees, each with a top-level manifest, must not contribute
    /// two sites with the same label.
    #[test]
    fn two_roots_are_analysed_together_with_distinct_labels() {
        let tree = TempTree::new("scan-two-roots");
        tree.write("api/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        tree.write("web/Cargo.toml", "[dependencies]\nserde = \"2\"\n");
        let report = scan(
            &[tree.path().join("api"), tree.path().join("web")],
            &ScanOptions::default(),
        )
        .expect("scans");
        assert_eq!(codes(&report), ["disjoint-constraint"]);
        let labels: Vec<&str> = report
            .manifests
            .iter()
            .map(|manifest| manifest.path.as_str())
            .collect();
        assert_ne!(labels[0], labels[1], "{labels:?}");
    }

    #[test]
    fn a_missing_root_is_refused() {
        let tree = TempTree::new("scan-missing");
        assert!(scan(&[tree.path().join("nope")], &ScanOptions::default()).is_err());
    }

    /// Classification goes through the same path spelling the labels do.
    ///
    /// This is a tidy-up rather than a fix: the old `components()` walk
    /// here joined an absolute path into `//a/…` — and, on Windows, a
    /// prefix and its root separator into `\\?\C:/\/a/…` — but
    /// `manifest_kind` only ever reads the basename and looks for
    /// `/.github/workflows/`, so both spellings classified alike. The
    /// duplicate is gone because the next rule that cares about the rest
    /// of the path would have found only one of the two copies.
    #[test]
    fn a_named_manifest_is_classified_whatever_the_path_shape() {
        assert!(manifest_of(Path::new("a/Cargo.toml")).is_some());
        assert!(manifest_of(Path::new("a/README.md")).is_none());
        for (path, kind) in [
            ("/Cargo.toml", ManifestKind::CargoToml),
            ("./package.json", ManifestKind::PackageJson),
            ("/a/.github/workflows/ci.yml", ManifestKind::Workflow),
            (".github/workflows/ci.yml", ManifestKind::Workflow),
        ] {
            assert_eq!(manifest_of(Path::new(path)), Some(kind), "{path}");
        }
        // A workflow is decided by its directory, and an absolute path
        // must not smuggle one in.
        assert_eq!(manifest_of(Path::new("/a/deploy/k8s.yml")), None);
    }
}
