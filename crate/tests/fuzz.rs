//! A standing net over the constraint grammars and the readers.
//!
//! Time-boxed, not run to convergence: the point is that a panic, a
//! hang, or a slice off a character boundary has somewhere to be caught,
//! not that the input space is proved. Sixty seconds in CI, a second
//! locally.
//!
//! **It never touches a filesystem.** Every case goes through
//! `compare_versions`, the MCP tool that takes manifest contents
//! directly, so the target is `detect/` and nothing else.
//!
//! Two properties, and the second is the one this crate cannot afford to
//! lose:
//!
//! - **Always a well-formed report.** One JSON object, `schema: 1`, the
//!   summary counting the lists beside it, and every site naming a file
//!   the caller actually supplied.
//! - **Never a conflict fabricated out of a grammar it does not model.**
//!   Half the generated pairs put a real constraint beside a
//!   `workspace:` specifier, a PEP 440 `~=`, a git URL or a channel
//!   name. Each of those must come back as a **refusal** — named, and
//!   taking part in no comparison. A regression that started guessing at
//!   one of them turns a refusal into a `disjoint-constraint` against
//!   somebody's real manifest, which is the one output this tool cannot
//!   afford, and this is the net under it.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

const BINARY: &str = env!("CARGO_BIN_EXE_versions-le");

/// Seconds of fuzzing. CI passes 60; a bare `cargo test` runs one, so
/// the net is present on every push without owning the run.
fn budget() -> Duration {
    let seconds = std::env::var("VERSIONS_LE_FUZZ_SECONDS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(1);
    Duration::from_secs(seconds)
}

/// Printed on every run, failing or not: a fuzz failure nobody can
/// reproduce is a fuzz failure nobody fixes.
fn seed() -> u64 {
    std::env::var("VERSIONS_LE_FUZZ_SEED")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(0x7e12_0a5e_0b1d_2026)
}

/// One case may not take longer than this. A grammar that went
/// exponential on four hundred `||` alternatives would otherwise be a CI
/// job that never ends.
const CASE_LIMIT: Duration = Duration::from_secs(20);

/// The six codes a finding can carry, and the four reasons a refusal
/// can. Pinned here as well as in the corpus so a fuzz case that
/// produced a seventh finding or a fifth reason would fail rather than
/// pass unnoticed.
const CODES: [&str; 6] = [
    "constraint-conflict",
    "disjoint-constraint",
    "floating-pin",
    "malformed-constraint",
    "msrv-mismatch",
    "prerelease-in-production",
];
const REASONS: [&str; 4] = [
    "ambiguous_version_string",
    "cross_ecosystem",
    "per_job_tool_version",
    "unknown_grammar",
];
const CONFLICTS: [&str; 2] = ["constraint-conflict", "disjoint-constraint"];

/// xorshift64*, four lines and identical on every platform. A fuzz run
/// needs to be reproducible, not statistically excellent.
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        self.0 = state;
        state.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, limit: usize) -> usize {
        let limit = u64::try_from(limit).unwrap_or(1).max(1);
        usize::try_from(self.next() % limit).unwrap_or(0)
    }

    fn pick<'a, T>(&mut self, from: &'a [T]) -> &'a T {
        &from[self.below(from.len())]
    }
}

/// A server held open across the whole run: spawning a process per case
/// would measure `fork`, not the grammars.
struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Option<String>>,
}

impl Server {
    fn start() -> Self {
        let mut child = Command::new(BINARY)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the server starts");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");

        // Read on a thread so a case that never answers is a timeout
        // naming its documents rather than a blocked test.
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sender.send(line.ok()).is_err() {
                    return;
                }
            }
            let _ = sender.send(None);
        });

        Self {
            child,
            stdin,
            lines,
        }
    }

    fn compare(&mut self, case: &str, files: &[(String, String)]) -> serde_json::Value {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "compare_versions",
                "arguments": {
                    "files": files
                        .iter()
                        .map(|(path, content)| serde_json::json!({
                            "path": path, "content": content
                        }))
                        .collect::<Vec<_>>(),
                },
            },
        });
        writeln!(self.stdin, "{request}")
            .unwrap_or_else(|error| panic!("{case}: the server stopped reading ({error})"));
        self.stdin
            .flush()
            .unwrap_or_else(|error| panic!("{case}: could not flush ({error})"));

        let raw = match self.lines.recv_timeout(CASE_LIMIT) {
            Ok(Some(line)) => line,
            Ok(None) => panic!("{case}: the server died — a panic, or a slice off a boundary"),
            Err(RecvTimeoutError::Timeout) => {
                panic!("{case}: no answer in {CASE_LIMIT:?} — a grammar is not terminating")
            }
            Err(RecvTimeoutError::Disconnected) => panic!("{case}: the server closed stdout"),
        };
        serde_json::from_str(&raw)
            .unwrap_or_else(|error| panic!("{case}: the answer is not JSON ({error}) — {raw}"))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// --------------------------------------------------------------- documents

/// A TOML basic string. The generator writes quotes, backslashes and
/// control characters on purpose, and a document that failed to parse
/// would test the TOML crate rather than the grammars.
fn toml_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('"');
    for character in raw.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04X}", other as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn json_string(raw: &str) -> String {
    serde_json::to_string(raw).expect("a string serializes")
}

/// The five ecosystems, as document builders. Each takes one constraint
/// and returns a manifest whose only content is that constraint, so a
/// finding can be about nothing else.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ecosystem {
    Cargo,
    Npm,
    Python,
    Go,
    Ci,
}

impl Ecosystem {
    fn file(self) -> &'static str {
        match self {
            Self::Cargo => "Cargo.toml",
            Self::Npm => "package.json",
            Self::Python => "pyproject.toml",
            Self::Go => "go.mod",
            Self::Ci => ".github/workflows/ci.yml",
        }
    }

    /// The name both documents constrain, so one finding can name it.
    fn dependency(self) -> &'static str {
        match self {
            Self::Go => "github.com/x/pkg",
            Self::Ci => "node",
            _ => "pkg",
        }
    }

    fn document(self, constraint: &str) -> String {
        match self {
            // A whole dependency value rather than a version string: the
            // Cargo grammars this tool declines to model are table
            // shapes — `workspace = true`, a bare path, a bare git — not
            // version syntax.
            Self::Cargo => format!("[dependencies]\npkg = {constraint}\n"),
            Self::Npm => format!(
                "{{ \"dependencies\": {{ \"pkg\": {} }} }}",
                json_string(constraint)
            ),
            Self::Python => format!(
                "[project]\ndependencies = [{}]\n",
                toml_string(&format!("pkg{constraint}"))
            ),
            Self::Go => format!("module example.com/a\n\nrequire github.com/x/pkg {constraint}\n"),
            Self::Ci => format!(
                "jobs:\n  test:\n    steps:\n      - uses: actions/setup-node@v4\n        \
                 with:\n          node-version: {}\n",
                constraint.replace(['\n', '\r'], " ")
            ),
        }
    }
}

const ECOSYSTEMS: [Ecosystem; 5] = [
    Ecosystem::Cargo,
    Ecosystem::Npm,
    Ecosystem::Python,
    Ecosystem::Go,
    Ecosystem::Ci,
];

/// **The refusal pool.** Every entry is a constraint the tool has
/// declared it does not model, and every case that uses one asserts both
/// halves of the claim: a refusal naming the dependency, and no conflict
/// naming it. `go.mod` has no entry here — Go module versions are exact
/// and there is no unmodelled grammar to refuse.
fn unmodelled(ecosystem: Ecosystem, seeded: &mut Seeded) -> Option<&'static str> {
    let pool: &[&'static str] = match ecosystem {
        Ecosystem::Cargo => &[
            "{ workspace = true }",
            "{ path = \"../pkg\" }",
            "{ git = \"https://example.com/pkg.git\", branch = \"main\" }",
            "{ git = \"https://example.com/pkg.git\", rev = \"deadbeef\" }",
        ],
        Ecosystem::Npm => &[
            "workspace:*",
            "workspace:^1.0.0",
            "npm:other@^1.0.0",
            "file:../local",
            "link:../local",
            "portal:../local",
            "catalog:default",
            "git+https://example.com/pkg.git",
            "git@example.com:owner/pkg.git",
            "github:owner/pkg",
            "owner/pkg",
            "https://example.com/pkg.tgz",
            "./local",
            "latest",
            "next",
        ],
        Ecosystem::Python => &[
            "~=1.4.2",
            "!=1.5.0",
            "===1.0",
            "==1.2.*",
            ">=1.0.post1",
            ">=1.0.dev1",
            ">=1.0+local",
        ],
        Ecosystem::Ci => &["latest", "stable", "nightly", "beta", "current", "lts/*"],
        Ecosystem::Go => &[],
    };
    (!pool.is_empty()).then(|| *seeded.pick(pool))
}

/// Constraints the tool *does* model, per ecosystem. The left-hand side
/// of a refusal pair is drawn from here so the pair is exactly the shape
/// a string-comparing tool would call a conflict.
fn modelled(ecosystem: Ecosystem, seeded: &mut Seeded) -> &'static str {
    let pool: &[&'static str] = match ecosystem {
        Ecosystem::Cargo => &[
            "\"1\"",
            "\"1.0.200\"",
            "\"^1.2.3\"",
            "\"~0.4\"",
            "\"=0.7.7\"",
            "\"0.7.7\"",
            "\">=1.0, <2.0\"",
            "\"*\"",
            "{ version = \"1.0.200\" }",
            "{ path = \"../pkg\", version = \"0.7.7\" }",
            "{ path = \"../pkg\", version = \"=0.7.7\" }",
        ],
        Ecosystem::Npm => &[
            "^1.0.0",
            "1.2.3",
            ">=1 <2",
            "1.x",
            "1.2.3 - 2.0.0",
            "^1.0.0 || ^2.0.0",
            "~2.3.0",
            "*",
            "2.0.0-rc.1",
        ],
        Ecosystem::Python => &[">=2.31", "==1.2", ">=1.0,<2.0", ">3.9", "<=2.0", ""],
        Ecosystem::Go => &["v1.4.0", "v0.9.1", "v2.0.0+incompatible", "v1.0.0-rc.1"],
        Ecosystem::Ci => &["20", "1.1.0", "^18", ">=18 <21", "v4", "22.1.0"],
    };
    seeded.pick(pool)
}

/// Constraint strings nobody writes by hand, which is the point: every
/// one of them reaches the interval arithmetic, and a version string is
/// the last place a slice off a character boundary is noticed.
fn hostile(ecosystem: Ecosystem, seeded: &mut Seeded) -> String {
    let bare = match seeded.below(18) {
        // An enormous prerelease chain. Prereleases are compared segment
        // by segment, and this is where that comparison is asked to do
        // four hundred of them.
        0 => format!("1.0.0-{}", vec!["rc.1"; 1 + seeded.below(400)].join(".")),
        // Alternatives, which is npm's only union grammar — and the
        // shape that turns one constraint into hundreds of intervals,
        // each of which the disjointness test must meet against every
        // interval of the other side.
        1 => (0..=seeded.below(300)).fold(String::new(), |mut out, index| {
            if !out.is_empty() {
                out.push_str(" || ");
            }
            let _ = write!(out, ">={index}.0.0 <{}.0.0", index + 1);
            out
        }),
        // Deeply intersected: every token narrows the same interval.
        2 => (0..=seeded.below(400)).fold(String::new(), |mut out, index| {
            if !out.is_empty() {
                out.push(' ');
            }
            let _ = write!(out, ">={index}.0.0 <9999.0.0");
            out
        }),
        3 => "1.0.0-\u{3b1}\u{3b2}\u{3b3}".to_string(),
        4 => "\u{ff11}.\u{ff10}.\u{ff10}".to_string(),
        5 => "1.0.0-\u{1f389}".to_string(),
        6 => format!("{}1.0.0", "^".repeat(1 + seeded.below(60))),
        7 => "99999999999999999999.0.0".to_string(),
        8 => format!("18446744073709551615.{}.0", seeded.below(1000)),
        9 => format!("1.0.{}", "0".repeat(1 + seeded.below(50_000))),
        10 => (*seeded.pick(&["", "   ", "\t", "\u{feff}"])).to_string(),
        11 => (*seeded.pick(&["*", "x", "X", "1.x.3", "x.2.3"])).to_string(),
        12 => (*seeded.pick(&[">=", "<", "^", "~", "=", "||", " - "])).to_string(),
        13 => format!("1.2.3.4.5{}", ".6".repeat(seeded.below(20))),
        14 => format!("v{}V1.0.0", "v".repeat(seeded.below(10))),
        15 => format!("1.0.0+{}", "build.".repeat(1 + seeded.below(200))),
        16 => (*seeded.pick(&["\"", "\\", "'", "\u{0}", "a\u{0}b"])).to_string(),
        _ => modelled(ecosystem, seeded).to_string(),
    };
    // A Cargo dependency value is a whole TOML value, so a bare string
    // has to be quoted before it is one.
    match ecosystem {
        Ecosystem::Cargo if !bare.starts_with(['{', '"']) => toml_string(&bare),
        _ => bare,
    }
}

// -------------------------------------------------------------- invariants

struct Case {
    name: String,
    files: Vec<(String, String)>,
    dependency: &'static str,
    /// The right-hand document is in a grammar the tool does not model.
    refused: bool,
}

fn build(ecosystem: Ecosystem, left: &str, right: &str, refused: bool, name: String) -> Case {
    let file = ecosystem.file();
    let (a, b) = match ecosystem {
        Ecosystem::Ci => (
            ".github/workflows/a.yml".to_string(),
            ".github/workflows/b.yml".to_string(),
        ),
        _ => (format!("a/{file}"), format!("b/{file}")),
    };
    Case {
        name,
        files: vec![
            (a, ecosystem.document(left)),
            (b, ecosystem.document(right)),
        ],
        dependency: ecosystem.dependency(),
        refused,
    }
}

/// Every property a report must hold whatever it was handed.
fn check(case: &Case, response: &serde_json::Value) {
    let name = &case.name;
    assert!(
        response.get("error").is_none(),
        "{name}: the tool call failed — {response}\n{}",
        rendered(case)
    );
    let envelope = &response["result"]["structuredContent"];
    assert_eq!(
        envelope["meta"]["tool"], "compare_versions",
        "{name}: not the tool's envelope — {response}"
    );

    let data = &envelope["data"];
    assert_eq!(data["schema"], 1, "{name}: no schema on the report");

    let supplied: Vec<&str> = case.files.iter().map(|(path, _)| path.as_str()).collect();
    let manifests = data["manifests"].as_array().expect("a list");
    assert_eq!(manifests.len(), case.files.len(), "{name}: {data}");
    for manifest in manifests {
        let path = manifest["path"].as_str().unwrap_or_default();
        assert!(
            supplied.contains(&path),
            "{name}: the report names {path}, which nobody supplied"
        );
    }

    let findings = data["findings"].as_array().expect("a list");
    let refusals = data["refusals"].as_array().expect("a list");
    assert_eq!(
        data["summary"]["findings"].as_u64(),
        u64::try_from(findings.len()).ok(),
        "{name}: the summary disagrees with the findings it summarises"
    );
    assert_eq!(
        data["summary"]["refusals"].as_u64(),
        u64::try_from(refusals.len()).ok(),
        "{name}: the summary disagrees with the refusals it summarises"
    );

    for finding in findings {
        let code = finding["code"].as_str().unwrap_or_default();
        assert!(CODES.contains(&code), "{name}: {code} is not a check");
        let severity = finding["severity"].as_str().unwrap_or_default();
        assert!(
            ["error", "warning", "info"].contains(&severity),
            "{name}: {severity} is not a severity"
        );
        let constraints = sites(finding, &supplied, name);
        // A tool cannot claim a string disagrees with itself. Two
        // distinct *ranges* is the conflict trigger, and one string can
        // only ever model one range.
        if CONFLICTS.contains(&code) {
            assert!(
                constraints.iter().any(|held| held != &constraints[0]),
                "{name}: {code} on {} sites that all say {:?}\n{}",
                constraints.len(),
                constraints.first(),
                rendered(case)
            );
        }
    }

    for refusal in refusals {
        let reason = refusal["reason"].as_str().unwrap_or_default();
        assert!(
            REASONS.contains(&reason),
            "{name}: {reason} is not a reason"
        );
        sites(refusal, &supplied, name);
    }

    if !case.refused {
        return;
    }
    // **The property this file exists for.** One side is a grammar the
    // tool declared it does not model, so it must be named as refused
    // and it must take part in no comparison.
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal["name"].as_str() == Some(case.dependency)),
        "{name}: an unmodelled grammar produced no refusal\n{}\n{data}",
        rendered(case)
    );
    for finding in findings {
        let code = finding["code"].as_str().unwrap_or_default();
        assert!(
            !CONFLICTS.contains(&code) || finding["name"].as_str() != Some(case.dependency),
            "{name}: a conflict was manufactured out of a grammar the tool does not model\n{}\n{finding}",
            rendered(case)
        );
    }
}

/// The `sites` of a finding or refusal: every one names a file the
/// caller supplied, and the constraints are returned for the caller to
/// compare.
fn sites(entry: &serde_json::Value, supplied: &[&str], case: &str) -> Vec<String> {
    entry["sites"]
        .as_array()
        .unwrap_or_else(|| panic!("{case}: no sites on {entry}"))
        .iter()
        .map(|site| {
            let file = site["file"].as_str().unwrap_or_default();
            assert!(
                supplied.contains(&file),
                "{case}: a site names {file}, which nobody supplied"
            );
            site["constraint"].as_str().unwrap_or_default().to_string()
        })
        .collect()
}

/// The documents, short enough to read in a failure and long enough to
/// reproduce it.
fn rendered(case: &Case) -> String {
    let mut out = String::new();
    for (path, content) in &case.files {
        let short: String = content.chars().take(400).collect();
        let _ = write!(out, "\n--- {path}\n{short}");
    }
    out
}

// ------------------------------------------------------------------ cases

#[test]
fn no_generated_manifest_pair_panics_hangs_or_fabricates_a_conflict() {
    let seed = seed();
    let budget = budget();
    eprintln!("fuzz: seed {seed:#x}, budget {budget:?}");

    let mut server = Server::start();
    let mut seeded = Seeded(seed);
    let mut cases = 0usize;
    let mut refusal_cases = 0usize;

    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        let ecosystem = *seeded.pick(&ECOSYSTEMS);
        let refuse = unmodelled(ecosystem, &mut seeded).filter(|_| seeded.below(2) == 0);
        let case = match refuse {
            Some(right) => {
                refusal_cases += 1;
                build(
                    ecosystem,
                    modelled(ecosystem, &mut seeded),
                    right,
                    true,
                    format!("seed {seed:#x} refusal case {cases}"),
                )
            }
            None => build(
                ecosystem,
                &hostile(ecosystem, &mut seeded),
                &hostile(ecosystem, &mut seeded),
                false,
                format!("seed {seed:#x} case {cases}"),
            ),
        };
        let response = server.compare(&case.name, &case.files);
        check(&case, &response);
        cases += 1;
    }

    eprintln!("fuzz: {cases} manifest pairs, {refusal_cases} of them refusals, seed {seed:#x}");
    assert!(cases > 0, "the fuzzer ran no cases");
    assert!(
        refusal_cases > 0,
        "no unmodelled grammar was tried, so the refusal property was never checked"
    );
}

/// The pathological shapes by name, outside the random path, so a
/// failure says what broke rather than which seed found it.
#[test]
fn pathological_constraints_terminate() {
    let mut server = Server::start();
    let alternatives = (0..500).fold(String::new(), |mut out, index| {
        if !out.is_empty() {
            out.push_str(" || ");
        }
        let _ = write!(out, ">={index}.0.0 <{}.0.0", index + 1);
        out
    });
    let intersections = (0..2_000).fold(String::new(), |mut out, index| {
        if !out.is_empty() {
            out.push(' ');
        }
        let _ = write!(out, ">={}.0.0 <9999.0.0", index % 100);
        out
    });

    let cases: [(&str, Ecosystem, String, String); 6] = [
        (
            "five hundred alternatives against five hundred more",
            Ecosystem::Npm,
            alternatives.clone(),
            alternatives.clone(),
        ),
        (
            "two thousand intersected comparators",
            Ecosystem::Npm,
            intersections,
            "^1.0.0".to_string(),
        ),
        (
            "a five hundred segment prerelease chain",
            Ecosystem::Npm,
            format!("1.0.0-{}", vec!["rc.1"; 500].join(".")),
            "1.0.0".to_string(),
        ),
        (
            "a fifty thousand character version",
            Ecosystem::Npm,
            format!("1.0.{}", "0".repeat(50_000)),
            "^1.0.0".to_string(),
        ),
        (
            "alternatives against a workspace protocol",
            Ecosystem::Npm,
            alternatives,
            "workspace:*".to_string(),
        ),
        (
            "unicode digits and an emoji prerelease",
            Ecosystem::Npm,
            "\u{ff11}.\u{ff10}.\u{ff10}".to_string(),
            "1.0.0-\u{1f389}".to_string(),
        ),
    ];

    for (name, ecosystem, left, right) in cases {
        let refused = right == "workspace:*";
        let case = build(ecosystem, &left, &right, refused, name.to_string());
        let started = Instant::now();
        let response = server.compare(name, &case.files);
        check(&case, &response);
        eprintln!("fuzz: {name} answered in {:?}", started.elapsed());
    }
}

/// Every ecosystem's refusal pool, exhaustively rather than by sampling.
/// A pool entry that stopped being refused would otherwise only show up
/// on the seeds that happened to pick it.
#[test]
fn every_unmodelled_grammar_in_the_pool_is_refused_and_never_compared() {
    let mut server = Server::start();
    let mut seeded = Seeded(seed());
    let mut checked = 0usize;

    for ecosystem in ECOSYSTEMS {
        // The pool is behind `pick`, so it is walked by drawing from it
        // often enough to see all of it — the pools are small and the
        // generator is deterministic.
        let mut seen: Vec<&str> = Vec::new();
        for _ in 0..400 {
            let Some(entry) = unmodelled(ecosystem, &mut seeded) else {
                break;
            };
            if seen.contains(&entry) {
                continue;
            }
            seen.push(entry);
            let name = format!("pool {} {entry}", ecosystem.file());
            let case = build(
                ecosystem,
                modelled(ecosystem, &mut seeded),
                entry,
                true,
                name.clone(),
            );
            let response = server.compare(&name, &case.files);
            check(&case, &response);
            checked += 1;
        }
    }

    eprintln!("fuzz: {checked} unmodelled grammars refused and never compared");
    assert!(checked >= 30, "only {checked} pool entries were reached");
}
