//! Behaviour that differs by operating system, asserted rather than
//! hoped.
//!
//! **A sibling crate in this family shipped a release whose report used
//! `\` on Windows and `/` everywhere else**, red on Windows CI for the
//! whole release before anyone looked. This crate is directly exposed:
//! every manifest it reads is named in the report four times over — in
//! `manifests[].path`, in the `sites[].file` of a finding, in the
//! `sites[].file` of a refusal, and in `diagnostics[].file` — and those
//! names are the thing a reader greps for, diffs against a baseline, and
//! pastes back into a shell. A report whose separators follow the
//! machine is a report that cannot be diffed between two machines.
//!
//! The other half of the file is what the platform does to the walk:
//! `Cargo.toml` and `cargo.toml` are one file on macOS and Windows and
//! two on Linux, `CON` is a device name on Windows and an ordinary
//! directory elsewhere, and a manifest saved by a Windows editor has
//! CRLF line endings that two of the readers scan by hand.

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
            "versions-le-platform-{name}-{}-{unique}",
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

fn run_with(args: &[&str], environment: &[(&str, Option<&str>)]) -> Run {
    let mut command = Command::new(BINARY);
    command.args(args).stdin(Stdio::null());
    for (name, value) in environment {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    let output = command.output().expect("the binary runs");
    Run {
        code: output.status.code().expect("an exit code, never a signal"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn run(args: &[&str]) -> Run {
    run_with(args, &[])
}

fn report(run: &Run) -> serde_json::Value {
    serde_json::from_str(run.stdout.trim())
        .unwrap_or_else(|error| panic!("stdout is not one report ({error}) — {}", run.stdout))
}

fn skipped(case: &str, why: &str) {
    eprintln!("SKIPPED {case}: {why}");
}

/// Every string the report uses **as a manifest's identity**, wherever it
/// appears: the manifest list, a finding's sites, a refusal's sites, a
/// diagnostic. Collected by key rather than by position so a field added
/// later is covered without anyone remembering to add it here.
fn identities(report: &serde_json::Value) -> Vec<String> {
    let mut found = Vec::new();
    collect(report, &mut found);
    found.sort();
    found.dedup();
    found
}

fn collect(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let names_a_file = matches!(key.as_str(), "path" | "file");
                if let (true, Some(text)) = (names_a_file, child.as_str()) {
                    into.push(text.to_string());
                }
                collect(child, into);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect(item, into);
            }
        }
        _ => {}
    }
}

/// One tree carrying all four kinds of path the report can hold: a
/// manifest that is merely listed, two that contradict each other, one
/// that is refused, and one that will not parse.
fn every_kind_of_path(name: &str) -> Tree {
    let tree = Tree::new(name);
    tree.write("api/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    tree.write("web/nested/Cargo.toml", "[dependencies]\nserde = \"2\"\n");
    tree.write(
        "lib/Cargo.toml",
        "[dependencies]\nshared = { workspace = true }\n",
    );
    tree.write("bad/package.json", "{ not json");
    tree
}

const EVERY_LABEL: [&str; 4] = [
    "api/Cargo.toml",
    "bad/package.json",
    "lib/Cargo.toml",
    "web/nested/Cargo.toml",
];

// ------------------------------------------------------------- separators

/// **The one this file exists for.** A manifest is named the same way on
/// every operating system, or the report is machine-specific and the
/// exit code is the only part of it worth reading.
#[test]
fn every_path_in_the_report_is_written_with_forward_slashes() {
    let tree = every_kind_of_path("separators");
    let run = run(&[&tree.path().to_string_lossy()]);
    assert_eq!(run.code, 1, "{}", run.stderr);

    let found = identities(&report(&run));
    assert_eq!(
        found, EVERY_LABEL,
        "the report does not name the manifests it read"
    );
    for label in &found {
        assert!(
            !label.contains('\\'),
            "{label} carries this machine's separator"
        );
        assert!(
            label.contains('/'),
            "{label} lost the separator it should have"
        );
    }
}

/// The human half is a projection of the same report, so it names the
/// same labels. A summary that spelled a path differently from the JSON
/// beside it would make one of the two impossible to grep for.
#[test]
fn the_human_summary_names_the_same_paths_as_the_report() {
    let tree = every_kind_of_path("summary");
    let run = run(&[&tree.path().to_string_lossy()]);
    for label in EVERY_LABEL {
        // The unreadable-file diagnostic is the one line that has only a
        // basename to point at, and this tree has no unreadable file.
        assert!(
            run.stderr.contains(label),
            "stderr never names {label}\n{}",
            run.stderr
        );
    }
}

/// **The shape that actually shipped red.** A manifest named as its own
/// root is labelled with the whole of the path it was given, and on
/// Windows `canonicalize` hands back an extended-length path — so the
/// label came out as `\\?\C:/Users/runneradmin/…/Cargo.toml`, verbatim
/// marker and backslashes intact, in the one string the report promises
/// has neither.
///
/// The unit tests in `discover.rs` pin the same property, but only this
/// one goes through the binary, and only the binary is what a user runs.
#[test]
fn an_absolute_file_root_is_labelled_without_a_platform_prefix() {
    let tree = Tree::new("file-root");
    let file = tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");

    let run = run(&[&file.to_string_lossy()]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let found = identities(&report(&run));
    assert_eq!(found.len(), 1, "{found:?}");
    for label in &found {
        assert!(!label.contains('\\'), "{label} carries a backslash");
        assert!(
            !label.contains('?'),
            "{label} carries an extended-length marker"
        );
        assert!(label.ends_with("/Cargo.toml"), "{label}");
    }
}

/// Two roots qualify their labels, and the qualification is built from a
/// path this machine supplied — the step where a separator most easily
/// leaks in.
#[test]
fn qualified_labels_keep_forward_slashes_too() {
    let tree = every_kind_of_path("qualified");
    let run = run(&[
        &tree.path().join("api").to_string_lossy(),
        &tree.path().join("web").to_string_lossy(),
    ]);
    for label in identities(&report(&run)) {
        assert!(!label.contains('\\'), "{label} carries a backslash");
    }
}

// ---------------------------------------------------------------- the walk

/// `Cargo.toml` and `cargo.toml` are one file on macOS and Windows and
/// two on Linux. Either answer is correct; what must not happen is a
/// crash, or a report that names a file the toolchains themselves ignore.
///
/// Manifest names are matched case-sensitively on purpose — `Cargo.toml`
/// has exactly one spelling — so on a case-folding filesystem the second
/// write lands in the first file and the walk still sees one manifest,
/// and on a case-sensitive one the lower-case file is simply not a
/// manifest. One either way.
#[test]
fn a_case_folding_filesystem_gives_a_consistent_answer() {
    let tree = Tree::new("case-fold");
    tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    tree.write("cargo.toml", "[dependencies]\nserde = \"2\"\n");

    let run = run(&[&tree.path().to_string_lossy()]);
    assert_eq!(run.code, 0, "one manifest cannot contradict itself");
    let found = identities(&report(&run));
    assert_eq!(found, ["Cargo.toml"], "the walk read a file Cargo ignores");
}

/// `CON`, `PRN`, `AUX`, `NUL` and `COM1` are device names on Windows and
/// ordinary directories everywhere else. The test asserts the tool
/// **survives the creation failing**, never that the directories exist.
#[test]
fn reserved_windows_names_do_not_break_the_walk() {
    let tree = Tree::new("reserved");
    let mut created = Vec::new();
    for name in ["CON", "PRN", "AUX", "NUL", "COM1"] {
        let target = tree.path().join(name).join("Cargo.toml");
        if std::fs::create_dir_all(target.parent().expect("a parent")).is_err()
            || std::fs::write(&target, "[dependencies]\nserde = \"1\"\n").is_err()
        {
            continue;
        }
        created.push(format!("{name}/Cargo.toml"));
    }
    if created.is_empty() {
        skipped("reserved-names", "this platform refused every device name");
        return;
    }

    let run = run(&[&tree.path().to_string_lossy()]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    created.sort();
    assert_eq!(
        identities(&report(&run)),
        created,
        "a reserved name was created and then not walked"
    );
}

/// Two of the five readers scan lines by hand — `go.mod` and the
/// workflow reader — so a manifest saved by a Windows editor is exactly
/// where a stray `\r` becomes part of a version string.
#[test]
fn line_endings_do_not_change_any_manifest() {
    for (name, body) in [
        (
            "Cargo.toml",
            "[package]\nrust-version = \"1.88\"\n[dependencies]\nserde = \"1.0.200\"\n",
        ),
        (
            "go.mod",
            "module example.com/a\n\ngo 1.22\n\nrequire (\n\tgithub.com/x/y v1.4.0\n)\n",
        ),
        (
            ".github/workflows/ci.yml",
            "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@v4\n        with:\n          node-version: 20\n",
        ),
        (
            "pyproject.toml",
            "[project]\nrequires-python = \">=3.11\"\ndependencies = [\"requests>=2.31\"]\n",
        ),
        (
            "package.json",
            "{\n  \"dependencies\": { \"left-pad\": \"^1.3.0\" }\n}\n",
        ),
    ] {
        let unix = Tree::new("lf");
        let windows = Tree::new("crlf");
        unix.write(name, body);
        windows.write(name, &body.replace('\n', "\r\n"));

        let lf = run(&[&unix.path().to_string_lossy()]);
        let crlf = run(&[&windows.path().to_string_lossy()]);
        assert_eq!(
            lf.stdout, crlf.stdout,
            "{name} was read differently with CRLF line endings"
        );
        assert_eq!(lf.code, crlf.code, "{name}");
        assert!(
            report(&crlf)["summary"]["entries"].as_u64().unwrap_or(0) > 0,
            "{name}: the CRLF document yielded no constraints"
        );
    }
}

// ------------------------------------------------------------ the clock

/// The report carries no timestamp and nothing here reads a clock, so
/// the answer cannot depend on `TZ` — which matters because Windows
/// ignores the variable entirely, and a suite that quietly depended on
/// it would be red there and nowhere else.
#[test]
fn the_answer_does_not_depend_on_the_time_zone() {
    let tree = every_kind_of_path("tz");
    let root = tree.path().to_string_lossy().into_owned();
    let args = [root.as_str()];

    let utc = run_with(&args, &[("TZ", Some("UTC"))]);
    let unset = run_with(&args, &[("TZ", None)]);
    let far = run_with(&args, &[("TZ", Some("Pacific/Kiritimati"))]);

    assert_eq!(utc.stdout, unset.stdout, "TZ=UTC differs from TZ unset");
    assert_eq!(utc.stdout, far.stdout, "the report moved with the clock");
    assert_eq!(utc.stderr, unset.stderr);
    assert_eq!(utc.stderr, far.stderr);
    assert_eq!(utc.code, unset.code);
    assert_eq!(utc.code, far.code);
}

// --------------------------------------------------------------- refusals

/// A path the caller typed comes back the way the caller typed it. This
/// is the one place a separator is *not* normalised, and deliberately:
/// a refusal that rewrote the argument it was given could not be grepped
/// for in the terminal scrollback that produced it.
#[test]
fn a_refusal_names_the_path_the_caller_gave_it() {
    let tree = Tree::new("refusal-path");
    let missing = tree.path().join("no").join("such").join("place");
    let given = missing.to_string_lossy().into_owned();

    let run = run(&[&given]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert!(
        run.stderr.contains(&given),
        "the refusal rewrote the path it was given\n  given: {given}\n  said:  {}",
        run.stderr
    );
    assert!(run.stdout.is_empty(), "a refusal wrote to stdout");
}

/// The MCP server reads stdin to end of stream. Closing it immediately is
/// a client that went away, and the answer is a clean exit rather than a
/// hang or a broken pipe.
#[test]
fn the_mcp_server_exits_cleanly_when_stdin_closes_immediately() {
    let mut child = Command::new(BINARY)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("the child finishes");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout was {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}
