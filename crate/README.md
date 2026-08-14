<h1 align="center">versions-le</h1>

<p align="center">
  <b>Find where the same dependency is constrained differently across a repository's manifests</b><br/>
  <i>and refuse, loudly, on any grammar it cannot model</i>
</p>

<p align="center">
  <a href="https://crates.io/crates/versions-le">
    <img src="https://img.shields.io/crates/v/versions-le.svg" alt="versions-le on crates.io" />
  </a>
  <a href="https://crates.io/crates/versions-le">
    <img src="https://img.shields.io/crates/d/versions-le.svg" alt="crates.io downloads" />
  </a>
  <a href="https://github.com/nolindnaidoo/versions-le/actions/workflows/ci-crate.yml">
    <img src="https://github.com/nolindnaidoo/versions-le/actions/workflows/ci-crate.yml/badge.svg" alt="Build Status" />
  </a>
  <img src="https://img.shields.io/badge/rustc-1.88+-93450a.svg" alt="MSRV: Rust 1.88+" />
  <a href="https://github.com/nolindnaidoo/versions-le/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" />
  </a>
  <a href="https://letools.dev/tools/versions-le">
    <img src="https://img.shields.io/badge/web-letools.dev-00A0FF.svg" alt="letools.dev" />
  </a>
</p>

> **Useful?** A star is how other developers find it —
> [★ GitHub](https://github.com/nolindnaidoo/versions-le) ·
> [letools.dev/tools/versions-le](https://letools.dev/tools/versions-le)

The build broke because `api` asks for `regex = "1"` and `web` asks for
`regex = "2"`, and no one version satisfies both. Or it did not break,
and will: CI has run on `1.80` since March while `rust-version` says
`1.88`.

```bash
versions-le .
```

```
error cargo: regex ("1" (api/Cargo.toml) and "2" (web/Cargo.toml) cannot both be satisfied by one version) [api/Cargo.toml, web/Cargo.toml]
warning cargo: serde (constrained 2 different ways across 2 files: 1, 1.0.200) [api/Cargo.toml, web/Cargo.toml]
warning ci: rust (CI builds on 1.80.0, below the declared minimum 1.88.0 in api/Cargo.toml) [.github/workflows/ci.yml, api/Cargo.toml]
info ci: node (an unpinned tool version installs whatever is newest that day) [.github/workflows/ci.yml]
info npm: left-pad (a dist tag resolves to whatever is newest at install time) [package.json]
refused cross_ecosystem regex: appears in cargo and npm; different ecosystems name different things, so these were not compared [api/Cargo.toml, package.json]
refused unknown_grammar left-pad: a dist tag is a moving target, not a version range; excluded from comparison [package.json]
refused unknown_grammar node: a channel name is a moving target, not a version; excluded from comparison [.github/workflows/ci.yml]
refused unknown_grammar shared: an inherited workspace dependency carries its version elsewhere; excluded from comparison [web/Cargo.toml]
5 findings across 4 manifests — 1 error, 2 warning, 2 info
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
refused per_job_tool_version python: installed as 3.12 and 3.9 by different jobs; a CI tool version belongs to the job that installs it, so these were not compared
```

The third is `node-version: ${{ matrix.node }}`. A `<tool>-version:`
key in a workflow is a naming convention, not a schema, so a value that
is not evidently a version is refused rather than read.

The fourth is the one that matters most for a repository that has done
nothing wrong. Testing on the oldest interpreter a project supports and
publishing on the newest is correct, and two jobs are not two claims
about one requirement. An action `uses:` pin is a repository-wide choice
and is still compared; only the per-job tool version steps out.

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
| **crates.io** | `cargo install versions-le` | Any platform, needs **Rust 1.88+**. |
| **From source** | `cd versions-le/crate && cargo install --path .` | `cargo build --release` is the same build CI runs. |

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

`--exclude` takes ripgrep's glob syntax, from the same crate as the walk:
a pattern with no `/` matches the basename anywhere, one with a `/`
matches the root-relative path, `*` and `?` stop at a separator and `**`
crosses one. A pattern that will not compile excludes nothing rather than
everything, and the patterns beside it still work.

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

## Documentation

| What | Where |
|---|---|
| What this tool is allowed to say — scope, output contract, refusals, non-goals | [SPEC.md](https://github.com/nolindnaidoo/versions-le/blob/main/crate/SPEC.md) |
| How the code is written and held together — architecture, invariants, the gates | [AGENTS.md](https://github.com/nolindnaidoo/versions-le/blob/main/crate/AGENTS.md) |
| What changed | [CHANGELOG.md](https://github.com/nolindnaidoo/versions-le/blob/main/crate/CHANGELOG.md) |
| The tool's page, and the other fifteen | [letools.dev/tools/versions-le](https://letools.dev/tools/versions-le) |

## More from the LE family

Sixteen single-purpose tools for the work in front of every model. Each ships
a Rust CLI and an MCP server. One page: **[letools.dev](https://letools.dev)**

**Get it out**

- **[String-LE](https://letools.dev/tools/string-le)** — Extract every string in a codebase, with its position, so a person can read them
- **[Numbers-LE](https://letools.dev/tools/numbers-le)** — Extract every hardcoded number in a codebase, so a person can check them
- **[Units-LE](https://letools.dev/tools/units-le)** — Extract every quantity with its unit, normalized, and refuse the ambiguous ones by name
- **[Dates-LE](https://letools.dev/tools/dates-le)** — Extract every date and timestamp, and the exact instant each one resolves to
- **[IDs-LE](https://letools.dev/tools/ids-le)** — Extract every UUID, ULID, NanoID, ObjectId and Snowflake, and decode the time inside
- **[IPs-LE](https://letools.dev/tools/ips-le)** — Extract every IP address, CIDR block and MAC, normalized and classified by scope
- **[URLs-LE](https://letools.dev/tools/urls-le)** — Extract every URL in a codebase, with its protocol and exact position
- **[Paths-LE](https://letools.dev/tools/paths-le)** — Extract every file path in a codebase, and say whether it still points at anything
- **[Colors-LE](https://letools.dev/tools/colors-le)** — Extract every color in a codebase, and say which ones are not in your palette

**Check it**

- **[Regex-LE](https://letools.dev/tools/regex-le)** — Find every regex in a codebase, and report which can be driven into catastrophic backtracking
- **[Versions-LE](https://letools.dev/tools/versions-le)** — Find where one dependency is constrained differently across a repository's manifests
- **[i18n-LE](https://letools.dev/tools/i18n-le)** — Identify the i18n library a project uses, then audit its catalogs by that library's rules
- **[Scrape-LE](https://letools.dev/tools/scrape-le)** — Check whether a page is scrapeable before the scraper is written, and say when it cannot tell

**Guard it**

- **[Secrets-LE](https://letools.dev/tools/secrets-le)** — Find hardcoded credentials in a codebase, and never print one into the report
- **[EnvSync-LE](https://letools.dev/tools/envsync-le)** — Compare the dotenv files in a tree, and say which keys are missing from which
- **[Unicode-LE](https://letools.dev/tools/unicode-le)** — Find the Unicode that hides meaning — bidi controls, invisibles, homoglyphs, mixed scripts

Each stands on its own: no shared crate, no published core. Where two of them
agree, it is because the same answer was right twice.

**Contact** — [nolindnaidoo.com](https://nolindnaidoo.com) · [GitHub](https://github.com/nolindnaidoo) · [LinkedIn](https://www.linkedin.com/in/nolindnaidoo/)

## Also by nolindnaidoo

**Rust** — pixelcoords and pixelactions are one loop: pixelcoords answers
*where*, pixelactions *acts* there. Their own tools, their own voice — not
part of the LE family.

- **[pixelcoords](https://github.com/nolindnaidoo/pixelcoords)** — Freeze your screen, mark regions, get pixel-exact coordinates and crops
  [pixelcoords.dev](https://pixelcoords.dev) · [crates.io](https://crates.io/crates/pixelcoords) · [docs.rs](https://docs.rs/pixelcoords)
- **[pixelactions](https://github.com/nolindnaidoo/pixelactions)** — Consume human-verified coordinates, perform the interaction, confirm it landed
  [pixelactions.dev](https://pixelactions.dev) · [crates.io](https://crates.io/crates/pixelactions) · [docs.rs](https://docs.rs/pixelactions)

## License

MIT — see [LICENSE](LICENSE).
