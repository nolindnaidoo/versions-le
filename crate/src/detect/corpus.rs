//! The corpus, embedded and run as unit tests.
//!
//! `fixtures/` ships inside the crate so `cargo test` on the published
//! tarball runs every case: whoever installed this can check the
//! refusal claims in the README instead of trusting them.
//!
//! Two halves, and the second is the point. `tree-*` is one synthetic
//! repository carrying a planted instance of every finding this tool
//! makes. `grammar-*` is the opposite: pairs of manifests that a tool
//! comparing strings would call a conflict, pinned here as **refusals**.
//! A regression that started guessing at `workspace = true` would turn a
//! refusal into a finding, and the corpus is what fails.

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
#[cfg_attr(not(test), expect(dead_code, reason = "the corpus is a test fixture"))]
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
        "go.mod" => include_str!("../../fixtures/documents/grammar-go.mod"),

        other => panic!("the corpus has no document at {other}"),
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::document;
    use crate::detect::compare::analyse;
    use crate::detect::heuristics::manifest_kind;
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
