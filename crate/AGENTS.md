# versions-le (CLI) — engineering standards

This is the source of truth for how code in `crate/` is written, tested
and reviewed. It applies to every contributor, human or AI-assisted.
[SPEC.md](SPEC.md) defines the product behavior — the checks, the
refusals, the exit codes; this file is how the code gets there. AGENTS.md
wins on any conflict.

## What this project is

Find the version constraints in a repository's manifests, and report
where the same dependency is constrained inconsistently — including
where two constraints cannot both be satisfied.

**This one has a verdict.** The extractor crates in this family report
what is there and hold no opinion; this answers a yes-or-no question and
the exit code is most of the product.

**Its value is cross-file.** A single manifest cannot contradict itself
in any interesting way. Everything here exists because the answer is
about the *set* of manifests — which is why the report is one object,
why discovery matters as much as parsing, and why the crate it most
resembles is `envsync-le`.

**Status: v0.1.0 core.** Five readers, six checks, three refusal
reasons, both surfaces, and the corpus that pins them.

## Layout

```
crate/src/
├── detect/       pure: grammars, readers, checks, the corpus.
│                 No filesystem, pub(crate).
│   ├── grammar.rs    constraint syntax → intervals; the disjointness test
│   ├── heuristics.rs what is a manifest, which ecosystem, is this a version
│   ├── parser.rs     one reader per manifest: text → entries + refusals
│   ├── compare.rs    the six checks, and cross_ecosystem
│   └── corpus.rs     fixtures/ embedded and run as unit tests
├── discover.rs   finding the manifests in a tree
├── scan.rs       one analysis end to end — the only path either surface calls
├── cli.rs        the terminal surface
└── mcp/          the agent surface
```

- **`detect/` touches no filesystem.** It takes document text and
  returns entries, so the whole decision layer tests from the corpus —
  no temp directories, no flake. A `std::fs` call appearing there is a
  bug.
- **`discover.rs` and `scan.rs` are the only modules allowed to touch
  the filesystem.**
- **Both surfaces are one implementation.** `cli.rs` and `mcp/` both
  call `scan.rs`. A surface that grows its own copy of a check is a bug,
  and a contract test asserts the two return identical reports for the
  same tree.
- **`grammar.rs` decides, `parser.rs` locates.** A reader's job is to
  find the constraint and say where it is; deciding what the characters
  mean belongs in one place. A reader that special-cases a syntax is a
  grammar change in the wrong file.
- Keep modules flat. No layers, registries, managers or services. No
  trait with a single implementation.

## Decisions already made (do not relitigate)

- **Refusal is the design, not the fallback.** A constraint in a grammar
  this tool does not model is named and excluded from comparison, never
  approximated. The whole `grammar-*` half of the corpus exists to fail
  the build if that ever changes.
- **`Unknown` and `Malformed` are different verdicts, and the difference
  is who is at fault.** `Unknown` is "this tool does not model that
  syntax". `Malformed` is "that is shaped like a constraint of its own
  ecosystem and is broken". The tool blames the manifest only when it is
  sure; when in doubt it blames itself. `latest` is `Unknown`; `^^1.0.0`
  is `Malformed`.
- **Comparison is scoped per ecosystem, always.** The one bridge,
  `msrv-mismatch`, relates two *named* keys — Cargo's `rust-version` and
  a workflow's Rust toolchain — and is not the generic comparator with
  its scoping relaxed. A second bridge is a spec change, not a patch.
- **Disjointness is interval arithmetic, not string comparison.** And
  prereleases widen a modelled range rather than narrow it, so the
  imprecision can only ever *hide* a disjointness. A change that made
  ranges narrower than the real ones could fabricate a
  `disjoint-constraint`, which is the one output this crate cannot
  afford.
- **A conflict needs two distinct ranges, not two distinct strings.**
  `>=20` and `>=20.0.0` are one requirement typed twice.
- **A Cargo `path` or `git` dependency that carries a `version` keeps
  it.** That version is a real registry requirement Cargo enforces on
  publish, and it is exactly the exact-patch pin a workspace uses to
  hold its own crates together. Only a table with no `version` at all
  has nothing to compare.
- **`rust-toolchain@<sha>` is an action pin, not a toolchain.** The SHA
  pins the action; the toolchain comes from the `toolchain:` input below
  it. Reading the SHA as a toolchain made a `malformed-constraint` out
  of the most careful thing a workflow can do.
- **The workflow reader scans lines, not YAML**, deliberately: a YAML
  parser would buy structure it then discards, at the cost of a
  dependency. The evidence gate (`looks_like_a_version`) is what keeps
  the shortcut honest.
- **`.github` is always descended.** A workflow lives in a hidden
  directory by definition, so `--hidden` controls the other hidden
  directories. Treating it the way `urls-le` or `paths-le` would makes
  the default find no CI pins at all.
- **`node_modules`, `vendor` and `.git` are never walked**, whatever the
  ignore rules say.
- **The report carries no timestamp**, and two runs over an unchanged
  tree are byte-identical. A report is a thing to diff.
- **stdout is protocol, stderr is human, and there is no `--json`
  flag.** One mode, nothing to misremember, and the human summary is a
  projection of the same report so the two cannot drift.
- **Nothing writes.** No `--fix`, no `--pin`, no `--update`, and no MCP
  argument that offers one. Tests assert the absence on both surfaces.
- **One crate, self-contained.** No published `-core`, no shared crate
  with the family.

## Control-flow style

Flat over nested, guards over branches — the same rules as pixelcoords,
pixelactions and the sibling LE crates:

- **No statement-position `else`.** Guard clauses and early `return`
  (`if !ok { return … }` / `let Some(x) = … else { return }`), then fall
  through to the happy path.
- **Value-position `if/else` is fine** — `let x = if cond { a } else
  { b }` is Rust's ternary.
- **`match` is fine and preferred** over any chain of condition tests on
  the same value; use match guards instead of `if/else` inside arms.
- Prefer combinators where they read cleanly: `bool::then_some`,
  `Option::map/filter/is_some_and`, `?`.
- No nesting deeper than two levels inside a function; extract a named
  helper instead.

## Hard rules

- **No inline `#[allow(...)]`.** Either fix the lint or add a visible,
  commented relaxation to `[lints.clippy]` in `Cargo.toml`. One is there
  already, with its reason.
- **Clippy pedantic, deny warnings.** `cargo clippy --all-targets --
  -D warnings` must pass exactly as CI runs it.
- **`unsafe` is forbidden crate-wide** (`[lints.rust]`).
- **No `anyhow` or `thiserror` in the library.** Errors are `String`,
  and every one of them names the file or the value it is about.
- **No async runtime.** This tool reads files and compares numbers.
- **No network, ever.** It does not know which versions exist, only
  whether two stated requirements can be met at once.
- **Dependencies are a cost.** Six is already more than most tools
  carry, and every one is justified by a comment in `Cargo.toml`.
  Justify any addition; prefer the standard library.
- **Strict parsing, never silent defaults** — for flags. An unrecognised
  flag or an unknown `--ecosystem` value is an error with an actionable
  message. A `--stict` that silently did nothing would report a clean
  audit that never ran the check asked for, and a `--ecosystem mavne`
  that silently widened the walk would report a clean audit of a scope
  nobody asked for.
- **Refuse rather than guess.** A grammar not modelled is a named
  refusal, never an approximated range. Never report coverage you did
  not achieve.
- **Refusals speak the caller's vocabulary.** An MCP caller has no
  command line; no message aimed at one mentions a flag, and a test
  asserts no MCP output contains `--`.

## The corpus contract

`fixtures/` lives inside the crate so the published package is
self-contained — `cargo package` cannot reach above its own directory —
and so `cargo test` on the unpacked tarball runs every case. The refusal
claims in the README are then checkable by whoever installed it rather
than taken on trust.

Three groups, and the last two are the point:

- **`tree-*`** is one synthetic repository carrying a planted instance
  of every finding this tool makes and every refusal reason.
- **`grammar-*`** is the opposite: manifest pairs a string-comparing
  tool would call a conflict, pinned here as **refusals**. A regression
  that started guessing at `workspace = true` turns a refusal into a
  fabricated finding, and `no_unmodelled_grammar_pairing_produces_a_conflict`
  is what fails.
- **`pin-*`** is the finding a real repository actually had. A Cargo
  `path` dependency carrying a bare `version = "0.7.7"` is a **caret**
  requirement — `[0.7.7, 0.8.0)` — and not the exact pin its author
  believed they had written; `=0.7.7` is the exact one. The pair is here
  because that one-character difference is the crate's most useful
  real-world finding and it must be impossible to regress.

**The coverage matrix lives here too**, in `corpus.rs`: every ecosystem,
every manifest kind, every finding code and every refusal reason has to
be reachable from a real document, and nothing in the code may emit a
code or a reason the corpus cannot produce. Both directions matter — a
seventh check added without a corpus case is a README claim no test
stands behind. The two `Ecosystem` and `ManifestKind` mappings are
exhaustive `match`es on purpose: a sixth reader with no document behind
it stops compiling.

Documents are stored **flat and dot-free** on disk and mapped to their
logical paths in `corpus.rs`: `cargo package` skips dotfiles, so a corpus
containing a real `.github/` directory would ship a crate that cannot run
its own tests. The logical path matters because classification reads it.

Changing a document or an expectation is a behavior change and needs a
CHANGELOG entry.

## Testing

- **`detect/` is pure and carries the corpus.** If something in it is
  hard to test, the design is wrong.
- **Exit codes belong in `tests/contracts.rs`.** They are the API —
  callers branch on them — so they are pinned by tests that drive the
  built binary against a real tree. **A new refusal adds its case
  there.**
- **Anything needing a tree larger than an editor opens is
  `tests/scenarios.rs`**, gated behind `VERSIONS_LE_SCENARIOS`. A
  skipped scenario says plainly that it did not run; it is never
  reported as a pass.
- **Four hardening suites, each aimed at a shape of bug a green suite
  let through somewhere in this family.** They build their trees at
  runtime, because half of what they need cannot be checked into git:
  - `tests/hazards.rs` — what a real machine holds and a fixture
    directory cannot: a byte-order mark, a manifest that is not UTF-8,
    a symlink loop, a FIFO named `Cargo.toml`, a mode-000 file, a path
    over 260 characters, an empty file, a 50 MB manifest, a workspace
    whose `members` point outside the tree. The property under all of
    them is that **a manifest never silently vanishes from the report**.
  - `tests/platform.rs` — every path the report uses as a manifest's
    identity is forward-slashed on every OS; plus case-folding
    filesystems, reserved Windows device names, CRLF manifests, and
    independence from `TZ`.
  - `tests/fuzz.rs` — time-boxed (`VERSIONS_LE_FUZZ_SECONDS`, one
    second locally, sixty in CI) and seeded (`VERSIONS_LE_FUZZ_SEED`,
    printed on every run). Hostile constraint strings through
    `compare_versions`, asserting a well-formed report and, above all,
    that **a grammar the tool does not model never becomes a conflict**.
    Adding a refusal adds its case to the pool there as well as to the
    corpus.
  - `tests/budget.rs` — gated behind `VERSIONS_LE_BUDGET`. A wall-clock
    ceiling, and linearity in both directions: four times the manifests,
    and four times the dependencies in one manifest. The second is the
    one this crate is exposed to, because disjointness is pairwise.
- **Every bug fix ships with a regression test** that fails before the
  fix. Three came out of the first dogfood run and each has one: a
  SHA-pinned `rust-toolchain` read as a malformed toolchain, `>=20` and
  `>=20.0.0` reported as a conflict, and an absolute file root labelled
  `//Users/…`.
- **Run the binary against a real repository, not only the tests.** All
  three of those were invisible to a green suite.
- Tests are deterministic: no clocks, no randomness, and no filesystem
  in `detect/`.

## Verification — the definition of done

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

All three, before every push. A change is not done because it compiles;
it is done when it is tested, linted, documented where behavior changed
(README / CHANGELOG / SPEC / this file), and honest — claims in docs
must match the code.
