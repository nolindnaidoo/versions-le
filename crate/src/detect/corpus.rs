//! The corpus, embedded and run as unit tests.
//!
//! `fixtures/` ships inside the crate so `cargo test` on the published
//! tarball runs every case: whoever installed this can check the
//! refusal claims in the README instead of trusting them.
//!
//! Three groups, and the last two are the point. `tree-*` is one
//! synthetic repository carrying a planted instance of every finding
//! this tool makes. `grammar-*` is the opposite: pairs of manifests that
//! a tool comparing strings would call a conflict, pinned here as
//! **refusals**. A regression that started guessing at `workspace = true`
//! would turn a refusal into a finding, and the corpus is what fails.
//!
//! `pin-*` is the finding a real repository actually had: a Cargo `path`
//! dependency whose bare `version = "0.7.7"` is a **caret** requirement —
//! `[0.7.7, 0.8.0)` — and not the exact pin its author believed they had
//! written. `=0.7.7` is the exact one. The pair is here so the difference
//! cannot regress into "both are pins".

/// One corpus document, by its **logical** path.
///
/// The paths are `api/Cargo.toml`, `.github/workflows/ci.yml` and so on,
/// because classification reads the path and it is one of the things
/// under test. The files on disk are flat and dot-free: `cargo package`
/// skips dotfiles, so a corpus containing a real `.github/` directory
/// would ship a crate that cannot run its own tests.
///
/// Panics on a path the corpus does not carry — a test naming a file
/// that is not there is a broken test, not a runtime condition.
///
/// **Compiled only under `cfg(test)`.** Nothing outside the tests reads a
/// fixture, so a shipped binary carries neither this function nor the
/// documents it embeds — and the crate keeps its rule of no inline lint
/// suppression, which the alternative (a `dead_code` exemption for the
/// non-test build) would have cost it.
#[cfg(test)]
pub(crate) fn document(path: &str) -> &'static str {
    match path {
        // The planted tree.
        "api/Cargo.toml" => include_str!("../../fixtures/documents/tree-api-Cargo.toml"),
        "web/Cargo.toml" => include_str!("../../fixtures/documents/tree-web-Cargo.toml"),
        "package.json" => include_str!("../../fixtures/documents/tree-package.json"),
        ".github/workflows/ci.yml" => include_str!("../../fixtures/documents/tree-workflow.yml"),

        // One file per grammar the tool declines to model.
        "crates/base/Cargo.toml" => {
            include_str!("../../fixtures/documents/grammar-cargo-base.toml")
        }
        "crates/inherited/Cargo.toml" => {
            include_str!("../../fixtures/documents/grammar-cargo-workspace.toml")
        }
        "crates/local/Cargo.toml" => {
            include_str!("../../fixtures/documents/grammar-cargo-path.toml")
        }
        "crates/remote/Cargo.toml" => {
            include_str!("../../fixtures/documents/grammar-cargo-git.toml")
        }
        "apps/base/package.json" => include_str!("../../fixtures/documents/grammar-npm-base.json"),
        "apps/inherited/package.json" => {
            include_str!("../../fixtures/documents/grammar-npm-workspace.json")
        }
        "apps/remote/package.json" => include_str!("../../fixtures/documents/grammar-npm-git.json"),
        "apps/tagged/package.json" => include_str!("../../fixtures/documents/grammar-npm-tag.json"),
        "svc/base/pyproject.toml" => {
            include_str!("../../fixtures/documents/grammar-python-base.toml")
        }
        "svc/compatible/pyproject.toml" => {
            include_str!("../../fixtures/documents/grammar-python-compatible.toml")
        }
        "svc/marker/pyproject.toml" => {
            include_str!("../../fixtures/documents/grammar-python-marker.toml")
        }
        ".github/workflows/build.yml" => {
            include_str!("../../fixtures/documents/grammar-ci-base.yml")
        }
        ".github/workflows/release.yml" => {
            include_str!("../../fixtures/documents/grammar-ci-sha.yml")
        }
        ".github/workflows/matrix.yml" => {
            include_str!("../../fixtures/documents/grammar-ci-matrix.yml")
        }
        ".github/workflows/jobs.yml" => {
            include_str!("../../fixtures/documents/grammar-ci-jobs.yml")
        }
        "go.mod" => include_str!("../../fixtures/documents/grammar-go.mod"),

        // The pin pair. Same package, same source, one character apart.
        "crates/caret/Cargo.toml" => {
            include_str!("../../fixtures/documents/pin-cargo-path-caret.toml")
        }
        "crates/exact/Cargo.toml" => {
            include_str!("../../fixtures/documents/pin-cargo-path-exact.toml")
        }

        other => panic!("the corpus has no document at {other}"),
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::document;
    use crate::detect::compare::analyse;
    use crate::detect::heuristics::{Ecosystem, ManifestKind, manifest_kind};
    use crate::detect::parser::{self, Entry, Refusal};

    const CORPUS: &str = include_str!("../../fixtures/detection.json");

    #[derive(Debug, Deserialize)]
    struct Corpus {
        classification: Vec<ClassificationCase>,
        extraction: Vec<ExtractionCase>,
        analysis: Vec<AnalysisCase>,
    }

    #[derive(Debug, Deserialize)]
    struct ClassificationCase {
        path: String,
        ecosystem: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct ExtractionCase {
        file: String,
        entries: Vec<String>,
        refusals: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct AnalysisCase {
        name: String,
        files: Vec<String>,
        findings: Vec<String>,
        refusals: Vec<String>,
    }

    fn corpus() -> Corpus {
        serde_json::from_str(CORPUS).expect("the corpus is valid JSON")
    }

    /// `<ecosystem> <kind> <key> <name>=<constraint>` — compact enough
    /// to read in a diff, complete enough that a reader change shows up.
    fn entry_line(entry: &Entry) -> String {
        format!(
            "{} {:?} {} {}={}",
            entry.ecosystem.name(),
            entry.kind,
            entry.site.key,
            entry.name,
            entry.site.constraint
        )
    }

    fn refusal_line(refusal: &Refusal) -> String {
        format!("{} {}", refusal.reason, refusal.name)
    }

    fn read(path: &str) -> parser::Parsed {
        let kind = manifest_kind(path).unwrap_or_else(|| panic!("{path} is not a manifest"));
        parser::parse(kind, path, document(path))
    }

    #[test]
    fn every_corpus_path_classifies_the_same() {
        let cases = corpus().classification;
        assert!(!cases.is_empty(), "the corpus classifies nothing");
        for case in cases {
            let actual = manifest_kind(&case.path).map(|kind| kind.ecosystem().name().to_string());
            assert_eq!(actual, case.ecosystem, "{}", case.path);
        }
    }

    #[test]
    fn every_corpus_document_extracts_the_same() {
        let cases = corpus().extraction;
        assert!(!cases.is_empty(), "the corpus extracts nothing");
        for case in cases {
            let parsed = read(&case.file);
            let entries: Vec<String> = parsed.entries.iter().map(entry_line).collect();
            let refusals: Vec<String> = parsed.refusals.iter().map(refusal_line).collect();
            assert_eq!(entries, case.entries, "{} entries", case.file);
            assert_eq!(refusals, case.refusals, "{} refusals", case.file);
        }
    }

    #[test]
    fn every_corpus_analysis_reproduces() {
        let cases = corpus().analysis;
        assert!(!cases.is_empty(), "the corpus analyses nothing");
        for case in cases {
            let entries: Vec<Entry> = case
                .files
                .iter()
                .flat_map(|path| read(path).entries)
                .collect();
            let parsed: Vec<Refusal> = case
                .files
                .iter()
                .flat_map(|path| read(path).refusals)
                .collect();
            let (findings, crossings) = analyse(&entries);

            let actual: Vec<String> = findings
                .iter()
                .map(|finding| {
                    format!(
                        "{} {} {} {}",
                        finding.severity,
                        finding.code,
                        finding.ecosystem.name(),
                        finding.name
                    )
                })
                .collect();
            let mut refusals: Vec<String> = parsed
                .iter()
                .chain(crossings.iter())
                .map(refusal_line)
                .collect();
            refusals.sort();

            assert_eq!(actual, case.findings, "{} findings", case.name);
            assert_eq!(refusals, case.refusals, "{} refusals", case.name);
        }
    }

    // -----------------------------------------------------------------
    // The coverage matrix: does the crate open what it claims?
    //
    // The extractor crates in this family answer that with one corpus
    // document per extension in their alias table. This crate's table is
    // its **five ecosystems, five manifest kinds, six findings and three
    // refusal reasons** — every one of them a claim in SPEC.md and the
    // README — so that is the matrix. A manifest kind with no document
    // behind it, or a finding nothing in the corpus can produce, inflates
    // what the tool says it covers.
    //
    // The two directions are checked separately and both matter:
    // *reachable* is "the corpus proves this exists", and *exhaustive* is
    // "the code can emit nothing the corpus does not prove".
    // -----------------------------------------------------------------

    /// The corpus document that proves a manifest kind is read.
    ///
    /// **Exhaustive on purpose.** A sixth reader added without a document
    /// behind it stops compiling here rather than shipping as an
    /// unproven line in the README's "what it reads" table.
    fn document_for_kind(kind: ManifestKind) -> &'static str {
        match kind {
            ManifestKind::PackageJson => "package.json",
            ManifestKind::CargoToml => "api/Cargo.toml",
            ManifestKind::PyprojectToml => "svc/base/pyproject.toml",
            ManifestKind::GoMod => "go.mod",
            ManifestKind::Workflow => ".github/workflows/ci.yml",
        }
    }

    /// The corpus document that proves an ecosystem yields constraints,
    /// exhaustive for the same reason.
    fn document_for_ecosystem(ecosystem: Ecosystem) -> &'static str {
        match ecosystem {
            Ecosystem::Cargo => "api/Cargo.toml",
            Ecosystem::Ci => ".github/workflows/ci.yml",
            Ecosystem::Go => "go.mod",
            Ecosystem::Npm => "package.json",
            Ecosystem::Python => "svc/base/pyproject.toml",
        }
    }

    const KINDS: [ManifestKind; 5] = [
        ManifestKind::PackageJson,
        ManifestKind::CargoToml,
        ManifestKind::PyprojectToml,
        ManifestKind::GoMod,
        ManifestKind::Workflow,
    ];

    const ECOSYSTEMS: [Ecosystem; 5] = [
        Ecosystem::Cargo,
        Ecosystem::Ci,
        Ecosystem::Go,
        Ecosystem::Npm,
        Ecosystem::Python,
    ];

    const FINDINGS: [&str; 6] = [
        "constraint-conflict",
        "disjoint-constraint",
        "floating-pin",
        "malformed-constraint",
        "msrv-mismatch",
        "prerelease-in-production",
    ];

    const REFUSALS: [&str; 4] = [
        "ambiguous_version_string",
        "cross_ecosystem",
        "per_job_tool_version",
        "unknown_grammar",
    ];

    /// Every identifier-shaped string literal a module's **non-test**
    /// source carries, joined by `separator` and otherwise lowercase.
    ///
    /// The test half is cut off first: it names package fixtures like
    /// `left-pad`, which is the same shape as a finding code and is not
    /// one. Every piece between quotes is examined rather than every
    /// other one, because a format string here escapes quotes of its own
    /// and an alternating scan would lose its place after the first.
    fn coded_literals(source: &str, separator: char) -> Vec<String> {
        let declarations = source.split("#[cfg(test)]").next().unwrap_or(source);
        let mut found: Vec<String> = Vec::new();
        for piece in declarations.split('"') {
            let shaped = piece.contains(separator)
                && !piece.starts_with(separator)
                && !piece.ends_with(separator)
                && piece
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == separator);
            if shaped && !found.contains(&piece.to_string()) {
                found.push(piece.to_string());
            }
        }
        found.sort();
        found
    }

    #[test]
    fn coverage_matrix_every_ecosystem_and_manifest_kind_opens_a_real_document() {
        for kind in KINDS {
            let path = document_for_kind(kind);
            assert_eq!(
                manifest_kind(path),
                Some(kind),
                "{path} does not classify as the kind it is the proof for"
            );
            let parsed = parser::parse(kind, path, document(path));
            assert!(
                !parsed.entries.is_empty(),
                "{path} is claimed as the proof for {kind:?} and yields no constraint"
            );
            assert!(parsed.errors.is_empty(), "{path}: {:?}", parsed.errors);
        }

        for ecosystem in ECOSYSTEMS {
            let path = document_for_ecosystem(ecosystem);
            let kind = manifest_kind(path).unwrap_or_else(|| panic!("{path} is not a manifest"));
            assert_eq!(kind.ecosystem(), ecosystem, "{path}");
            assert!(
                parser::parse(kind, path, document(path))
                    .entries
                    .iter()
                    .any(|entry| entry.ecosystem == ecosystem),
                "{path} names no {} constraint",
                ecosystem.name()
            );
        }

        eprintln!(
            "coverage-matrix: {} ecosystems and {} manifest kinds reachable",
            ECOSYSTEMS.len(),
            KINDS.len()
        );
    }

    /// Every finding the crate can emit and every reason it can refuse
    /// for, produced by a real corpus document — and nothing the corpus
    /// cannot produce left in the code.
    ///
    /// The second half is the one that catches drift: a seventh check
    /// added without a corpus case is a claim in the README that no test
    /// stands behind.
    #[test]
    fn coverage_matrix_every_finding_and_refusal_is_produced_by_the_corpus() {
        let mut produced_findings: Vec<String> = Vec::new();
        let mut produced_refusals: Vec<String> = Vec::new();
        for case in corpus().analysis {
            let entries: Vec<Entry> = case
                .files
                .iter()
                .flat_map(|path| read(path).entries)
                .collect();
            let (findings, crossings) = analyse(&entries);
            let reasons = case
                .files
                .iter()
                .flat_map(|path| read(path).refusals)
                .chain(crossings);
            for finding in findings {
                if !produced_findings.contains(&finding.code) {
                    produced_findings.push(finding.code);
                }
            }
            for refusal in reasons {
                if !produced_refusals.contains(&refusal.reason) {
                    produced_refusals.push(refusal.reason);
                }
            }
        }
        produced_findings.sort();
        produced_refusals.sort();

        assert_eq!(produced_findings, FINDINGS, "findings the corpus produces");
        assert_eq!(produced_refusals, REFUSALS, "refusals the corpus produces");

        // And the other direction: nothing in the code that the corpus
        // cannot reach. Finding codes are kebab-cased and live in
        // compare.rs; refusal reasons are snake-cased and are split
        // between the readers and the cross-ecosystem check.
        let emitted = coded_literals(include_str!("compare.rs"), '-');
        assert_eq!(emitted, FINDINGS, "finding codes compare.rs can emit");

        let mut reasons = coded_literals(include_str!("compare.rs"), '_');
        reasons.extend(coded_literals(include_str!("parser.rs"), '_'));
        reasons.sort();
        reasons.dedup();
        assert_eq!(reasons, REFUSALS, "refusal reasons the readers can emit");

        eprintln!(
            "coverage-matrix: {} finding codes and {} refusal reasons reachable",
            FINDINGS.len(),
            REFUSALS.len()
        );
    }

    /// **The refusal spine, over the corpus rather than a unit test.**
    /// Every `grammar-*` pairing must produce a refusal and no conflict:
    /// a tool that started guessing at these grammars would turn each
    /// refusal into a fabricated finding.
    #[test]
    fn no_unmodelled_grammar_pairing_produces_a_conflict() {
        for case in corpus()
            .analysis
            .iter()
            .filter(|case| case.name.starts_with("refuses"))
        {
            assert!(
                !case.refusals.is_empty(),
                "{} pins no refusal, so it proves nothing",
                case.name
            );
            for finding in &case.findings {
                assert!(
                    !finding.contains("conflict"),
                    "{} manufactured {finding}",
                    case.name
                );
            }
        }
    }
}
