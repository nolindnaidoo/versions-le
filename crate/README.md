<h1 align="center">versions-le</h1>

<p align="center">
  <b>Find where the same dependency is constrained differently across a repository's manifests</b><br/>
  <i>and refuse, loudly, on any grammar it cannot model</i>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rustc-1.88+-93450a.svg" alt="MSRV: Rust 1.88+" />
  <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" />
</p>

The build broke because `crates/api` asks for `serde = "1.0.200"` and
`crates/worker` asks for `serde = "2"`. Or it did not break, and will:
CI has run on `1.85` since March while `rust-version` says `1.88`.

```bash
versions-le .
```

```
error cargo: regex ("1" (api/Cargo.toml) and "2" (web/Cargo.toml) cannot both be satisfied by one version) [api/Cargo.toml, web/Cargo.toml]
warning ci: rust (CI builds on 1.80.0, below the declared minimum 1.88.0 in api/Cargo.toml) [.github/workflows/ci.yml, api/Cargo.toml]
refused unknown_grammar shared: an inherited workspace dependency carries its version elsewhere; excluded from comparison [web/Cargo.toml]
7 findings across 4 manifests — 2 error, 4 warning, 1 info
```

Exit code 1. The build stops before the deploy does.

## The exit code is the product

- **0** — nothing above `info`. Also 0 when there are no manifests at
  all: nothing can be in conflict with nothing.
- **1** — findings.
- **2** — the question was malformed, or `--strict` and part of the tree
  went unanalysed.

## What it will not do

**It never guesses.** A constraint in a grammar it does not model is
named in the report's `refusals` and takes part in no comparison:

```
refused unknown_grammar pkg: PEP 440 compatible-release (~=); excluded from comparison
refused cross_ecosystem semver: appears in cargo and npm; different ecosystems name different things, so these were not compared
refused ambiguous_version_string node: not evidently a version; excluded from comparison
```

That last one is `node-version: ${{ matrix.node }}`. A `<tool>-version:`
key in a workflow is a naming convention, not a schema, so a value that
is not evidently a version is refused rather than read.

**Comparison never crosses an ecosystem.** An npm `semver` and a Cargo
`semver` are unrelated packages that share a word — and a bare
`"1.0.200"` means *exactly 1.0.200* in npm and *anything below 2.0.0* in
Cargo. One bridge exists, `msrv-mismatch`, and it is built by naming
both keys rather than by matching a name.

**It never writes.** No `--fix`, no `--pin`. The right version for a
drifted dependency is a decision, not a derivation. It also never
resolves a dependency graph, reads a lockfile, or touches the network:
the question is whether the stated constraints agree, not what a
resolver would pick.

## Different, and unsatisfiable, are two findings

`constraint-conflict` is a smell: one dependency, more than one
requirement. `disjoint-constraint` is a build that cannot resolve: two
requirements **no single version satisfies**. The second is an `error`
and the first is a `warning`, and they are never conflated.

Disjointness is decided by interval arithmetic over the modelled ranges,
not by string comparison — which is also why `>=20` and `>=20.0.0` are
not reported as a conflict. They are one requirement typed twice.

## Checks

| Code | Severity | What it means |
|---|---|---|
| `disjoint-constraint` | error | Two constraints no single version satisfies |
| `malformed-constraint` | error | Shaped like a constraint of its ecosystem, and broken |
| `constraint-conflict` | warning | One dependency, two or more different requirements |
| `msrv-mismatch` | warning | `rust-version` differs, or CI builds below it |
| `prerelease-in-production` | warning | An `-rc` or `-alpha` outside dev dependencies |
| `floating-pin` | info | `latest`, `*`, a caret on `0.x`, an unpinned CI tool |

## What it reads

| Manifest | Keys |
|---|---|
| `package.json` | the four dependency sections, `engines`, `packageManager` |
| `Cargo.toml` | dependencies, dev, build, workspace and target variants, `rust-version` |
| `pyproject.toml` | PEP 621 `dependencies`, `optional-dependencies`, `requires-python` |
| `go.mod` | `require`, the `go` directive |
| `.github/workflows/*.yml` | `uses:` action pins, `<tool>-version:` inputs, `toolchain:` |

That last row is why this exists as much as the first: a CI toolchain
drifting away from the floor a manifest declares is exactly the failure
nobody notices until a release.

`node_modules`, `vendor` and `.git` are never walked. **`.github` always
is** — a workflow lives in a hidden directory by definition, so
`--hidden` controls the *other* hidden directories.

## Install

| Route | Command | Worth knowing |
|---|---|---|
| **cargo** | `cargo install versions-le` | Any platform, needs **Rust 1.88+**. |
| **From source** | `cd versions-le/crate && cargo build --release` | The same build CI runs. |

No runtime, no network, nothing written.

## Options

```
--ecosystem <name>   only npm, cargo, python, go or ci; repeatable
--fail-on <what>     conflict (default) or any
--strict             a refusal or an unreadable manifest exits 2
--exclude <glob>     skip manifests matching this pattern; repeatable
--hidden             descend hidden directories too
--no-ignore          walk files that .gitignore excludes
```

`--fail-on any` includes the `info` findings, for a repository that has
decided it wants no floating pins at all. `--strict` is for a pipeline
that wants no unanalysed corners: it turns `unknown_grammar` and
`ambiguous_version_string` into exit 2. `cross_ecosystem` never trips it
— that refusal is not a failure to answer, it *is* the answer.

**stdout is protocol, stderr is human, and there is no `--json` flag.**
One mode, nothing to misremember. The report carries `schema: 1` and no
timestamp, so two runs over an unchanged tree are byte-identical and the
report can be diffed against a baseline.

## As an MCP server

```bash
versions-le mcp
```

Two tools, both returning `{ ok, data, diagnostics, meta }`:

- **`compare_versions`** — manifest contents in, findings out. Touches
  no filesystem.
- **`versions_le_check`** — a directory in, the discovery and the same
  report the CLI writes.

`ok` reports whether the check ran, never whether the answer was yes.

## License

MIT — see [LICENSE](LICENSE).
