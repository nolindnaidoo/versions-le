# versions-le — Rust specification

Find the version constraints in a repository's manifests, and report
where the same dependency is constrained inconsistently.

## The one question

**Do this repository's manifests agree about what version of anything it
depends on?**

Asked across every manifest in a tree rather than one file at a time,
answered with an exit code a CI step can fail on.

## This one has a verdict

The extractor tools in this family report what is there and hold no
opinion. This one answers a yes-or-no question and the exit code is most
of the product:

- **0** — nothing above `info`. Also 0 when there are no manifests at
  all: nothing can be in conflict with nothing, and failing a build over
  that would be the tool inventing a problem.
- **1** — findings.
- **2** — the question was malformed, or (with `--strict`) part of the
  tree went unanalysed.

## Refusal is the design, not the fallback

A validator that guesses is worse than one that stops. Every constraint
this tool cannot model **is named and excluded from comparison**, never
approximated into a range. The report carries a `refusals` array
alongside `findings` so a narrower answer can never be read as a clean
one.

| Reason | When | What happens |
|---|---|---|
| `unknown_grammar` | The value is a constraint, in a syntax this tool does not model: PEP 440 `~=`, `!=`, `===`, `==1.2.*`; npm `workspace:`, `npm:`, `file:`, `link:`, a git or https URL, an `owner/repo` shorthand, a dist tag; a Cargo dependency table with no `version` (`workspace = true`, a bare `path`, a bare `git`); a commit-SHA or branch action ref; a CI channel name (`stable`, `latest`, `lts/*`) | The entry stays in the report, named with the reason. **It takes part in no comparison.** |
| `cross_ecosystem` | The same name appears under two ecosystems | Named once, with a site in each. **The two are never compared.** |
| `ambiguous_version_string` | A `<tool>-version:` value in a workflow that is not evidently a version — `${{ matrix.node }}`, a list, a filename | No entry is created at all. There is nothing to compare and nothing was invented. |
| `per_job_tool_version` | One CI tool installed at two different versions — `python-version: 3.9` in the test job, `3.12` in the publish job | Named once, with every site. **The two are never compared.** |

`malformed-constraint` is a **finding**, not a refusal, and the
difference is deliberate: it is the narrower verdict that the value is
shaped like a constraint of its own ecosystem and is broken. The tool
blames the manifest only when it is sure; everything else it blames on
itself. `^^1.0.0` is malformed. `latest` is not — it is a syntax with a
meaning this tool chose not to model.

`--strict` turns `unknown_grammar` and `ambiguous_version_string` into
exit 2, for a pipeline that wants zero unanalysed corners.
`cross_ecosystem` and `per_job_tool_version` never trip it: those two are
not failures to answer, they **are** the answer.

### A CI tool version belongs to the job that installs it

A `<tool>-version:` input is what one job installs before it runs. Two
jobs at two versions is not a repository contradicting itself — testing
on the oldest interpreter a project supports and publishing on the
newest is correct, and so is a build step that needs an old toolchain.
The tool cannot know whether two jobs were meant to agree, so it names
what it saw and refuses, exactly where it used to report a conflict.

This is scoped as tightly as the reason justifies. An action `uses:` pin
is which version of a dependency the *repository* has chosen, so two
workflows disagreeing about `actions/checkout` is still a finding.
`packageManager` is Corepack's one-per-repository pin and is still
compared. `toolchain:` is still an MSRV claim and still bridges to
`rust-version`. Only the per-job tool version steps out of the
comparison.

Scoping the comparison per file would not have been enough: the two jobs
are as often in one workflow as in two, and the corpus case that pins
this is a single file for that reason.

## Per-ecosystem scoping — the settled rule

**Conflict analysis never crosses an ecosystem line.** Two reasons, and
either alone would be enough:

1. **The names are different namespaces.** An npm `semver` and a Cargo
   `semver` are unrelated packages that happen to share a word.
2. **The grammars disagree about the same characters.** A bare
   `"1.0.200"` means *exactly 1.0.200* in npm and *anything below 2.0.0*
   in Cargo. Comparing them would be comparing two different questions
   and reporting the answer to neither.

The five ecosystems are `npm`, `cargo`, `python`, `go` and `ci`.

**One bridge exists, and it is built by name, not by coincidence.**
`msrv-mismatch` relates Cargo's `rust-version` to a workflow's Rust
toolchain pin, because those two really do describe the same number. It
is a purpose-built check over two named keys — not the generic
name-matching comparator with its scoping switched off. Nothing else
crosses.

**Not bridged in v1, and visible as a refusal instead:** a
`packageManager: "bun@1.1.0"` and a workflow's `bun-version: 1.1.0`
describe the same tool, and this reports them as `cross_ecosystem`
rather than comparing them. That is the honest v1 answer — the tool says
it saw both and did not compare them — and the obvious next bridge.

## Manifests read in v1

| Manifest | Keys |
|---|---|
| `package.json` | `dependencies`, `devDependencies`, `peerDependencies`, `optionalDependencies`, `engines.*`, `packageManager` |
| `Cargo.toml` | `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`, the same three under `[workspace]` and under `[target.'…']`, `package.rust-version`, `workspace.package.rust-version` |
| `pyproject.toml` | PEP 621 `project.dependencies`, `project.optional-dependencies.*`, `project.requires-python` |
| `go.mod` | `require` (single and block form), the `go` directive |
| `.github/workflows/*.yml` | `uses:` action refs, `<tool>-version:` inputs, `toolchain:` |

The workflow test is on the **directory**, not the extension. A
repository is full of YAML that is not a workflow, and reading a
Kubernetes manifest for `uses:` lines would invent entries out of
nothing.

`node_modules`, `vendor` and `.git` are never walked, whatever the
ignore rules say: a `package.json` per installed package is the
resolver's output, not this repository's constraints.

### The workflow reader scans lines, not YAML

It wants two things — the `@ref` of a `uses:` and the value of a
`<tool>-version:` — and both are single-line in every workflow anyone
writes. A YAML parser would buy structure this then discards, at the
cost of a dependency. What keeps the shortcut honest is the evidence
gate: a value that is not evidently a version becomes an
`ambiguous_version_string` refusal instead of an invented entry.

`go.mod` is read the same way and for the same reason. Those two are the
readers that know a line number, and their sites are the only ones that
carry one — the structured readers address by key, and a `line` a reader
does not honestly know is worse than no `line` at all.

`dtolnay/rust-toolchain@1.88` puts the toolchain in the ref, so that ref
is read as an MSRV claim. `rust-toolchain@<sha>` is the hardened
spelling — the SHA pins the action and the toolchain comes from the
`toolchain:` input below it — so a SHA ref is read as an action pin.

## Checks in v1

| Code | Severity | What it means |
|---|---|---|
| `disjoint-constraint` | `error` | Two constraints for one dependency that **no single version satisfies**. The strongest claim this tool makes. |
| `malformed-constraint` | `error` | Shaped like a constraint of its ecosystem, and broken. |
| `constraint-conflict` | `warning` | One dependency, two or more **different requirements** across sites. |
| `msrv-mismatch` | `warning` | `rust-version` differs across manifests, or a CI toolchain pin is below the declared minimum. |
| `prerelease-in-production` | `warning` | A prerelease constraint outside dev or build dependencies. |
| `floating-pin` | `info` | `latest`, `*`, a caret on a `0.x` version, or an unpinned CI tool version. |

**The conflict trigger is two distinct modelled ranges, not two distinct
strings.** `>=20` and `>=20.0.0` are one requirement typed twice;
reporting the spelling as a conflict would spend the reader's attention
on something that cannot go wrong.

**One finding per drifted dependency**, carrying every site, rather than
one per pair — a dependency constrained four ways is one problem with
four sites, and four findings would read as four problems. Refusals are
grouped the same way.

### How disjointness is decided

Every modelled constraint becomes a union of intervals over a semantic
version. Two constraints are disjoint when no interval of one meets any
interval of the other. That is exact for the grammars modelled here,
which is why `disjoint-constraint` is a claim rather than a guess.

One deliberate imprecision, in the safe direction: a prerelease is
treated as an ordinary point on the line, so `^1` is modelled as
`[1.0.0, 2.0.0)` and therefore admits `2.0.0-rc.1`, which neither npm
nor Cargo would install. Modelled ranges are therefore very slightly
*wider* than the real ones — and a wider range can only hide a
disjointness, never invent one.

## Output contract

**stdout is protocol, stderr is human. There is no `--json` flag** — one
mode, nothing to misremember, and the human summary is a projection of
the same report so the two cannot drift.

One JSON report for the whole run, because the answer is about the
*set* of manifests.

```json
{
  "schema": 1,
  "manifests": [
    { "path": "api/Cargo.toml", "ecosystem": "cargo", "entries": 4 }
  ],
  "findings": [
    {
      "code": "disjoint-constraint",
      "severity": "error",
      "ecosystem": "cargo",
      "name": "regex",
      "message": "\"1\" (api/Cargo.toml) and \"2\" (web/Cargo.toml) cannot both be satisfied by one version",
      "sites": [
        { "file": "api/Cargo.toml", "key": "dependencies.regex", "constraint": "1" },
        { "file": "web/Cargo.toml", "key": "dependencies.regex", "constraint": "2" }
      ]
    }
  ],
  "refusals": [
    {
      "reason": "cross_ecosystem",
      "ecosystem": null,
      "name": "regex",
      "message": "appears in cargo and npm; different ecosystems name different things, so these were not compared",
      "sites": [ … ]
    }
  ],
  "diagnostics": [],
  "summary": {
    "manifests": 4, "entries": 20, "findings": 7, "refusals": 5,
    "errors": 2, "warnings": 4, "infos": 1
  }
}
```

`schema` is carried from day one so there is never a report a reader has
to sniff. `ecosystem` is `null` only on a `cross_ecosystem` refusal,
which by definition spans several. A site carries `line` only where the
reader honestly knows it.

### The report has no timestamp

A report is a thing to diff — against the previous run, against a
baseline in review — and a timestamp makes every run differ from every
other, which defeats the only reason to keep one. Two runs over an
unchanged tree produce byte-identical stdout, and a test asserts it.

## The CLI surface

```
usage: versions-le [options] <dir|file>...
       versions-le mcp
       versions-le --version | --help

Options:
  --ecosystem <name>   only npm, cargo, python, go or ci; repeatable
  --fail-on <what>     conflict (default: exit 1 on anything above info)
                       or any (exit 1 on any finding, floating pins too)
  --strict             a refusal or an unreadable manifest exits 2
  --exclude <glob>     skip manifests matching this pattern; repeatable
  --hidden             descend hidden directories too
  --no-ignore          walk files that .gitignore excludes
```

Several roots are allowed, because the question spans trees. When more
than one is named the labels are qualified with their root: two trees
each holding a top-level `Cargo.toml` would otherwise contribute two
sites with the same label, which reads as one file contradicting itself.

**`--hidden` does not control `.github`.** A workflow lives in a hidden
directory by definition, so that one is always descended; a tool that
needed a flag to see it would ship with the CI half switched off.

An exclude pattern that will not compile excludes **nothing**, rather
than everything — a typo in a config must not silently hide the
manifests this exists to compare.

## The MCP surface

- **`compare_versions`** — file contents in, findings out. Touches no
  filesystem. The `path` of each file decides which grammar its content
  is read with, so a path this tool does not recognise as a manifest is
  refused by name rather than sniffed from the content: a JSON document
  is not necessarily a `package.json`.
- **`versions_le_check`** — a directory in, the discovery and the same
  report the CLI writes.

Both return `{ ok, data, diagnostics, meta }`, where **`ok` reports
whether the check ran, not whether the answer is yes**. A tree full of
conflicting pins is the answer, not a failure to produce one.

**Arguments are parsed as strictly here as flags are on the command
line.** An argument of the wrong type and an argument the tool does not
take are both refusals naming what was wrong — `"hidden": "true"` is the
same mistake as `--stict`, and silently doing nothing would report a
clean audit of a check that never ran. An absent argument takes its
declared default, and an explicit `null` is read as absent: that is how
a client spells "not supplied", not a value of the wrong type.

Refusals speak the caller's vocabulary: an MCP caller has no command
line, and a test asserts no message on that surface contains `--`.

## Non-goals

- **It never edits a manifest.** No `--fix`, no `--pin`, no `--update`.
  The right version for a drifted dependency is a decision, not a
  derivation.
- **It never resolves a dependency graph.** It reads what the manifests
  *say*, not what a resolver would pick. There is no lockfile reading and
  no transitive analysis: the question is whether the stated constraints
  agree.
- **It never hits the network.** It does not know which versions exist,
  which are yanked, or which are newest — only whether two stated
  requirements can be met at once.
- **It does not lint style.** Ordering, quoting and formatting of a
  manifest are somebody else's job.

## Not in v1

- **Poetry, PDM and setuptools tables** (`[tool.poetry.dependencies]`
  and friends). They are not PEP 621 and carry their own grammar; a
  reader that guessed at them would be the thing this crate is against.
- **Lockfiles.** `Cargo.lock`, `package-lock.json` and `go.sum` are the
  resolver's answer, not the repository's claim.
- **Gradle, Maven, Gemfile, composer.json.**
- **A baseline file** for accepting known findings.
- **The `packageManager` ↔ CI tool-version bridge** described above.

## Files that cannot be read

Exit 2 means the *question* was malformed — an unknown flag, an unknown
ecosystem name, a path that does not exist. It does not mean one
manifest in fifty was unreadable.

A manifest that is not UTF-8 text, that cannot be opened, or that does
not parse is:

- named on stderr,
- carried in the report's `diagnostics` saying why,
- and left out of the findings.

The manifests that did parse still answer. `--strict` turns any such
diagnostic back into exit 2. What is never allowed is the third option:
a manifest that silently vanishes from the report, which reads to
whoever ran it as a manifest that was clean.

## The byte-order mark

A leading BOM is stripped before parsing. It is three invisible bytes
that Notepad, Excel and a PowerShell redirect all add, and before a `{`
it makes a JSON parser reject the whole document — which is
indistinguishable from a manifest with no dependencies in it.
