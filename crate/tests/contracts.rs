//! The exit codes and the stdout contract, driven against the built
//! binary.
//!
//! These are the API — a shell branches on them — so they are pinned by
//! tests that drive the real binary against a real tree. No network, no
//! privileged operation, so they run everywhere on every push.
//!
//! **A new refusal adds its case here.**

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const BINARY: &str = env!("CARGO_BIN_EXE_versions-le");
static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "versions-le-contract-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a temporary directory");
        Self {
            root: std::fs::canonicalize(&root).expect("a canonical directory"),
        }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let target = self.root.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("a parent directory");
        }
        std::fs::write(&target, contents).expect("a file");
        target
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Run {
    let output = Command::new(BINARY)
        .args(args)
        .output()
        .expect("the binary runs");
    Run {
        code: output.status.code().expect("an exit code"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// The whole report, parsed. Doubles as the assertion that stdout is one
/// JSON object and nothing else.
fn report(run: &Run) -> serde_json::Value {
    serde_json::from_str(run.stdout.trim()).expect("stdout carries one JSON report")
}

fn codes(report: &serde_json::Value) -> Vec<String> {
    report["findings"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|finding| finding["code"].as_str().expect("a code").to_string())
        .collect()
}

/// Two crates that disagree about `serde`, and a workflow whose
/// toolchain is older than the declared minimum.
fn drifted(name: &str) -> Tree {
    let tree = Tree::new(name);
    tree.write(
        "api/Cargo.toml",
        "[package]\nname = \"api\"\nrust-version = \"1.88\"\n\n[dependencies]\nserde = \"1.0.200\"\n",
    );
    tree.write(
        "web/Cargo.toml",
        "[package]\nname = \"web\"\n\n[dependencies]\nserde = \"1\"\n",
    );
    tree.write(
        ".github/workflows/ci.yml",
        "jobs:\n  test:\n    steps:\n      - uses: dtolnay/rust-toolchain@1.80\n",
    );
    tree.write("README.md", "not a manifest\n");
    tree
}

#[test]
fn manifests_that_agree_exit_zero() {
    let tree = Tree::new("agree");
    tree.write("a/Cargo.toml", "[dependencies]\nserde = \"1.0.200\"\n");
    tree.write("b/Cargo.toml", "[dependencies]\nserde = \"1.0.200\"\n");
    let run = run(&[&tree.path().to_string_lossy()]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(report(&run)["summary"]["findings"], 0);
    assert!(run.stderr.contains("consistent"), "{}", run.stderr);
}

#[test]
fn a_drifted_constraint_exits_one_and_names_both_sites() {
    let tree = drifted("drift");
    let run = run(&[&tree.path().to_string_lossy()]);
    assert_eq!(run.code, 1, "{}", run.stderr);
    let report = report(&run);
    assert!(codes(&report).contains(&"constraint-conflict".to_string()));
    assert!(run.stderr.contains("api/Cargo.toml"), "{}", run.stderr);
    assert!(run.stderr.contains("web/Cargo.toml"), "{}", run.stderr);
}

/// The strongest finding, and the one that must stay distinguishable
/// from a merely different constraint.
#[test]
fn an_unsatisfiable_pair_is_reported_as_disjoint_and_not_as_a_conflict() {
    let tree = Tree::new("disjoint");
    tree.write("a/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    tree.write("b/Cargo.toml", "[dependencies]\nserde = \"2\"\n");
    let run = run(&[&tree.path().to_string_lossy()]);
    assert_eq!(run.code, 1);
    assert_eq!(codes(&report(&run)), ["disjoint-constraint"]);
    assert!(run.stderr.contains("error"), "{}", run.stderr);
}

/// **The refusal spine, end to end.** A grammar the tool does not model
/// is named and excluded — never turned into a conflict against a real
/// constraint.
#[test]
fn an_unmodelled_grammar_is_a_refusal_and_never_a_finding() {
    let tree = Tree::new("refusal");
    tree.write("a/Cargo.toml", "[dependencies]\nserde = \"1.0.200\"\n");
    tree.write(
        "b/Cargo.toml",
        "[dependencies]\nserde = { workspace = true }\n",
    );
    let run = run(&[&tree.path().to_string_lossy()]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let report = report(&run);
    assert!(codes(&report).is_empty(), "{:?}", codes(&report));
    assert_eq!(report["refusals"][0]["reason"], "unknown_grammar");
    assert!(run.stderr.contains("refused"), "{}", run.stderr);
}

/// The same word in two ecosystems is a coincidence, and the tool says
/// so instead of comparing two different questions.
#[test]
fn a_name_in_two_ecosystems_is_refused_rather_than_compared() {
    let tree = Tree::new("cross");
    tree.write("Cargo.toml", "[dependencies]\nsemver = \"1.0.28\"\n");
    tree.write(
        "package.json",
        r#"{ "dependencies": { "semver": "^7.6.0" } }"#,
    );
    let root = tree.path().to_string_lossy().to_string();
    let relaxed = run(&[&root]);
    assert_eq!(relaxed.code, 0, "{}", relaxed.stderr);
    let report = report(&relaxed);
    assert!(codes(&report).is_empty());
    assert_eq!(report["refusals"][0]["reason"], "cross_ecosystem");
    // It is the answer, not a failure to answer, so --strict leaves it
    // alone.
    assert_eq!(run(&["--strict", &root]).code, 0);
}

/// A `<tool>-version:` key is a naming convention, not a schema, so a
/// value that is not evidently a version is refused rather than read.
#[test]
fn a_matrix_expression_is_an_ambiguous_version_string() {
    let tree = Tree::new("ambiguous");
    tree.write(
        ".github/workflows/ci.yml",
        "jobs:\n  test:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n          node-version: ${{ matrix.node }}\n",
    );
    let root = tree.path().to_string_lossy().to_string();
    let relaxed = run(&[&root]);
    assert_eq!(
        report(&relaxed)["refusals"][0]["reason"],
        "ambiguous_version_string"
    );
    assert_eq!(relaxed.code, 0);
    assert_eq!(run(&["--strict", &root]).code, 2);
}

/// A repository with no manifests is not in conflict — failing a build
/// over it would be the tool inventing a problem.
#[test]
fn no_manifests_exits_zero() {
    let tree = Tree::new("none");
    tree.write("README.md", "nothing here\n");
    let run = run(&[&tree.path().to_string_lossy()]);
    assert_eq!(run.code, 0);
    assert_eq!(report(&run)["summary"]["manifests"], 0);
    assert!(run.stderr.contains("no manifests"), "{}", run.stderr);
}

#[test]
fn a_floating_pin_fails_only_under_fail_on_any() {
    let tree = Tree::new("floating");
    tree.write(
        "package.json",
        r#"{ "dependencies": { "left-pad": "latest" } }"#,
    );
    let root = tree.path().to_string_lossy().to_string();
    assert_eq!(run(&[&root]).code, 0);
    assert_eq!(codes(&report(&run(&[&root]))), ["floating-pin"]);
    assert_eq!(run(&["--fail-on", "any", &root]).code, 1);
}

/// The one check that reads across ecosystems, and it does so by naming
/// both keys rather than by matching a package name.
#[test]
fn a_ci_toolchain_below_the_declared_msrv_is_a_finding() {
    let tree = drifted("msrv");
    let run = run(&[
        "--ecosystem",
        "cargo",
        "--ecosystem",
        "ci",
        &tree.path().to_string_lossy(),
    ]);
    assert!(codes(&report(&run)).contains(&"msrv-mismatch".to_string()));
    assert!(run.stderr.contains("1.80"), "{}", run.stderr);
}

#[test]
fn an_ecosystem_filter_narrows_the_walk() {
    let tree = Tree::new("filter");
    tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    tree.write("package.json", r#"{ "dependencies": { "a": "1" } }"#);
    let root = tree.path().to_string_lossy().to_string();
    assert_eq!(report(&run(&[&root]))["summary"]["manifests"], 2);
    assert_eq!(
        report(&run(&["--ecosystem", "cargo", &root]))["summary"]["manifests"],
        1
    );
}

#[test]
fn an_exclude_pattern_removes_a_manifest_from_the_comparison() {
    let tree = Tree::new("exclude");
    tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    tree.write(
        "examples/demo/Cargo.toml",
        "[dependencies]\nserde = \"2\"\n",
    );
    let root = tree.path().to_string_lossy().to_string();
    assert_eq!(run(&[&root]).code, 1);
    let excluded = run(&["--exclude", "examples/**", &root]);
    assert_eq!(excluded.code, 0, "{}", excluded.stderr);
}

/// `.github` is a hidden directory, and a tool that needed a flag to see
/// it would ship with the CI half switched off.
#[test]
fn the_workflow_directory_is_walked_but_other_hidden_directories_are_not() {
    let tree = Tree::new("hidden");
    tree.write(
        ".github/workflows/ci.yml",
        "steps:\n  - uses: actions/checkout@v4\n",
    );
    tree.write(".cache/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    let root = tree.path().to_string_lossy().to_string();
    assert_eq!(report(&run(&[&root]))["summary"]["manifests"], 1);
    assert_eq!(
        report(&run(&["--hidden", &root]))["summary"]["manifests"],
        2
    );
}

#[test]
fn several_roots_are_analysed_together() {
    let tree = drifted("roots");
    let run = run(&[
        &tree.path().join("api").to_string_lossy(),
        &tree.path().join("web").to_string_lossy(),
    ]);
    assert_eq!(report(&run)["summary"]["manifests"], 2);
    assert_eq!(run.code, 1);
}

#[test]
fn a_named_manifest_is_the_whole_walk() {
    let tree = drifted("named");
    let run = run(&[&tree.path().join("api/Cargo.toml").to_string_lossy()]);
    assert_eq!(report(&run)["summary"]["manifests"], 1);
    assert_eq!(run.code, 0);
}

#[test]
fn an_unreadable_input_exits_two() {
    assert_eq!(run(&["/no/such/place-xyz"]).code, 2);
}

/// A manifest that does not parse is named rather than dropped: a file
/// that vanished from the report reads to whoever ran it as a clean one.
#[test]
fn a_manifest_that_does_not_parse_is_named_and_the_rest_still_answer() {
    let tree = Tree::new("broken");
    tree.write("package.json", "{ not json");
    tree.write("a/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    tree.write("b/Cargo.toml", "[dependencies]\nserde = \"2\"\n");
    let root = tree.path().to_string_lossy().to_string();
    let relaxed = run(&[&root]);
    assert_eq!(relaxed.code, 1, "the Cargo answer still stands");
    assert!(
        relaxed.stderr.contains("package.json"),
        "{}",
        relaxed.stderr
    );
    assert_eq!(report(&relaxed)["diagnostics"][0]["code"], "parse-error");
    assert_eq!(run(&["--strict", &root]).code, 2);
}

#[test]
fn an_unknown_flag_exits_two_and_names_itself() {
    let tree = drifted("badflag");
    let run = run(&["--stict", &tree.path().to_string_lossy()]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("--stict"), "{}", run.stderr);
    assert!(run.stdout.is_empty(), "a refusal writes no report");
}

#[test]
fn an_unknown_ecosystem_exits_two_rather_than_widening_the_walk() {
    let tree = drifted("badecosystem");
    let run = run(&["--ecosystem", "maven", &tree.path().to_string_lossy()]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("maven"), "{}", run.stderr);
}

/// Nothing here writes, and no flag may suggest otherwise.
#[test]
fn no_flag_offers_to_change_a_manifest() {
    let tree = drifted("nowrite");
    for attempt in ["--fix", "--write", "--update", "--pin", "--json"] {
        assert_eq!(
            run(&[attempt, &tree.path().to_string_lossy()]).code,
            2,
            "{attempt} was accepted"
        );
    }
}

#[test]
fn version_and_help_exit_clear() {
    let version = run(&["--version"]);
    assert_eq!(version.code, 0);
    assert!(version.stdout.contains("versions-le"));
    let help = run(&["--help"]);
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("usage: versions-le"));
    assert!(help.stdout.contains("never crosses an ecosystem"));
}

/// A report is a thing to diff. One object, no timestamp, and the same
/// bytes for the same tree.
#[test]
fn stdout_is_one_report_with_no_timestamp_and_is_byte_stable() {
    let tree = drifted("streams");
    let first = run(&[&tree.path().to_string_lossy()]);
    let again = run(&[&tree.path().to_string_lossy()]);
    assert_eq!(first.stdout.trim().lines().count(), 1);
    assert_eq!(first.stdout, again.stdout, "two runs, two reports");
    assert!(!first.stderr.contains('{'), "{}", first.stderr);
    assert_eq!(report(&first)["schema"], 1);
    for absent in ["timestamp", "lastChecked", "generatedAt"] {
        assert!(!first.stdout.contains(absent), "{}", first.stdout);
    }
}

/// **The cross-surface contract.** Both surfaces call one entry point,
/// so they must answer identically for the same tree.
#[test]
fn the_cli_and_the_mcp_server_report_the_same_thing() {
    let tree = drifted("agreement");
    let from_cli = report(&run(&[&tree.path().to_string_lossy()]));

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "versions_le_check",
            "arguments": { "path": tree.path().to_string_lossy() },
        },
    });
    let mut child = Command::new(BINARY)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the server starts");
    writeln!(child.stdin.as_mut().expect("stdin"), "{request}").expect("written");
    let output = child.wait_with_output().expect("finishes");
    let response: serde_json::Value = serde_json::from_slice(
        output
            .stdout
            .split(|byte| *byte == b'\n')
            .next()
            .expect("a line"),
    )
    .expect("the reply is JSON");

    assert_eq!(
        response["result"]["structuredContent"]["data"], from_cli,
        "the two surfaces disagree"
    );
}
