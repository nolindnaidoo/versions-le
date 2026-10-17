# versions-le

Find where one dependency is constrained inconsistently across a
repository: `package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod` and
the actions and tool versions in `.github/workflows`.

```bash
npx versions-le
npx versions-le --ecosystem npm --exclude 'examples/**' . > versions.json
npx versions-le --fail-on any packages/
```

The command it installs is `versions-le`, the same command line as the Rust
crate [`versions-le`](https://crates.io/crates/versions-le), and it behaves
identically: same flags, same walk, same findings, same output, same exit
codes. Install whichever toolchain you already have.

## What it reports

Two constraints no single version can satisfy, the same dependency
constrained different ways, `rust-version` declarations that disagree or
that CI never builds on, a prerelease in production, a malformed constraint and a
floating pin. Each finding names every file involved.

**Comparison never crosses an ecosystem**: an npm `semver` and a Cargo
`semver` are unrelated packages that share a word. A constraint in a
grammar this does not model, such as PEP 440's `~=`, a git or path
dependency, or a commit-SHA action pin, is named in the report's refusals
and left out of the comparison, never guessed at.

The walk respects `.gitignore` and `.ignore` unless you pass `--no-ignore`.
It always reads `.github`, descends other hidden directories only under
`--hidden`, and never descends `node_modules`, `.git` or `vendor`.
`--exclude` takes globset's glob syntax: a pattern with a `/` matches the
path below the root, any other matches the file name.

## Output

stdout carries one JSON report whenever it is piped or redirected, and
nothing when it is a terminal. stderr is the summary.

```text
$ versions-le
error npm: react ("^17.0.2" (api/package.json) and "^18.2.0" (web/package.json) cannot both be satisfied by one version) [api/package.json, web/package.json]
warning cargo: regex (constrained 2 different ways across 2 files: 1, 1.10) [Cargo.toml, core/Cargo.toml]
refused unknown_grammar local: a path dependency with no version has no registry requirement; excluded from comparison [core/Cargo.toml]
2 findings across 4 manifests — 1 error, 1 warning, 0 info
```

## Exit codes

- **0**: consistent, or no manifests at all.
- **1**: findings. Only those above `info` fail the run, unless
  `--fail-on any` asks for every one.
- **2**: the question was malformed, a directory could not be walked, or
  `--strict` and a refusal or an unreadable manifest.

## For agents

`versions-le mcp` serves the Model Context Protocol on stdio with two
tools: `compare_versions` for manifest contents it is handed and
`versions_le_check` for a directory. Hosts that want the first alone can
install [`versions-le-mcp`](https://www.npmjs.com/package/versions-le-mcp).

`versions-le --help` lists every option.

MIT licensed. Part of [letools](https://letools.dev/tools/versions-le).
