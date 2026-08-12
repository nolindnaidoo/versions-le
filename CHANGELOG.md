# Changelog

All notable changes to versions-le are documented here. This repository
is crate-only, so this file tracks the repository as a whole;
[`crate/CHANGELOG.md`](crate/CHANGELOG.md) is the one that ships with the
package and describes the tool's behaviour.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-12

First release. The Rust CLI and MCP server in [`crate/`](crate/): five
manifest readers, six checks, four refusal reasons, and both surfaces.
Not published to crates.io yet — build from source.

### Added

- **The tool.** Reads `package.json`, `Cargo.toml`, `pyproject.toml`,
  `go.mod` and `.github/workflows/*.yml`, and reports where the same
  dependency is constrained inconsistently across them — including where
  two constraints cannot both be satisfied. One JSON report on stdout, a
  human summary on stderr, and an exit code a CI step can fail on: 0
  clean, 1 findings, 2 malformed question. Full detail in
  [`crate/CHANGELOG.md`](crate/CHANGELOG.md) and
  [`crate/SPEC.md`](crate/SPEC.md).
- **The MCP server** (`versions-le mcp`) with `compare_versions`
  (contents in, no filesystem) and `versions_le_check` (a directory in).
- **Repository documentation** — this file, [README.md](README.md),
  [AGENTS.md](AGENTS.md), [CLAUDE.md](CLAUDE.md), [GEMINI.md](GEMINI.md)
  and [LICENSE](LICENSE). The root files are routers; the crate's own
  `AGENTS.md` and `SPEC.md` remain the source of truth.
- **Four hardening suites**, each carrying the shape of bug it exists to
  catch:
  - `crate/tests/hazards.rs` — a byte-order mark on every manifest kind,
    a manifest that is not UTF-8, a document that parses but is not a
    manifest, symlinks and symlink loops, a FIFO named `Cargo.toml`,
    permission denied, a path over 260 characters, an empty file, a
    50 MB manifest, and a workspace whose `members` point outside the
    tree. Built at runtime; every case a platform cannot express skips
    by name.
  - `crate/tests/platform.rs` — every path the report uses as a
    manifest's identity is forward-slashed on every OS, plus
    case-folding filesystems, reserved Windows device names, CRLF
    manifests, and independence from `TZ`.
  - `crate/tests/fuzz.rs` — time-boxed and seeded, over generated
    constraint strings: enormous prerelease chains, hundreds of `||`
    alternatives, thousands of intersected comparators, unicode
    versions, and every grammar the tool declines to model. Never
    panics, never hangs, always a well-formed report, and **never
    fabricates a conflict out of a grammar it does not model**.
  - `crate/tests/budget.rs` — a wall-clock ceiling on a seeded corpus
    plus two linearity checks: four times the manifests, and four times
    the dependencies in one manifest.
- **A coverage matrix** in `crate/src/detect/corpus.rs`: every
  ecosystem, every manifest kind, every finding code and every refusal
  reason reachable from a real fixture — and nothing in the code that
  the corpus cannot produce. It prints a marker line and CI greps for
  it, because `cargo test <filter>` exits 0 when the filter matches
  nothing.
- **CI jobs** for the five above, on the three-OS matrix where the
  platform is the point — plus one for `crate/tests/scenarios.rs`, which
  had none.

### Fixed

- **Report paths carried a Windows path prefix verbatim**, so a manifest
  named as its own root was labelled
  `\\?\C:/Users/runneradmin/…/Cargo.toml`. `canonicalize` returns an
  extended-length path on Windows and nothing turned it back, which also
  gave one file two identities depending on whether the caller had
  canonicalized. Fixed in `discover::normalise`, and the prefix decision
  is now an exhaustive `match` so it cannot be dropped again on the two
  platforms that never see a prefix.
- The crate's install instructions claimed `cargo install versions-le`.
  It is not on crates.io yet, and a README may not say it is.
- `crate/tests/scenarios.rs` expected 500 refusal rows from 500 crates
  inheriting one workspace dependency, from before refusals for one name
  merged into a single row carrying every site. Nothing set
  `VERSIONS_LE_SCENARIOS`, so the suite had never run and the stale
  expectation was invisible. It now asserts the merge — one row, 500
  sites — and CI runs it.

### The finding that came out of dogfooding

Run against a real workspace, the tool reported
`core = { path = "../core", version = "0.7.7" }` as a **`floating-pin`**.
That reading is right and it is the point of the crate: beside a `path`,
a bare `version` is still a *caret* requirement in Cargo —
`[0.7.7, 0.8.0)` — and not the exact pin the workspace's own
documentation claimed it had. Only `=0.7.7` is that pin. The corpus now
carries the pair as `pin-cargo-path-caret.toml` and
`pin-cargo-path-exact.toml`, with a unit test in the reader, one in the
grammar, and one driving the built binary, so the one-character
difference cannot collapse.
