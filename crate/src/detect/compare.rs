//! The checks: entries in, findings out.
//!
//! **Comparison is scoped per ecosystem, always.** An npm `semver` and a
//! Cargo `semver` are unrelated packages that share a word, and a bare
//! `"1.0.200"` means "exactly 1.0.200" in one grammar and "anything
//! below 2" in the other. A name that turns up in two ecosystems is a
//! `cross_ecosystem` refusal — the tool saying it saw the coincidence
//! and declined to read anything into it.
//!
//! The one deliberate exception is `msrv-mismatch`, and it is not a name
//! match: it relates two *named* keys — Cargo's `rust-version` and a
//! workflow's Rust toolchain pin — because those two do describe the
//! same number. Every other check stays inside one ecosystem.
//!
//! An entry whose constraint is `Unknown` never reaches the comparison.
//! Guessing at a grammar the tool does not model would manufacture a
//! conflict out of syntax, which is the one output this crate cannot
//! afford.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::grammar;
use super::heuristics::Ecosystem;
use super::parser::{Entry, Kind, Refusal, Site};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Finding {
    pub(crate) code: String,
    pub(crate) severity: String,
    pub(crate) ecosystem: Ecosystem,
    pub(crate) name: String,
    pub(crate) message: String,
    pub(crate) sites: Vec<Site>,
}

const ERROR: &str = "error";
const WARNING: &str = "warning";
const INFO: &str = "info";

/// Every check, over every entry. The refusals returned here are the two
/// the comparison itself declines to make — `cross_ecosystem` and
/// `per_job_tool_version`; the rest come from the readers.
pub(crate) fn analyse(entries: &[Entry]) -> (Vec<Finding>, Vec<Refusal>) {
    let mut findings = Vec::new();
    findings.extend(conflicts(entries));
    findings.extend(msrv(entries));
    findings.extend(prereleases(entries));
    findings.extend(malformed(entries));
    findings.extend(floating(entries));

    findings.sort_by(|left, right| {
        rank(&left.severity)
            .cmp(&rank(&right.severity))
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.ecosystem.cmp(&right.ecosystem))
            .then_with(|| left.name.cmp(&right.name))
    });

    let mut refusals = cross_ecosystem(entries);
    refusals.extend(per_job_tool_versions(entries));
    (findings, refusals)
}

fn rank(severity: &str) -> u8 {
    match severity {
        ERROR => 0,
        WARNING => 1,
        _ => 2,
    }
}

/// Group by ecosystem **and** name. The pair is the key everywhere in
/// this module, and it is the whole of the scoping rule.
fn group_by_name(
    entries: &[Entry],
    keep: impl Fn(&Entry) -> bool,
) -> BTreeMap<(Ecosystem, &str), Vec<&Entry>> {
    let mut groups: BTreeMap<(Ecosystem, &str), Vec<&Entry>> = BTreeMap::new();
    for entry in entries.iter().filter(|entry| keep(entry)) {
        groups
            .entry((entry.ecosystem, entry.name.as_str()))
            .or_default()
            .push(entry);
    }
    groups
}

fn sites(entries: &[&Entry]) -> Vec<Site> {
    let mut sites: Vec<Site> = entries.iter().map(|entry| entry.site.clone()).collect();
    sites.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then_with(|| left.key.cmp(&right.key))
    });
    sites.dedup();
    sites
}

fn distinct_constraints(entries: &[&Entry]) -> Vec<String> {
    let mut seen: Vec<String> = entries
        .iter()
        .map(|entry| entry.site.constraint.clone())
        .collect();
    seen.sort();
    seen.dedup();
    seen
}

/// How many genuinely different requirements the group holds.
///
/// Compared as modelled ranges, so two spellings of one requirement
/// count once. `Range` has no ordering, so this is quadratic in the size
/// of a single dependency's site list — which is a handful even in a
/// large monorepo.
fn distinct_ranges(entries: &[&Entry]) -> usize {
    let mut seen: Vec<&grammar::Range> = Vec::new();
    for range in entries.iter().filter_map(|entry| entry.constraint.range()) {
        if !seen.contains(&range) {
            seen.push(range);
        }
    }
    seen.len()
}

fn distinct_files(entries: &[&Entry]) -> usize {
    let mut files: Vec<&str> = entries
        .iter()
        .map(|entry| entry.site.file.as_str())
        .collect();
    files.sort_unstable();
    files.dedup();
    files.len()
}

// ---------------------------------------------------------------------
// constraint-conflict / disjoint-constraint
// ---------------------------------------------------------------------

/// One finding per drifted dependency, escalating to
/// `disjoint-constraint` when some pair of its constraints is provably
/// unsatisfiable together.
///
/// One finding rather than one per pair, because a dependency
/// constrained four ways is one problem with four sites, and four
/// findings would read as four problems. The escalation is the
/// distinction that matters: *different* is a smell, *disjoint* is a
/// build that cannot resolve.
///
/// **The trigger is two distinct modelled ranges, not two distinct
/// strings.** `>=20` and `>=20.0.0` are the same requirement typed twice
/// and nothing can go wrong between them; reporting a spelling
/// difference as a conflict would spend the reader's attention on
/// something that is not a defect.
fn conflicts(entries: &[Entry]) -> Vec<Finding> {
    let comparable = |entry: &Entry| {
        entry.kind != Kind::Msrv && !per_job(entry) && entry.constraint.range().is_some()
    };

    group_by_name(entries, comparable)
        .into_iter()
        .filter_map(|((ecosystem, name), group)| {
            if distinct_ranges(&group) < 2 {
                return None;
            }
            let constraints = distinct_constraints(&group);
            let clash = first_disjoint_pair(&group);
            let (code, severity, message) = match clash {
                Some((left, right)) => (
                    "disjoint-constraint",
                    ERROR,
                    format!(
                        "\"{}\" ({}) and \"{}\" ({}) cannot both be satisfied by one version",
                        left.site.constraint,
                        left.site.file,
                        right.site.constraint,
                        right.site.file
                    ),
                ),
                None => (
                    "constraint-conflict",
                    WARNING,
                    format!(
                        "constrained {} different ways across {} files: {}",
                        constraints.len(),
                        distinct_files(&group),
                        constraints.join(", ")
                    ),
                ),
            };
            Some(Finding {
                code: code.to_string(),
                severity: severity.to_string(),
                ecosystem,
                name: name.to_string(),
                message,
                sites: sites(&group),
            })
        })
        .collect()
}

/// Whether this constraint belongs to a **job** rather than to the
/// repository.
///
/// A `<tool>-version:` input is what one CI job installs before it runs.
/// Testing on the oldest supported interpreter and publishing on the
/// newest is correct and common, and so is a job that needs an old
/// toolchain for one build step — two jobs are not two claims about a
/// single requirement, they are two jobs.
///
/// Everything else keeps its repository-wide reading. An action `uses:`
/// pin is which version of a dependency the repository has chosen, and
/// two workflows disagreeing about that is exactly the drift this crate
/// exists for; `packageManager` is Corepack's one-per-repository pin and
/// is `Kind::Tool` in npm, not here.
fn per_job(entry: &Entry) -> bool {
    entry.ecosystem == Ecosystem::Ci && entry.kind == Kind::Tool
}

/// The refusal that replaces the comparison `per_job` withholds.
///
/// It fires exactly where a conflict used to, so nothing goes quiet: the
/// reader still sees that one tool is installed two ways and still sees
/// every site. What changes is the verdict — the tool cannot know
/// whether two jobs were meant to agree, and inventing an answer to that
/// was an `error` and a failed build against a repository that had done
/// nothing wrong.
fn per_job_tool_versions(entries: &[Entry]) -> Vec<Refusal> {
    group_by_name(entries, |entry| {
        per_job(entry) && entry.constraint.range().is_some()
    })
    .into_iter()
    .filter(|(_, group)| distinct_ranges(group) > 1)
    .map(|((ecosystem, name), group)| Refusal {
        reason: "per_job_tool_version".to_string(),
        ecosystem: Some(ecosystem),
        name: name.to_string(),
        message: format!(
            "installed as {} by different jobs; a CI tool version belongs to the job that installs it, so these were not compared",
            distinct_constraints(&group).join(" and ")
        ),
        sites: sites(&group),
    })
    .collect()
}

fn first_disjoint_pair<'a>(group: &[&'a Entry]) -> Option<(&'a Entry, &'a Entry)> {
    for (index, left) in group.iter().enumerate() {
        for right in &group[index + 1..] {
            let (Some(one), Some(other)) = (left.constraint.range(), right.constraint.range())
            else {
                continue;
            };
            if grammar::disjoint(one, other) {
                return Some((left, right));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------
// msrv-mismatch
// ---------------------------------------------------------------------

/// Two questions, one code: do the workspace's own `rust-version`
/// declarations agree, and does CI actually build on a toolchain that
/// old?
///
/// This is the one check that reads across ecosystems, and it does so by
/// naming both keys rather than by matching a package name. A CI job on
/// 1.85 is not evidence about a crate called "rust"; it is evidence
/// about the toolchain the declared MSRV claims to support.
fn msrv(entries: &[Entry]) -> Vec<Finding> {
    let declared: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.kind == Kind::Msrv && entry.ecosystem == Ecosystem::Cargo)
        .collect();

    let mut findings = Vec::new();
    if distinct_ranges(&declared) > 1 {
        let constraints = distinct_constraints(&declared);
        findings.push(Finding {
            code: "msrv-mismatch".to_string(),
            severity: WARNING.to_string(),
            ecosystem: Ecosystem::Cargo,
            name: "rust".to_string(),
            message: format!(
                "rust-version is declared {} different ways: {}",
                constraints.len(),
                constraints.join(", ")
            ),
            sites: sites(&declared),
        });
    }

    let Some(highest) = declared
        .iter()
        .filter_map(|entry| Some((entry.constraint.range()?.floor()?, *entry)))
        .max_by(|left, right| left.0.cmp(right.0))
    else {
        return findings;
    };

    for pin in entries
        .iter()
        .filter(|entry| entry.kind == Kind::Msrv && entry.ecosystem == Ecosystem::Ci)
    {
        let Some(floor) = pin.constraint.range().and_then(grammar::Range::floor) else {
            continue;
        };
        if floor >= highest.0 {
            continue;
        }
        findings.push(Finding {
            code: "msrv-mismatch".to_string(),
            severity: WARNING.to_string(),
            ecosystem: Ecosystem::Ci,
            name: "rust".to_string(),
            message: format!(
                "CI builds on {floor}, below the declared minimum {} in {}",
                highest.0, highest.1.site.file
            ),
            sites: sites(&[pin, highest.1]),
        });
    }
    findings
}

// ---------------------------------------------------------------------
// prerelease-in-production / malformed-constraint / floating-pin
// ---------------------------------------------------------------------

/// A release candidate a test harness pulls in is a choice. One a
/// shipped dependency pulls in is a finding.
fn prereleases(entries: &[Entry]) -> Vec<Finding> {
    let shipped = |entry: &Entry| {
        !entry.kind.is_development()
            && entry
                .constraint
                .range()
                .is_some_and(grammar::Range::mentions_prerelease)
    };
    group_by_name(entries, shipped)
        .into_iter()
        .map(|((ecosystem, name), group)| Finding {
            code: "prerelease-in-production".to_string(),
            severity: WARNING.to_string(),
            ecosystem,
            name: name.to_string(),
            message: format!(
                "a prerelease constraint outside dev dependencies: {}",
                distinct_constraints(&group).join(", ")
            ),
            sites: sites(&group),
        })
        .collect()
}

fn malformed(entries: &[Entry]) -> Vec<Finding> {
    let broken = |entry: &Entry| entry.constraint.malformed_reason().is_some();
    group_by_name(entries, broken)
        .into_iter()
        // The reason is read back out of the group rather than assumed:
        // a group with none is no finding, not a finding with a
        // substituted message nobody wrote.
        .filter_map(|((ecosystem, name), group)| {
            let reason = group
                .iter()
                .find_map(|entry| entry.constraint.malformed_reason())?;
            Some(Finding {
                code: "malformed-constraint".to_string(),
                severity: ERROR.to_string(),
                ecosystem,
                name: name.to_string(),
                message: format!("{reason}: {}", distinct_constraints(&group).join(", ")),
                sites: sites(&group),
            })
        })
        .collect()
}

/// Informational by default: a floating pin is a choice with a cost, not
/// a defect. It is here so the cost is visible, and `--fail-on any` is
/// how a repository that has decided otherwise says so.
fn floating(entries: &[Entry]) -> Vec<Finding> {
    group_by_name(entries, |entry| entry.floats.is_some())
        .into_iter()
        // As in `malformed`: the reason comes back out of the group, so
        // no finding can carry a message the readers did not write.
        .filter_map(|((ecosystem, name), group)| {
            let why = group.iter().find_map(|entry| entry.floats)?;
            Some(Finding {
                code: "floating-pin".to_string(),
                severity: INFO.to_string(),
                ecosystem,
                name: name.to_string(),
                message: why.to_string(),
                sites: sites(&group),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------
// cross_ecosystem
// ---------------------------------------------------------------------

/// A name in two ecosystems, named and left alone.
///
/// The refusal exists so the silence is legible: without it, a reader
/// who knows both manifests mention `semver` cannot tell whether the
/// tool compared them and found nothing or never compared them at all.
fn cross_ecosystem(entries: &[Entry]) -> Vec<Refusal> {
    let mut by_name: BTreeMap<&str, BTreeMap<Ecosystem, &Entry>> = BTreeMap::new();
    // Minimum-toolchain declarations are the one pair that *is* bridged,
    // by `msrv-mismatch`. Refusing to compare them here as well would
    // have the report contradict itself.
    for entry in entries.iter().filter(|entry| entry.kind != Kind::Msrv) {
        by_name
            .entry(entry.name.as_str())
            .or_default()
            .entry(entry.ecosystem)
            .or_insert(entry);
    }

    by_name
        .into_iter()
        .filter(|(_, seen)| seen.len() > 1)
        .map(|(name, seen)| {
            let names: Vec<&str> = seen.keys().map(|ecosystem| ecosystem.name()).collect();
            Refusal {
                reason: "cross_ecosystem".to_string(),
                ecosystem: None,
                name: name.to_string(),
                message: format!(
                    "appears in {}; different ecosystems name different things, so these were not compared",
                    names.join(" and ")
                ),
                sites: sites(&seen.into_values().collect::<Vec<_>>()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::heuristics::ManifestKind;
    use crate::detect::parser::{self, ParseError};

    fn entries(files: &[(ManifestKind, &str, &str)]) -> (Vec<Entry>, Vec<ParseError>) {
        let mut all = Vec::new();
        let mut errors = Vec::new();
        for (kind, file, content) in files {
            let parsed = parser::parse(*kind, file, content);
            all.extend(parsed.entries);
            errors.extend(parsed.errors);
        }
        (all, errors)
    }

    fn findings(files: &[(ManifestKind, &str, &str)]) -> Vec<Finding> {
        let (entries, errors) = entries(files);
        assert!(errors.is_empty(), "{errors:?}");
        analyse(&entries).0
    }

    fn coded<'a>(findings: &'a [Finding], code: &str) -> Vec<&'a Finding> {
        findings.iter().filter(|f| f.code == code).collect()
    }

    const CARGO_A: (ManifestKind, &str, &str) = (
        ManifestKind::CargoToml,
        "a/Cargo.toml",
        "[dependencies]\nserde = \"1.0.200\"\n",
    );

    #[test]
    fn the_same_dependency_constrained_two_ways_is_a_conflict() {
        let found = findings(&[
            CARGO_A,
            (
                ManifestKind::CargoToml,
                "b/Cargo.toml",
                "[dependencies]\nserde = \"1\"\n",
            ),
        ]);
        let conflicts = coded(&found, "constraint-conflict");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].name, "serde");
        assert_eq!(conflicts[0].severity, WARNING);
        assert_eq!(conflicts[0].sites.len(), 2);
    }

    /// The strongest finding, and the one that must stay distinguishable
    /// from a merely different constraint.
    #[test]
    fn two_constraints_no_version_satisfies_are_disjoint_not_merely_different() {
        let found = findings(&[
            CARGO_A,
            (
                ManifestKind::CargoToml,
                "b/Cargo.toml",
                "[dependencies]\nserde = \"2.0\"\n",
            ),
        ]);
        assert!(coded(&found, "constraint-conflict").is_empty());
        let disjoint = coded(&found, "disjoint-constraint");
        assert_eq!(disjoint.len(), 1);
        assert_eq!(disjoint[0].severity, ERROR);
        assert!(disjoint[0].message.contains("cannot both be satisfied"));
    }

    #[test]
    fn the_same_constraint_in_two_files_is_not_a_finding() {
        let found = findings(&[
            CARGO_A,
            (
                ManifestKind::CargoToml,
                "b/Cargo.toml",
                "[dependencies]\nserde = \"1.0.200\"\n",
            ),
        ]);
        assert!(coded(&found, "constraint-conflict").is_empty());
        assert!(coded(&found, "disjoint-constraint").is_empty());
    }

    /// Regression: `>=20` and `>=20.0.0` are one requirement typed
    /// twice, and reporting the spelling as a conflict spends the
    /// reader's attention on something that cannot go wrong.
    #[test]
    fn two_spellings_of_one_requirement_are_not_a_conflict() {
        let found = findings(&[
            (
                ManifestKind::PackageJson,
                "package.json",
                r#"{ "engines": { "node": ">=20.0.0" } }"#,
            ),
            (
                ManifestKind::PackageJson,
                "mcp/package.json",
                r#"{ "engines": { "node": ">=20" } }"#,
            ),
        ]);
        assert!(found.is_empty(), "{found:?}");
    }

    /// **The scoping rule.** Same word, two ecosystems, no comparison.
    #[test]
    fn a_name_in_two_ecosystems_is_never_compared() {
        let (entries, _) = entries(&[
            (
                ManifestKind::CargoToml,
                "Cargo.toml",
                "[dependencies]\nsemver = \"1.0.28\"\n",
            ),
            (
                ManifestKind::PackageJson,
                "package.json",
                r#"{ "dependencies": { "semver": "^7.6.0" } }"#,
            ),
        ]);
        let (found, refusals) = analyse(&entries);
        assert!(coded(&found, "constraint-conflict").is_empty());
        assert!(coded(&found, "disjoint-constraint").is_empty());
        assert_eq!(refusals.len(), 1);
        assert_eq!(refusals[0].reason, "cross_ecosystem");
        assert_eq!(refusals[0].name, "semver");
        assert!(refusals[0].ecosystem.is_none());
    }

    /// **The refusal spine.** A grammar the tool does not model never
    /// becomes a conflict, however different it looks.
    #[test]
    fn an_unknown_grammar_never_manufactures_a_conflict() {
        let found = findings(&[
            CARGO_A,
            (
                ManifestKind::CargoToml,
                "b/Cargo.toml",
                "[dependencies]\nserde = { workspace = true }\n",
            ),
        ]);
        assert!(coded(&found, "constraint-conflict").is_empty());
        assert!(coded(&found, "disjoint-constraint").is_empty());
    }

    /// **A CI tool version belongs to the job that installs it.**
    /// Testing on the oldest supported interpreter and publishing on the
    /// newest is correct, common, and not a contradiction — but it is
    /// two versions of one name, which is the shape a conflict is made
    /// of. The tool cannot know whether two jobs were meant to agree, so
    /// it says what it saw and declines to compare.
    #[test]
    fn two_jobs_pinning_one_tool_differently_is_refused_not_a_conflict() {
        let (entries, _) = entries(&[
            (
                ManifestKind::Workflow,
                ".github/workflows/ci.yml",
                "      - uses: actions/setup-python@v5\n        with:\n          python-version: 3.9\n",
            ),
            (
                ManifestKind::Workflow,
                ".github/workflows/publish.yml",
                "      - uses: actions/setup-python@v5\n        with:\n          python-version: 3.12\n",
            ),
        ]);
        let (found, refusals) = analyse(&entries);
        assert!(coded(&found, "disjoint-constraint").is_empty(), "{found:?}");
        assert!(coded(&found, "constraint-conflict").is_empty(), "{found:?}");

        let refused: Vec<&Refusal> = refusals
            .iter()
            .filter(|refusal| refusal.reason == "per_job_tool_version")
            .collect();
        assert_eq!(refused.len(), 1, "{refusals:?}");
        assert_eq!(refused[0].name, "python");
        assert_eq!(refused[0].sites.len(), 2);
        assert!(refused[0].message.contains("3.9"), "{}", refused[0].message);
    }

    /// The action pin beside it is **not** per job. Which version of an
    /// action the repository uses is the repository's decision, and two
    /// workflows disagreeing about it is the drift this crate is for.
    #[test]
    fn an_action_pinned_two_ways_is_still_a_conflict() {
        let found = findings(&[
            (
                ManifestKind::Workflow,
                ".github/workflows/ci.yml",
                "      - uses: actions/checkout@v4\n",
            ),
            (
                ManifestKind::Workflow,
                ".github/workflows/publish.yml",
                "      - uses: actions/checkout@v5\n",
            ),
        ]);
        assert_eq!(coded(&found, "disjoint-constraint").len(), 1, "{found:?}");
    }

    /// Corepack's `packageManager` is one per repository, so it keeps
    /// being compared: the exemption is for a CI job's toolchain, not
    /// for everything the report calls a tool.
    #[test]
    fn a_package_manager_pinned_two_ways_is_still_a_conflict() {
        let found = findings(&[
            (
                ManifestKind::PackageJson,
                "package.json",
                r#"{ "packageManager": "bun@1.1.0" }"#,
            ),
            (
                ManifestKind::PackageJson,
                "app/package.json",
                r#"{ "packageManager": "bun@1.2.0" }"#,
            ),
        ]);
        assert_eq!(coded(&found, "disjoint-constraint").len(), 1, "{found:?}");
    }

    #[test]
    fn a_floating_pin_is_informational() {
        let found = findings(&[(
            ManifestKind::PackageJson,
            "package.json",
            r#"{ "dependencies": { "left-pad": "latest" } }"#,
        )]);
        let floating = coded(&found, "floating-pin");
        assert_eq!(floating.len(), 1);
        assert_eq!(floating[0].severity, INFO);
    }

    #[test]
    fn a_declared_msrv_that_differs_across_manifests_is_a_mismatch() {
        let found = findings(&[
            (
                ManifestKind::CargoToml,
                "a/Cargo.toml",
                "[package]\nrust-version = \"1.88\"\n",
            ),
            (
                ManifestKind::CargoToml,
                "b/Cargo.toml",
                "[package]\nrust-version = \"1.82\"\n",
            ),
        ]);
        let msrv = coded(&found, "msrv-mismatch");
        assert_eq!(msrv.len(), 1);
        assert!(msrv[0].message.contains("1.88"));
    }

    #[test]
    fn a_ci_toolchain_below_the_declared_msrv_is_a_mismatch() {
        let found = findings(&[
            (
                ManifestKind::CargoToml,
                "Cargo.toml",
                "[package]\nrust-version = \"1.88\"\n",
            ),
            (
                ManifestKind::Workflow,
                ".github/workflows/ci.yml",
                "      - uses: dtolnay/rust-toolchain@1.85\n",
            ),
        ]);
        let msrv = coded(&found, "msrv-mismatch");
        assert_eq!(msrv.len(), 1);
        assert_eq!(msrv[0].ecosystem, Ecosystem::Ci);
        assert!(msrv[0].message.contains("1.85"), "{}", msrv[0].message);
    }

    #[test]
    fn a_ci_toolchain_at_or_above_the_declared_msrv_is_silent() {
        let found = findings(&[
            (
                ManifestKind::CargoToml,
                "Cargo.toml",
                "[package]\nrust-version = \"1.88\"\n",
            ),
            (
                ManifestKind::Workflow,
                ".github/workflows/ci.yml",
                "      - uses: dtolnay/rust-toolchain@1.88\n",
            ),
        ]);
        assert!(coded(&found, "msrv-mismatch").is_empty());
    }

    #[test]
    fn a_prerelease_is_a_finding_in_production_and_not_in_dev() {
        let found = findings(&[(
            ManifestKind::PackageJson,
            "package.json",
            r#"{
              "dependencies": { "shipped": "^2.0.0-rc.1" },
              "devDependencies": { "harness": "^3.0.0-beta.2" }
            }"#,
        )]);
        let pre = coded(&found, "prerelease-in-production");
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0].name, "shipped");
    }

    #[test]
    fn a_constraint_that_does_not_parse_is_a_finding_against_the_manifest() {
        let found = findings(&[(
            ManifestKind::PackageJson,
            "package.json",
            r#"{ "dependencies": { "broken": "^^1.0.0" } }"#,
        )]);
        let malformed = coded(&found, "malformed-constraint");
        assert_eq!(malformed.len(), 1);
        assert_eq!(malformed[0].severity, ERROR);
    }

    #[test]
    fn findings_come_back_worst_first_and_in_a_stable_order() {
        let found = findings(&[
            (
                ManifestKind::PackageJson,
                "package.json",
                r#"{ "dependencies": { "a": "1.0.0", "b": "latest", "c": "^^1" } }"#,
            ),
            (
                ManifestKind::PackageJson,
                "app/package.json",
                r#"{ "dependencies": { "a": "2.0.0" } }"#,
            ),
        ]);
        let codes: Vec<&str> = found.iter().map(|f| f.code.as_str()).collect();
        assert_eq!(
            codes,
            [
                "disjoint-constraint",
                "malformed-constraint",
                "floating-pin"
            ]
        );
    }

    #[test]
    fn nothing_to_compare_is_no_findings() {
        assert!(analyse(&[]).0.is_empty());
        assert!(analyse(&[]).1.is_empty());
    }
}
