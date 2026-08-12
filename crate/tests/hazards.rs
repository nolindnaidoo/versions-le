//! Inputs a real machine holds and a fixture directory cannot.
//!
//! `detect/` is pure and tests from the corpus, which is exactly why
//! nothing in the corpus can be a byte-order mark, a named pipe, a
//! mode-000 file or a symlink loop. Those live on the other side of the
//! split — in `discover.rs` and `scan.rs` — and this is where they are
//! answered.
//!
//! The tree is **built at runtime**, not checked in: Windows cannot hold
//! a FIFO, a permission-denied file, or a path over 260 characters in
//! git. Each case the platform cannot express says so by name — see
//! `skipped()` — and a skip is never reported as a pass.
//!
//! Every case asserts the same floor: the process does not panic, does
//! not hang, and exits 0, 1 or 2 — never on a signal. And the one this
//! crate cares about most, asserted everywhere a file misbehaves: **a
//! manifest never silently vanishes from the report.** A file that
//! dropped out without a diagnostic reads to whoever ran it as a
//! manifest that was clean.

use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const BINARY: &str = env!("CARGO_BIN_EXE_versions-le");

/// Generous enough for a shared runner walking a 50 MB manifest, tight
/// enough that a blocking read on a FIFO is a failure rather than a
/// coffee break.
const LIMIT: Duration = Duration::from_secs(120);

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "versions-le-hazard-{name}-{}-{unique}",
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

    fn at(&self, relative: &str) -> String {
        self.root.join(relative).to_string_lossy().into_owned()
    }

    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        self.write_bytes(relative, contents.as_bytes())
    }

    fn write_bytes(&self, relative: &str, contents: &[u8]) -> PathBuf {
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

/// Runs the binary and **fails rather than blocks**. A hang is the
/// failure mode this file exists to catch — a FIFO named `Cargo.toml` is
/// one `read` away from an eternal CI job — so the child is killed and
/// the case is named.
fn run(case: &str, args: &[&str]) -> Run {
    let mut child = Command::new(BINARY)
        .args(args)
        // Never inherit the terminal: `mcp` reads stdin, and a child
        // waiting on a keyboard that is not there is a hang with an
        // innocent explanation.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");

    // Drained on threads: a report larger than a pipe buffer would
    // deadlock a parent that waits before reading, and this suite writes
    // some large ones.
    let mut out = child.stdout.take().expect("stdout");
    let mut err = child.stderr.take().expect("stderr");
    let out = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = out.read_to_end(&mut buffer);
        buffer
    });
    let err = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = err.read_to_end(&mut buffer);
        buffer
    });

    let deadline = Instant::now() + LIMIT;
    let status = loop {
        match child.try_wait().expect("the child is waitable") {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{case}: hung for {LIMIT:?} on {args:?}");
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };

    let code = status.code().unwrap_or_else(|| {
        panic!("{case}: died on a signal rather than exiting ({status:?}) on {args:?}")
    });
    assert!(
        (0..=2).contains(&code),
        "{case}: exit {code} is outside the documented 0/1/2 on {args:?}"
    );
    Run {
        code,
        stdout: String::from_utf8_lossy(&out.join().expect("stdout thread")).into_owned(),
        stderr: String::from_utf8_lossy(&err.join().expect("stderr thread")).into_owned(),
    }
}

/// The report, parsed. Doubles as the assertion that a run which reached
/// the analysis wrote one JSON object to stdout and nothing else.
fn report(case: &str, run: &Run) -> serde_json::Value {
    serde_json::from_str(run.stdout.trim()).unwrap_or_else(|error| {
        panic!(
            "{case}: stdout is not one report ({error}) — {}",
            run.stdout
        )
    })
}

fn manifests(report: &serde_json::Value) -> Vec<String> {
    report["manifests"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|manifest| manifest["path"].as_str().expect("a path").to_string())
        .collect()
}

fn codes(report: &serde_json::Value) -> Vec<String> {
    report["findings"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|finding| finding["code"].as_str().expect("a code").to_string())
        .collect()
}

/// A case the platform cannot express. Named on stderr so a green run
/// still says what it did not check — a silent skip is a lie.
fn skipped(case: &str, why: &str) {
    eprintln!("SKIPPED {case}: {why}");
}

/// One manifest of each kind, and how many constraints a reader must
/// find in it. The content is deliberately dull: these cases are about
/// the bytes around the document, not the constraints inside it.
const MANIFESTS: [(&str, &str, usize); 5] = [
    (
        "package.json",
        "{ \"dependencies\": { \"left-pad\": \"^1.3.0\" }, \"engines\": { \"node\": \">=20\" } }",
        2,
    ),
    (
        "Cargo.toml",
        "[package]\nrust-version = \"1.88\"\n[dependencies]\nserde = \"1.0.200\"\n",
        2,
    ),
    (
        "pyproject.toml",
        "[project]\nrequires-python = \">=3.11\"\ndependencies = [\"requests>=2.31\"]\n",
        2,
    ),
    (
        "go.mod",
        "module example.com/a\n\ngo 1.22\n\nrequire github.com/x/y v1.4.0\n",
        2,
    ),
    (
        ".github/workflows/ci.yml",
        "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@v4\n        with:\n          node-version: 20\n",
        2,
    ),
];

// ---------------------------------------------------------------- content

/// A byte-order mark is three invisible bytes that Notepad, Excel and a
/// PowerShell redirect all add. Before a `{` it makes a JSON parser
/// reject the whole document — which is indistinguishable from a
/// manifest with no dependencies in it, and therefore reads as clean.
#[test]
fn a_byte_order_mark_changes_no_manifest_of_any_kind() {
    for (name, body, entries) in MANIFESTS {
        let plain = Tree::new("bom-plain");
        let marked = Tree::new("bom-marked");
        plain.write(name, body);
        marked.write(name, &format!("\u{feff}{body}"));

        let without = run("bom", &[&plain.path().to_string_lossy()]);
        let with = run("bom", &[&marked.path().to_string_lossy()]);
        assert_eq!(
            without.stdout, with.stdout,
            "a byte-order mark changed how {name} was read\n  without: {}\n  with:   {}",
            without.stdout, with.stdout
        );
        let parsed = report("bom", &with);
        assert!(
            parsed["diagnostics"].as_array().expect("a list").is_empty(),
            "{name}: {}",
            with.stderr
        );
        assert_eq!(parsed["summary"]["manifests"], 1, "{name}");
        assert_eq!(
            parsed["summary"]["entries"], entries,
            "{name}: a byte-order mark cost the reader its constraints"
        );
    }
}

/// **Never silently absent.** A `Cargo.toml` that is not UTF-8 is named
/// on stderr and carried in `diagnostics`; what is not allowed is a run
/// that reports on the manifests it could read and says nothing about
/// the one it could not. The manifests that did parse still answer.
#[test]
fn an_undecodable_manifest_is_named_and_the_rest_still_answer() {
    for (case, bytes) in [
        (
            "invalid-utf8",
            vec![b'[', b'd', b'e', b'p', 0xff, 0xfe, b'\n'],
        ),
        // UTF-16LE with a BOM: what Notepad writes when asked for
        // "Unicode", and not UTF-8 by any reading.
        (
            "utf16le",
            vec![0xff, 0xfe, b'[', 0x00, b'd', 0x00, b'e', 0x00],
        ),
    ] {
        let tree = Tree::new(case);
        tree.write_bytes("broken/Cargo.toml", &bytes);
        tree.write("a/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        tree.write("b/Cargo.toml", "[dependencies]\nserde = \"2\"\n");
        let root = tree.path().to_string_lossy().to_string();

        let relaxed = run(case, &[&root]);
        assert_eq!(relaxed.code, 1, "{case}: the readable pair still answers");
        let parsed = report(case, &relaxed);
        assert_eq!(codes(&parsed), ["disjoint-constraint"], "{case}");
        assert_eq!(parsed["diagnostics"][0]["code"], "unreadable", "{case}");
        assert!(
            relaxed.stderr.contains("Cargo.toml"),
            "{case}: the diagnostic names no file — {}",
            relaxed.stderr
        );
        assert_eq!(run(case, &["--strict", &root]).code, 2, "{case}");
    }
}

/// A file called `Cargo.toml` that is valid TOML and describes a web
/// server is not a broken manifest — it is a manifest with no dependency
/// keys in it. A reader that failed the file would turn somebody's
/// unusual repository into a red build; a reader that invented entries
/// from `[server]` would be worse.
#[test]
fn a_document_that_parses_but_is_not_a_manifest_yields_nothing_and_fails_nothing() {
    let tree = Tree::new("not-a-manifest");
    tree.write(
        "svc/Cargo.toml",
        "[server]\nport = 8080\nhost = \"0.0.0.0\"\n\n[logging]\nlevel = \"debug\"\n",
    );
    // Poetry is explicitly not read in v1. It must be silence, not a
    // guess and not a parse failure.
    tree.write(
        "svc/pyproject.toml",
        "[tool.poetry.dependencies]\nrequests = \"^2.31\"\n",
    );
    let run = run("not-a-manifest", &[&tree.path().to_string_lossy()]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let parsed = report("not-a-manifest", &run);
    assert_eq!(parsed["summary"]["manifests"], 2);
    assert_eq!(parsed["summary"]["entries"], 0);
    assert!(
        parsed["diagnostics"].as_array().expect("a list").is_empty(),
        "{}",
        run.stderr
    );
    assert!(codes(&parsed).is_empty());
}

/// An empty file is the shape a half-finished `git add` leaves behind.
/// Each reader answers it in its own way — TOML is an empty document,
/// JSON is a parse error — and both answers are fine. What is not fine
/// is the manifest disappearing from the report without a diagnostic.
#[test]
fn an_empty_manifest_is_still_a_manifest_in_the_report() {
    for (name, _, _) in MANIFESTS {
        let tree = Tree::new("empty");
        tree.write(name, "");
        let run = run("empty", &[&tree.path().to_string_lossy()]);
        let parsed = report("empty", &run);
        assert_eq!(parsed["summary"]["manifests"], 1, "{name} vanished");
        assert_eq!(parsed["summary"]["entries"], 0, "{name}");
        assert_ne!(run.code, 1, "{name}: an empty file cannot be in conflict");

        // Either it parsed to nothing, or it was named as unparseable.
        // Never both, and never neither.
        let diagnostics = parsed["diagnostics"].as_array().expect("a list").len();
        assert!(diagnostics <= 1, "{name}: {}", run.stderr);
        if diagnostics == 1 {
            assert_eq!(parsed["diagnostics"][0]["file"], name, "{name}");
        }
    }
}

/// The readers scan lines as well as parse documents, and a very long
/// one is where a line-scanning reader quietly goes quadratic.
#[test]
fn a_fifty_megabyte_manifest_completes() {
    const ENTRIES: usize = 200_000;

    let tree = Tree::new("huge");
    // A real dependency table interleaved with comment lines, rather
    // than one enormous string value: the reader has to walk all 50 MB
    // either way, and this shape also asks the TOML parser to skip half
    // of it — which is what a generated manifest actually looks like.
    let padding = "#".repeat(240);
    let mut body = String::with_capacity(56 * 1024 * 1024);
    body.push_str("[dependencies]\n");
    for index in 0..ENTRIES {
        let _ = writeln!(
            body,
            "{padding}\na-very-long-dependency-name-number-{index} = \"1.0.{index}\""
        );
    }
    assert!(
        body.len() > 50 * 1024 * 1024,
        "the case is only {} bytes",
        body.len()
    );
    tree.write("Cargo.toml", &body);

    let started = Instant::now();
    let run = run("huge", &[&tree.path().to_string_lossy()]);
    eprintln!(
        "hazards: a {} byte manifest in {:?}",
        body.len(),
        started.elapsed()
    );
    assert_eq!(run.code, 0, "one manifest cannot contradict itself");
    assert_eq!(report("huge", &run)["summary"]["entries"], ENTRIES);
}

// ------------------------------------------------------------- filesystem

#[cfg(unix)]
fn symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

/// Windows needs Developer Mode or an elevated process to create one, so
/// a failure here is the platform refusing rather than the code being
/// wrong — the caller skips by name.
#[cfg(windows)]
fn symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    if original.is_dir() {
        return std::os::windows::fs::symlink_dir(original, link);
    }
    std::os::windows::fs::symlink_file(original, link)
}

/// The walk does not follow links and the caller's own argument does.
///
/// Both halves are deliberate. A tree full of symlinked manifests would
/// otherwise report the same file twice under two labels, which reads as
/// one repository contradicting itself; a file the caller named by hand
/// is intent, and intent beats configuration.
#[test]
fn a_symlinked_manifest_is_read_when_named_and_not_when_merely_walked() {
    let tree = Tree::new("symlink");
    let real = tree.write("real/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    std::fs::create_dir_all(tree.path().join("link")).expect("a directory");
    let link = tree.path().join("link/Cargo.toml");
    if symlink(&real, &link).is_err() {
        skipped("symlink", "this platform refused to create a symlink");
        return;
    }

    let walked = run("symlink", &[&tree.path().to_string_lossy()]);
    assert_eq!(walked.code, 0, "{}", walked.stderr);
    assert_eq!(
        manifests(&report("symlink", &walked)),
        ["real/Cargo.toml"],
        "the walk followed a link and counted one file twice"
    );

    let named = run("symlink-named", &[&link.to_string_lossy()]);
    assert_eq!(named.code, 0, "{}", named.stderr);
    assert_eq!(
        report("symlink-named", &named)["summary"]["entries"],
        1,
        "a manifest named by hand was not read through its link"
    );

    let broken = tree.path().join("nowhere/Cargo.toml");
    std::fs::create_dir_all(tree.path().join("nowhere")).expect("a directory");
    let _ = symlink(&tree.path().join("not-there-at-all"), &broken);
    let refused = run("symlink-broken", &[&broken.to_string_lossy()]);
    assert_eq!(refused.code, 2, "{}", refused.stderr);
    assert!(refused.stdout.is_empty(), "a refusal wrote a report");
}

/// The hang. A directory that contains itself is an infinite tree to a
/// walker that follows links, and a manifest symlinked into a cycle is
/// an infinite `read` to a reader that resolves one.
#[test]
fn a_symlink_loop_does_not_make_the_walk_infinite() {
    let tree = Tree::new("symlink-loop");
    tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    if symlink(tree.path(), &tree.path().join("itself")).is_err() {
        skipped("symlink-loop", "this platform refused to create a symlink");
        return;
    }
    let walked = run("symlink-loop", &[&tree.path().to_string_lossy()]);
    assert_eq!(walked.code, 0, "{}", walked.stderr);
    assert_eq!(manifests(&report("symlink-loop", &walked)), ["Cargo.toml"]);

    let first = tree.path().join("a/Cargo.toml");
    let second = tree.path().join("b/Cargo.toml");
    std::fs::create_dir_all(tree.path().join("a")).expect("a directory");
    std::fs::create_dir_all(tree.path().join("b")).expect("a directory");
    if symlink(&second, &first).is_err() || symlink(&first, &second).is_err() {
        skipped(
            "symlink-loop-file",
            "this platform refused to create a symlink",
        );
        return;
    }
    let named = run("symlink-loop-file", &[&first.to_string_lossy()]);
    assert_eq!(named.code, 2, "{}", named.stderr);
    assert!(named.stdout.is_empty(), "a refusal wrote a report");
}

/// A named pipe with no writer blocks a `read` forever, and every reader
/// here reads a whole file into a string before it looks at it.
#[cfg(unix)]
#[test]
fn a_fifo_named_like_a_manifest_does_not_block_the_walk() {
    let tree = Tree::new("fifo");
    tree.write("real/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    let fifo = tree.path().join("pipe/Cargo.toml");
    std::fs::create_dir_all(tree.path().join("pipe")).expect("a directory");
    // Shelled out rather than called through libc: `unsafe` is forbidden
    // crate-wide and a test is not an exemption.
    let made = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|status| status.success());
    if !made {
        skipped("fifo", "mkfifo is not available on this runner");
        return;
    }

    let walked = run("fifo", &[&tree.path().to_string_lossy()]);
    assert_eq!(walked.code, 0, "{}", walked.stderr);
    assert_eq!(
        manifests(&report("fifo", &walked)),
        ["real/Cargo.toml"],
        "the walk opened a named pipe"
    );

    // Named by hand it is not a regular file either, so the walk finds
    // nothing in it — and finding nothing is exit 0, not a hang.
    let named = run("fifo-named", &[&fifo.to_string_lossy()]);
    assert!((0..=2).contains(&named.code), "{}", named.stderr);
}

#[cfg(not(unix))]
#[test]
fn a_fifo_named_like_a_manifest_does_not_block_the_walk() {
    skipped("fifo", "Windows has no FIFO in a directory tree");
}

/// A manifest the tool cannot open is a diagnostic, never a silence.
/// Unlike a root the caller named, one file in fifty being unreadable
/// does not make the *question* malformed — so the rest still answer and
/// only `--strict` turns it into exit 2.
#[cfg(unix)]
#[test]
fn a_permission_denied_manifest_is_named_and_only_strict_makes_it_fatal() {
    use std::os::unix::fs::PermissionsExt;

    let tree = Tree::new("denied");
    let denied = tree.write("locked/Cargo.toml", "[dependencies]\nserde = \"9\"\n");
    tree.write("a/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    if std::fs::read(&denied).is_ok() {
        skipped(
            "permission-denied",
            "this runner reads a mode-000 file anyway (root)",
        );
        return;
    }
    let root = tree.path().to_string_lossy().to_string();

    let relaxed = run("denied", &[&root]);
    assert_eq!(relaxed.code, 0, "{}", relaxed.stderr);
    let parsed = report("denied", &relaxed);
    assert_eq!(parsed["diagnostics"][0]["code"], "unreadable");
    assert_eq!(parsed["diagnostics"][0]["file"], "Cargo.toml");
    assert!(
        relaxed.stderr.contains("Cargo.toml"),
        "the diagnostic names no file — {}",
        relaxed.stderr
    );
    assert_eq!(run("denied", &["--strict", &root]).code, 2);
}

#[cfg(not(unix))]
#[test]
fn a_permission_denied_manifest_is_named_and_only_strict_makes_it_fatal() {
    skipped(
        "permission-denied",
        "Windows ACLs are not chmod; the unix case covers the read failure",
    );
}

/// Where Windows differs: `MAX_PATH` is 260 characters unless long paths
/// are enabled, so the creation itself is the platform's answer. The
/// label matters as much as the read — a report that truncated a deep
/// path would name a file nobody can find.
#[test]
fn a_path_over_260_characters_is_walked_and_labelled_whole() {
    let tree = Tree::new("long-path");
    let mut deep = String::new();
    while deep.len() < 300 {
        deep.push_str("a-directory-with-a-long-name/");
    }
    let relative = format!("{deep}Cargo.toml");
    let target = tree.root.join(&relative);
    if std::fs::create_dir_all(target.parent().expect("a parent")).is_err()
        || std::fs::write(&target, "[dependencies]\nserde = \"1\"\n").is_err()
    {
        skipped(
            "long-path",
            "this platform refused a path over 260 characters",
        );
        return;
    }

    let run = run("long-path", &[&tree.path().to_string_lossy()]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(manifests(&report("long-path", &run)), [relative]);
}

/// **Discovery is the tree, never the workspace graph.** A `members`
/// list is not resolved, so a crate outside the root the caller named
/// contributes nothing — not its constraints, not a conflict, not a
/// diagnostic. A tool that followed `../` out of its own root would read
/// files nobody pointed it at, and report a conflict between a tree and
/// its neighbour.
#[test]
fn a_workspace_pointing_outside_the_root_stays_outside_it() {
    let tree = Tree::new("workspace-escape");
    tree.write(
        "inside/Cargo.toml",
        "[workspace]\nmembers = [\"../outside/*\", \"crates/*\"]\n\n[workspace.dependencies]\nserde = \"1\"\n",
    );
    tree.write(
        "inside/crates/api/Cargo.toml",
        "[dependencies]\nserde = \"1\"\n",
    );
    tree.write(
        "outside/worker/Cargo.toml",
        "[dependencies]\nserde = \"2\"\n",
    );

    let run = run("workspace-escape", &[&tree.at("inside")]);
    assert_eq!(run.code, 0, "a neighbour was pulled in — {}", run.stderr);
    let parsed = report("workspace-escape", &run);
    assert_eq!(
        manifests(&parsed),
        ["Cargo.toml", "crates/api/Cargo.toml"],
        "the walk left the root it was given"
    );
    assert!(
        !run.stdout.contains("worker") && !run.stderr.contains("worker"),
        "a manifest outside the root reached the report"
    );
}

// ------------------------------------------------------------- exit codes

/// Exit 2 is for a malformed *question* — the flag, the ecosystem name,
/// the root. It is never what one unreadable manifest in fifty earns,
/// and a refusal never writes to the protocol stream.
#[test]
fn every_malformed_question_exits_two_and_leaves_stdout_empty() {
    let tree = Tree::new("exit-two");
    let missing = tree.at("not-here");
    let readme = tree.write("README.md", "not a manifest\n");
    let readme = readme.to_string_lossy().into_owned();
    let directory = tree.path().to_string_lossy().into_owned();

    for args in [
        vec!["--not-a-flag", directory.as_str()],
        vec!["--ecosystem", "maven", directory.as_str()],
        vec!["--fail-on", "sometimes", directory.as_str()],
        vec!["--exclude"],
        vec![missing.as_str()],
        vec![readme.as_str()],
    ] {
        let refused = run("exit-two", &args);
        assert_eq!(refused.code, 2, "{args:?}: {}", refused.stderr);
        assert!(
            refused.stdout.is_empty(),
            "{args:?} wrote to the protocol stream"
        );
        assert!(
            refused.stderr.contains("versions-le:"),
            "{args:?}: the refusal does not say who refused — {}",
            refused.stderr
        );
    }
}
