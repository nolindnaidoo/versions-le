//! A wall-clock ceiling, and two shape checks on how the clock moves.
//!
//! A sibling crate was fifty times slower than the rest of the family
//! for a whole release and nothing noticed, because nothing measured it.
//! The ceiling here is deliberately loose — a shared runner is not a
//! benchmark rig — and exists to catch an order of magnitude, not a
//! percent.
//!
//! Two linearity assertions, because a repository grows in two
//! directions and only one of them is the obvious one:
//!
//! - **four times the manifests** must not cost six times the clock;
//! - **four times the dependencies in one manifest** must not either.
//!   That is the one this crate is exposed to. Disjointness is interval
//!   arithmetic over *pairs*, and a comparison that reached for every
//!   entry rather than every entry **of one name** would be quadratic in
//!   the width of a manifest — invisible in a corpus of small files and
//!   fatal on the one application `package.json` that has accumulated
//!   eight thousand dependencies. Nothing else in the suite would show
//!   it.
//!
//! **No filesystem.** The documents are generated and handed to
//! `compare_versions`, the tool that takes manifest contents directly,
//! so this measures `detect/` and not the walker.
//!
//! The corpus **agrees** on purpose. `first_disjoint_pair` is quadratic
//! in the sites of a single *drifted* name by design — a handful even in
//! a large monorepo, and documented as such — so planting drift here
//! would measure a known cost rather than catch a regression.
//!
//! Gated behind `VERSIONS_LE_BUDGET` and run by CI on one platform with
//! `--test-threads=1`; a timing assertion measured against six other
//! tests on the same cores is noise. A skipped run says so by name.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

const BINARY: &str = env!("CARGO_BIN_EXE_versions-le");

/// The corpus: 500 manifests of 20 dependencies each, generated from a
/// fixed seed rather than checked in — 500 near-identical `Cargo.toml`
/// files in git would be 500 a reviewer has to ignore — and from a
/// **fixed** seed, so two runs measure the same corpus.
const SEED: u64 = 0x7e12_0a5e_0b1d_2026;
const DOCUMENTS: usize = 500;
const DEPENDENCIES: usize = 20;

/// **~10× the local measurement**, recorded with the machine it came
/// from: 75-80 ms for the 500-manifest corpus on an Apple M-series
/// laptop, debug build, 2026-08.
///
/// **The runner number is not in here yet, and that is stated rather
/// than guessed.** This job had not run when the ceiling was set, so the
/// only evidence available was the CI run of the unit suite on the same
/// commit — `ubuntu-24.04` finished it in 0.03 s against 0.01 s here,
/// so a shared runner is roughly three times slower at the pure-CPU work
/// this test measures. That puts the expected runner figure near 240 ms,
/// about three times under the ceiling: loose enough not to flake, tight
/// enough that an order of magnitude cannot hide.
///
/// Every run prints its own elapsed time, so the real number is the
/// first line of the first `budget` job's log. **Whoever lands that run
/// should replace this paragraph with it** — a ceiling calibrated only
/// against a laptop is a ceiling nobody can defend when it goes red.
const BUDGET: Duration = Duration::from_millis(750);

/// Four times the work, at most six times the clock.
const LINEARITY: f64 = 6.0;

fn enabled(name: &str) -> bool {
    if std::env::var_os("VERSIONS_LE_BUDGET").is_some() {
        return true;
    }
    eprintln!("SKIPPED {name}: set VERSIONS_LE_BUDGET to run it");
    false
}

/// xorshift64*, four lines and identical on every platform.
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
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Server {
    fn start() -> Self {
        let mut child = Command::new(BINARY)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the server starts");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    /// One call carrying the whole corpus, because the question this
    /// crate answers is about the *set* of manifests. Timing one call
    /// per document would measure the protocol instead.
    fn compare(&mut self, request: &str) {
        writeln!(self.stdin, "{request}").expect("the server reads");
        self.stdin.flush().expect("flushed");
        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .expect("the server answers");
        assert!(read > 0, "the server closed stdout mid-run");
        assert!(
            line.contains("\"compare_versions\""),
            "the measured call failed: {}",
            &line[..line.len().min(400)]
        );
        assert!(
            !line.contains("\"isError\":true"),
            "the measured call failed: {}",
            &line[..line.len().min(400)]
        );
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One `Cargo.toml` of roughly the shape a workspace member has: a
/// dependency table drawn from a **shared** name pool, so every name has
/// a site in every manifest and the grouping has real work to do.
fn document(seeded: &mut Seeded, dependencies: usize) -> String {
    let mut out = String::from("[package]\nrust-version = \"1.88\"\n\n[dependencies]\n");
    for index in 0..dependencies {
        // Same name, same requirement, three spellings of the source —
        // the reader still has to look at all of them.
        let _ = match seeded.below(3) {
            0 => writeln!(out, "dep-{index} = \"1.0.200\""),
            1 => writeln!(out, "dep-{index} = {{ version = \"1.0.200\" }}"),
            _ => writeln!(
                out,
                "dep-{index} = {{ version = \"1.0.200\", features = [\"std\"] }}"
            ),
        };
    }
    out
}

/// The request for one corpus, built once and reused across the three
/// rounds so the JSON encoding is not part of what is measured.
fn request(documents: usize, dependencies: usize) -> String {
    let mut seeded = Seeded(SEED);
    let files: Vec<serde_json::Value> = (0..documents)
        .map(|index| {
            serde_json::json!({
                "path": format!("crates/c{index}/Cargo.toml"),
                "content": document(&mut seeded, dependencies),
            })
        })
        .collect();
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "compare_versions", "arguments": { "files": files } },
    })
    .to_string()
}

/// The fastest of three runs. The fastest, not the mean: a shared runner
/// pauses for reasons that have nothing to do with this code, and the
/// question is what the work costs, not what the neighbours cost.
fn fastest(documents: usize, dependencies: usize) -> Duration {
    let request = request(documents, dependencies);
    let mut server = Server::start();
    // One warm-up call outside the measurement: the first answer pays
    // for the process reaching its steady state, not for the analysis.
    server.compare(&request);

    let mut best = Duration::MAX;
    for _ in 0..3 {
        let started = Instant::now();
        server.compare(&request);
        best = best.min(started.elapsed());
    }
    best
}

#[test]
fn five_hundred_manifests_are_compared_inside_their_budget() {
    if !enabled("five_hundred_manifests_are_compared_inside_their_budget") {
        return;
    }
    let elapsed = fastest(DOCUMENTS, DEPENDENCIES);
    eprintln!("budget: {DOCUMENTS} manifests in {elapsed:?} (ceiling {BUDGET:?}, seed {SEED:#x})");
    assert!(
        elapsed <= BUDGET,
        "{DOCUMENTS} manifests took {elapsed:?}, over the {BUDGET:?} ceiling (seed {SEED:#x})"
    );
}

#[test]
fn four_times_the_manifests_is_not_six_times_the_clock() {
    if !enabled("four_times_the_manifests_is_not_six_times_the_clock") {
        return;
    }
    let one = fastest(DOCUMENTS, DEPENDENCIES);
    let four = fastest(DOCUMENTS * 4, DEPENDENCIES);
    let ratio = four.as_secs_f64() / one.as_secs_f64().max(f64::EPSILON);
    eprintln!(
        "budget: {one:?} for {DOCUMENTS} manifests, {four:?} for {}, ratio {ratio:.2}",
        DOCUMENTS * 4
    );
    assert!(
        ratio <= LINEARITY,
        "four times the manifests cost {ratio:.2}× the time (limit {LINEARITY}×), seed {SEED:#x}"
    );
}

/// **The one this crate is exposed to.** Disjointness is decided pairwise
/// over the sites of one dependency; a comparison that reached for every
/// entry instead would be quadratic in how wide a manifest is, and every
/// other test in the suite uses manifests narrow enough to hide it.
#[test]
fn four_times_the_dependencies_in_one_manifest_is_not_six_times_the_clock() {
    if !enabled("four_times_the_dependencies_in_one_manifest_is_not_six_times_the_clock") {
        return;
    }
    let one = fastest(2, 2_000);
    let four = fastest(2, 8_000);
    let ratio = four.as_secs_f64() / one.as_secs_f64().max(f64::EPSILON);
    eprintln!("budget: {one:?} for 2000 dependencies, {four:?} for 8000, ratio {ratio:.2}");
    assert!(
        ratio <= LINEARITY,
        "four times the dependencies cost {ratio:.2}× the time (limit {LINEARITY}×), seed {SEED:#x}"
    );
}
