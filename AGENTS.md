# AGENTS.md — versions-le

Technical source of truth for this repository. [README.md](README.md) is
the user-facing page; this file is for anyone, human or agent, changing
the code.

**This repository is crate-only.** There is one product in it — the Rust
CLI and MCP server in [`crate/`](crate/) — and everything about how it is
written, tested and reviewed lives there:

| Question | File |
|---|---|
| How is code in this repo written? | [`crate/AGENTS.md`](crate/AGENTS.md) — layout, control-flow style, hard rules, the corpus contract, the definition of done |
| What is the product supposed to do? | [`crate/SPEC.md`](crate/SPEC.md) — the checks, the refusals, the exit codes, the output contract |
| What does a user see? | [README.md](README.md) at the root, [`crate/README.md`](crate/README.md) for the published crate |
| What changed? | [CHANGELOG.md](CHANGELOG.md) at the root, [`crate/CHANGELOG.md`](crate/CHANGELOG.md) for the crate |

`crate/AGENTS.md` wins on any conflict with this file. It is the standard;
this one is a router with the repository-level facts that do not belong
inside the crate.

## The one-line version of the product

Find the version constraints in a repository's manifests, and report
where the same dependency is constrained inconsistently — including where
two constraints cannot both be satisfied.

**This one has a verdict.** The extractor tools in this family report
what is there and hold no opinion; this answers a yes-or-no question and
the exit code is most of the product. **Its value is cross-file**: a
single manifest cannot contradict itself in any interesting way.

## Non-negotiables

These are the crate's rules, restated because they are the ones most
easily lost in a hurry. The reasoning for each is in
[`crate/AGENTS.md`](crate/AGENTS.md).

- **Refusal is the design, not the fallback.** A constraint in a grammar
  this tool does not model is named in `refusals` and excluded from
  comparison, never approximated into a range. A fabricated
  `disjoint-constraint` against somebody's real manifest is the one
  output this crate cannot afford.
- **`Unknown` blames the tool, `Malformed` blames the manifest.** When in
  doubt, blame the tool.
- **Comparison never crosses an ecosystem.** `msrv-mismatch` is the one
  bridge and it names both keys explicitly. A second bridge is a spec
  change, not a patch.
- **A conflict needs two distinct modelled ranges, not two distinct
  strings.**
- **The exit code is the product.** 0 clean, 1 findings, 2 malformed
  question. No manifests at all is 0 — do not "improve" that into a
  failure.
- **Nothing writes.** No `--fix`, no `--pin`, no `--update`, and no MCP
  argument that offers one.
- **`detect/` never touches the filesystem.** A `std::fs` call there is a
  bug; that split is what lets the whole decision layer test from the
  corpus with no temporary directories and no flake.
- **Never report coverage you did not achieve.** A skipped case says so
  by name; a skip is never reported as a pass.
- **Comments explain *why*, never what.** A change should be
  indistinguishable from the code around it.
- **Commits are conventional** (`fix:`, `feat:`, `docs:`, `test:`, `ci:`…),
  imperative, and enforced by the hook in `.githooks/commit-msg`.

## Before you commit

```bash
cd crate
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

All three. A change is not done because it compiles; it is done when it
is tested, linted, documented where behaviour changed, and honest —
claims in the README, `SPEC.md` and the help text must match the code.

The coverage floor (90% line coverage **per module** across `detect/`) is
a floor and is never lowered to make a build pass.

## CI

[`.github/workflows/ci-crate.yml`](.github/workflows/ci-crate.yml) runs
the gates on three operating systems, plus `msrv`, `policy` (no inline
`#[allow]`), `coverage`, `audit`, and five jobs that each exist because
something real got through a green suite: `hazards`, `platform`, `fuzz`,
`budget` and `coverage-matrix`. Every one of them carries a comment
naming the bug it catches — keep that discipline when adding another.

`codeql.yml`, `dependabot-auto-merge.yml` and `release-crate.yml` are
byte-identical across the sibling repositories in this family. Changing
one of them here means changing it everywhere.

## Docs that must stay true together

Behaviour is described in four places and they are checked against each
other by tests, not by hope:

- the help text in `crate/src/cli.rs` — a unit test asserts every
  documented flag is parsed and every parsed flag is documented;
- `crate/SPEC.md` — the behavioural contract;
- `crate/fixtures/` — the corpus, run as tests, which is where a claim
  about a refusal becomes checkable by whoever installed the crate;
- the two READMEs and the two CHANGELOGs.

Changing a corpus document or an expectation is a behaviour change and
needs a CHANGELOG entry.
