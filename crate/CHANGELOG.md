# Changelog

The Rust CLI and MCP server.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-12

First release. Core functionality: five manifest readers, six checks,
three refusal reasons, and both surfaces.

### Added

- **Manifest readers** for `package.json`, `Cargo.toml`,
  `pyproject.toml`, `go.mod` and `.github/workflows/*.yml`. Every one
  skips a key it does not model rather than failing the file — an
  unrecognised key is not a parse error, it is a key that is not a
  dependency constraint.
- **The constraint grammars**: Cargo (via the `semver` crate's
  comparators), npm ranges (`^`, `~`, `x`, hyphen ranges, `||`,
  space-separated intersections), the PEP 440 comparison clauses, go
  module versions, action refs and CI tool versions. Each becomes a
  union of intervals, so disjointness is decided by interval arithmetic
  rather than string comparison.
- **Six checks**: `disjoint-constraint` and `malformed-constraint`
  (error), `constraint-conflict`, `msrv-mismatch` and
  `prerelease-in-production` (warning), `floating-pin` (info). Findings
  carry `severity` from day one.
- **Three refusal reasons** — `unknown_grammar`, `cross_ecosystem` and
  `ambiguous_version_string` — reported alongside the findings so a
  narrower answer can never be read as a clean one.
- **The CLI**: one JSON report on stdout, a human summary on stderr, and
  exit codes — 0 clean, 1 findings, 2 malformed question. Several roots,
  an `--ecosystem` filter, `--fail-on conflict|any` and `--strict`.
- **The MCP server** (`versions-le mcp`) with `compare_versions`
  (contents in, no filesystem) and `versions_le_check` (a directory in).
- **A third corpus group, `pin-*`**, carrying the finding that came out
  of running the binary against a real workspace: a Cargo `path`
  dependency whose bare `version = "0.7.7"` is a **caret** requirement —
  `[0.7.7, 0.8.0)` — and not the exact pin its author believed they had
  written. `=0.7.7` is the exact one. The pair is pinned as a
  `floating-pin` and a silence respectively, plus a `constraint-conflict`
  when the two are compared, so the one-character difference cannot
  collapse into "both are pins".
- **Four hardening suites** — `tests/hazards.rs` (a byte-order mark on
  every manifest kind, a manifest that is not UTF-8, a document that
  parses but is not a manifest, symlinks and symlink loops, a FIFO named
  `Cargo.toml`, permission denied, a path over 260 characters, an empty
  file, a 50 MB manifest, a workspace pointing outside its own tree),
  `tests/platform.rs` (every path the report uses as an identity is
  forward-slashed on every OS; case folding, reserved Windows names,
  CRLF manifests, `TZ`), `tests/fuzz.rs` (time-boxed and seeded over
  generated constraint strings, asserting a well-formed report and that
  a grammar the tool does not model never becomes a conflict) and
  `tests/budget.rs` (a wall-clock ceiling, and linearity in both the
  number of manifests and the width of one).
- **A coverage matrix** in `detect/corpus.rs`: every ecosystem, every
  manifest kind, every finding code and every refusal reason reachable
  from a real fixture, and nothing in the code that the corpus cannot
  produce.

### Fixed

- **A range with an unbounded alternative reported the wrong floor.**
  `Range::floor` answers "how old a toolchain does this permit" and the
  MSRV check believes it, but it took the lowest bound anybody had
  written down and ignored the alternative that had none: a CI pin of
  `<1.80 || >=1.90` came back as 1.90 while admitting every version below
  1.80, so the check cleared a toolchain that was not pinned at all. A
  union with an alternative that is unbounded below now has no floor, and
  the check skips what it cannot bound rather than believing a number.
  Reachable only through a `||` union in a workflow's tool version, which
  is why no corpus document had one — the unit test does.
- **Report paths carried a Windows path prefix verbatim.** On Windows
  `std::fs::canonicalize` returns an extended-length path, so a manifest
  named as its own root was labelled
  `\\?\C:/Users/runneradmin/…/Cargo.toml` — backslashes and a verbatim
  marker in the one string the report promises has neither, and two
  identities for one file depending on whether the caller had
  canonicalized. `normalise` now turns a prefix into a designator:
  every disk prefix collapses to an upper-cased `C:`, a UNC prefix keeps
  its host and share as `//server/share`, and `\\?\C:\a` and `C:\a`
  produce the same label. Red on the Windows CI leg only, because no
  other platform parses a path prefix at all — and now covered on all
  three legs: the designator shapes are unit tested against literal
  `Prefix` values, which are constructible everywhere, and the
  reassembly and the end-to-end label are asserted where a prefix is
  actually parsed.
- The prefix decision is now an **exhaustive `match` on `Component`**,
  which compiles on every platform — so dropping the prefix arm is a
  build failure everywhere rather than a Windows-only test failure.
- `scan::manifest_of` walked `components()` itself and joined a prefix
  and its root separator into `\\?\C:/\/a/…`. Classification was
  unaffected — it reads only the basename and `/.github/workflows/` —
  but the duplicate is gone and both callers now share one spelling.
- `tests/scenarios.rs` expected 500 refusal rows from 500 crates
  inheriting one workspace dependency, from before refusals for one name
  merged into a single row carrying every site. Nothing set
  `VERSIONS_LE_SCENARIOS`, so the suite had never run. It now asserts the
  merge — one row, 500 sites — and CI runs it.
- The install instructions claimed `cargo install versions-le`. The
  crate is not on crates.io yet, and a README may not say it is.

### The shape of it

**Refusal is the design, not the fallback.** A constraint in a grammar
this tool does not model is named and excluded from comparison, never
approximated into a range. The corpus is built around it: `fixtures/`
carries a manifest pair per unsupported grammar, each pinned to produce
a **refusal and no conflict**, so a regression that started guessing
turns a refusal into a fabricated finding and fails the build.

**Comparison never crosses an ecosystem.** An npm `semver` and a Cargo
`semver` are unrelated packages that share a word, and a bare
`"1.0.200"` means different things in the two grammars. The one bridge
is `msrv-mismatch`, and it relates two *named* keys rather than matching
a name.

**The report carries no timestamp.** Two runs over an unchanged tree
produce byte-identical stdout, so the report can be diffed against a
baseline. A `schema` field ships from the first release so no reader has
to sniff.

**The conflict trigger is two distinct modelled ranges, not two distinct
strings.** `>=20` and `>=20.0.0` are one requirement typed twice, and
reporting the spelling would spend the reader's attention on something
that cannot go wrong.

**`.github` is always walked.** A workflow lives in a hidden directory by
definition, so `--hidden` controls the other hidden directories.
Treating it the way a sibling tool does would ship the CI half switched
off.

**`node_modules`, `vendor` and `.git` are never walked**, whatever the
ignore rules say: a `package.json` per installed package is the
resolver's output, not this repository's constraints.
