//! The terminal surface.
//!
//! stdout is protocol — **one JSON report for the whole run**, not one
//! line per manifest, because the answer is about the set of them.
//! stderr is for the human and is a projection of the same report.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::detect::heuristics::Ecosystem;
use crate::scan::{self, FailOn, Report, ScanOptions};

const USAGE: &str = "usage: versions-le [options] <dir|file>...
       versions-le mcp
       versions-le --version | --help

Finds the version constraints in a repository's manifests — package.json,
Cargo.toml, pyproject.toml, go.mod and .github/workflows — and reports
where the same dependency is constrained inconsistently.

Options:
  --ecosystem <name>   only npm, cargo, python, go or ci; repeatable
  --fail-on <what>     conflict (default: exit 1 on anything above info)
                       or any (exit 1 on any finding, floating pins too)
  --strict             a refusal or an unreadable manifest exits 2
  --exclude <glob>     skip manifests matching this pattern; repeatable
  --hidden             descend hidden directories too
  --no-ignore          walk files that .gitignore excludes

Comparison never crosses an ecosystem: an npm `semver` and a Cargo
`semver` are unrelated packages that share a word. A constraint in a
grammar this tool does not model — PEP 440's ~=, a git or workspace
dependency, a commit-SHA action pin — is named in the report's refusals
and left out of the comparison, never guessed at.

Exit codes: 0 clean · 1 findings · 2 malformed question. Finding no
manifests at all is 0 — there is nothing to be in conflict with.";

/// Every flag the parser accepts. Held equal to the flags named in USAGE
/// by a test, and consulted at runtime so the list is what the parser
/// actually honours.
const FLAGS: [&str; 6] = [
    "--ecosystem",
    "--fail-on",
    "--strict",
    "--exclude",
    "--hidden",
    "--no-ignore",
];

#[derive(Debug)]
struct Cli {
    roots: Vec<PathBuf>,
    scan: ScanOptions,
    fail_on: FailOn,
    /// Exit 2 when part of the tree went unanalysed.
    strict: bool,
}

pub(crate) fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(first) = args.first() {
        match first.as_str() {
            "mcp" => return crate::mcp::serve(),
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--version" | "-V" => {
                println!("versions-le {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ => {}
        }
    }

    match execute(&args) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("versions-le: {message}");
            ExitCode::from(2)
        }
    }
}

fn execute(args: &[String]) -> Result<u8, String> {
    let options = parse(args)?;
    let report = scan::scan(&options.roots, &options.scan)?;

    let line = serde_json::to_string(&report).expect("a report serializes");
    writeln!(std::io::stdout().lock(), "{line}")
        .map_err(|error| format!("could not write the report: {error}"))?;

    summarise(&report);
    Ok(scan::exit_code(&report, options.fail_on, options.strict))
}

fn parse(args: &[String]) -> Result<Cli, String> {
    let mut options = Cli {
        roots: Vec::new(),
        scan: ScanOptions::default(),
        fail_on: FailOn::default(),
        strict: false,
    };

    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        // Strict parsing, never a silent default: a typo'd `--stict`
        // that quietly did nothing would report a clean audit that never
        // ran the check it was asked for.
        if arg.starts_with('-') && !FLAGS.contains(&arg.as_str()) {
            return Err(format!("{arg} is not an option. Try --help."));
        }

        match arg.as_str() {
            "--strict" => options.strict = true,
            "--hidden" => options.scan.discover.hidden = true,
            "--no-ignore" => options.scan.discover.respect_ignore = false,
            "--ecosystem" => {
                let value = rest
                    .next()
                    .ok_or_else(|| "--ecosystem needs a name".to_string())?;
                let ecosystem = Ecosystem::parse(value).ok_or_else(|| {
                    format!("{value} is not an ecosystem. Try npm, cargo, python, go or ci.")
                })?;
                options.scan.discover.ecosystems.push(ecosystem);
            }
            "--fail-on" => {
                let value = rest
                    .next()
                    .ok_or_else(|| "--fail-on needs conflict or any".to_string())?;
                options.fail_on = match value.as_str() {
                    "conflict" => FailOn::Conflict,
                    "any" => FailOn::Any,
                    other => return Err(format!("{other} is not conflict or any")),
                };
            }
            "--exclude" => {
                let value = rest
                    .next()
                    .ok_or_else(|| "--exclude needs a pattern".to_string())?;
                options.scan.discover.exclude.push(value.clone());
            }
            path => options.roots.push(PathBuf::from(path)),
        }
    }

    if options.roots.is_empty() {
        options.roots.push(PathBuf::from("."));
    }
    Ok(options)
}

/// The human half. Every line restates something already on stdout.
fn summarise(report: &Report) {
    let mut stderr = std::io::stderr().lock();

    for diagnostic in &report.diagnostics {
        let _ = writeln!(stderr, "{}: {}", diagnostic.file, diagnostic.message);
    }
    for finding in &report.findings {
        let _ = writeln!(
            stderr,
            "{} {}: {} ({}) [{}]",
            finding.severity,
            finding.ecosystem.name(),
            finding.name,
            finding.message,
            files(&finding.sites)
        );
    }
    // Refusals are printed too. A tool that quietly analysed less than
    // it was pointed at, and said so only in a JSON field, would be
    // read as having found nothing.
    for refusal in &report.refusals {
        let _ = writeln!(
            stderr,
            "refused {} {}: {} [{}]",
            refusal.reason,
            refusal.name,
            refusal.message,
            files(&refusal.sites)
        );
    }

    let summary = &report.summary;
    let _ = if summary.manifests == 0 {
        writeln!(stderr, "no manifests found")
    } else if summary.findings == 0 {
        writeln!(
            stderr,
            "consistent — {} across {}",
            plural(summary.entries, "constraint", "constraints"),
            plural(summary.manifests, "manifest", "manifests")
        )
    } else {
        writeln!(
            stderr,
            "{} across {} — {} error, {} warning, {} info",
            plural(summary.findings, "finding", "findings"),
            plural(summary.manifests, "manifest", "manifests"),
            summary.errors,
            summary.warnings,
            summary.infos
        )
    };
}

/// The files a finding touches, each named once. The JSON keeps every
/// site; a human reading nine identical filenames learns nothing from
/// the eight repeats.
fn files(sites: &[crate::detect::parser::Site]) -> String {
    let mut named: Vec<&str> = Vec::new();
    for site in sites {
        if !named.contains(&site.file.as_str()) {
            named.push(&site.file);
        }
    }
    named.join(", ")
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_documented_flag_is_parsed_and_the_reverse() {
        let mut documented: Vec<&str> = USAGE
            .split_whitespace()
            .filter(|word| word.starts_with("--"))
            .map(|word| word.trim_end_matches([',', '.', ':', ';']))
            .filter(|word| !matches!(*word, "--version" | "--help"))
            .collect();
        documented.sort_unstable();
        documented.dedup();

        let mut implemented = FLAGS.to_vec();
        implemented.sort_unstable();
        assert_eq!(documented, implemented);
    }

    #[test]
    fn the_parser_accepts_every_flag_it_lists() {
        for flag in FLAGS {
            let args: Vec<String> = match flag {
                "--ecosystem" => vec![flag.into(), "npm".into()],
                "--fail-on" => vec![flag.into(), "any".into()],
                "--exclude" => vec![flag.into(), "x".into()],
                _ => vec![flag.into()],
            };
            assert!(parse(&args).is_ok(), "{flag}");
        }
    }

    #[test]
    fn an_unknown_flag_is_refused_rather_than_ignored() {
        let error = parse(&["--stict".into()]).expect_err("a refusal");
        assert!(error.contains("--stict"), "{error}");
    }

    #[test]
    fn a_flag_with_no_value_is_refused() {
        for flag in ["--ecosystem", "--fail-on", "--exclude"] {
            assert!(parse(&[flag.into()]).is_err(), "{flag}");
        }
    }

    /// A misspelled ecosystem must not silently widen the walk to every
    /// ecosystem — that would report a clean audit of a scope nobody
    /// asked for.
    #[test]
    fn an_unknown_ecosystem_is_refused_and_names_the_alternatives() {
        let error = parse(&["--ecosystem".into(), "maven".into()]).expect_err("a refusal");
        assert!(error.contains("maven"), "{error}");
        assert!(error.contains("cargo"), "{error}");
    }

    #[test]
    fn an_unknown_fail_on_value_is_refused() {
        assert!(parse(&["--fail-on".into(), "sometimes".into()]).is_err());
    }

    #[test]
    fn the_working_directory_is_the_default_root() {
        assert_eq!(parse(&[]).expect("options").roots, [PathBuf::from(".")]);
    }

    #[test]
    fn several_roots_are_allowed_because_the_question_spans_trees() {
        let options = parse(&["api".into(), "web".into()]).expect("options");
        assert_eq!(options.roots, [PathBuf::from("api"), PathBuf::from("web")]);
    }

    #[test]
    fn the_repeatable_flags_accumulate() {
        let options = parse(&[
            "--ecosystem".into(),
            "npm".into(),
            "--ecosystem".into(),
            "cargo".into(),
            "--exclude".into(),
            "a".into(),
            "--exclude".into(),
            "b".into(),
        ])
        .expect("options");
        assert_eq!(
            options.scan.discover.ecosystems,
            [Ecosystem::Npm, Ecosystem::Cargo]
        );
        assert_eq!(options.scan.discover.exclude, ["a", "b"]);
    }

    /// stdout is protocol; there is one mode and nothing to misremember.
    #[test]
    fn there_is_no_json_flag_and_nothing_that_writes() {
        for attempt in ["--json", "--fix", "--write", "--update", "--pin"] {
            assert!(parse(&[attempt.into()]).is_err(), "{attempt} was accepted");
        }
    }

    #[test]
    fn the_usage_text_states_the_exit_codes_and_the_scoping_rule() {
        for code in ["0", "1", "2"] {
            assert!(USAGE.contains(code), "exit code {code} is undocumented");
        }
        assert!(USAGE.contains("never crosses an ecosystem"));
        assert!(USAGE.contains("refusals"));
    }
}
