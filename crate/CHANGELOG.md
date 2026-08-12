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
