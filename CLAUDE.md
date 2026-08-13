# CLAUDE.md

[AGENTS.md](AGENTS.md) is the technical source of truth for this repo, and it
is mostly a router: **this repository is crate-only**, so the standard the code
is held to — layout, control flow, error handling, testing, the definition of
done — lives in [`crate/AGENTS.md`](crate/AGENTS.md), and the product behaviour
lives in [`crate/SPEC.md`](crate/SPEC.md). Read both before writing code.
README.md is user-facing.

## Where to look

| Question | File |
|---|---|
| How should this code be written? | [`crate/AGENTS.md`](crate/AGENTS.md) — the standard, the layout, and the decisions already made |
| What is the tool supposed to do? | [`crate/SPEC.md`](crate/SPEC.md) — checks, refusals, exit codes, output contract |
| What does the user see? | [README.md](README.md) · [`crate/README.md`](crate/README.md) |
| What changed? | [CHANGELOG.md](CHANGELOG.md) · [`crate/CHANGELOG.md`](crate/CHANGELOG.md) |

## Gates

```bash
cd crate
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --locked
```

The timed and gated suites are opt-in and CI sets them:
`VERSIONS_LE_BUDGET=1`, `VERSIONS_LE_FUZZ_SECONDS=60`,
`VERSIONS_LE_SCENARIOS=1`.

## Things that will bite you

- **Refusal is the design, not the fallback.** A grammar the tool does not
  model is named in `refusals` and compared with nothing. Never approximate one
  into a range — a fabricated `disjoint-constraint` against a real manifest is
  the one output this crate cannot afford, and `tests/fuzz.rs` plus the
  `grammar-*` corpus exist to fail the build if that changes.
- **`Unknown` blames the tool, `Malformed` blames the manifest.** When in
  doubt, blame the tool.
- **A conflict needs two distinct modelled *ranges*, not two distinct
  strings.** `>=20` and `>=20.0.0` are one requirement typed twice.
- **`detect/` touches no filesystem.** New pure logic goes there and is unit
  tested against the corpus; only `discover.rs` and `scan.rs` may read a file.
- **Never add an inline `#[allow(...)]`** — a CI job greps for it. Fix the lint
  or add a commented relaxation to `[lints.clippy]` in `crate/Cargo.toml`.
- **Report paths are always forward-slashed**, on every OS, **prefix and all**.
  A sibling shipped a release that used `\` on Windows; this one shipped a
  label of `\\?\C:/Users/…` because `canonicalize` returns an extended-length
  path there and `normalise` only special-cased separators. The `match` on
  `Component` in `discover.rs` is exhaustive so that arm cannot be dropped
  again — a wildcard there puts the bug straight back.
- **Corpus documents are stored flat and dot-free** and mapped to logical paths
  in `detect/corpus.rs`. `cargo package` skips dotfiles, and a corpus of them
  ships a crate that cannot run its own tests.
- **Changing a corpus document or an expectation is a behaviour change** and
  needs a CHANGELOG entry.
- **CI narrows itself on a docs-only push.** `ci-crate.yml` fires on `*.md` and
  the agent instruction files — it has to, because the `policy` job greps them,
  and the filter used to admit only `crate/**` so that gate could run only when
  the files it guards had *not* been touched. On a docs-only push `policy` and
  `commits` run and every Rust job skips. Anything unrecognised, and an
  unreadable diff, counts as code and runs everything.
- **Coverage floors are a backstop, not a target.** They sit well below where
  the code actually is, and they are not raised to track it — a floor that
  follows real coverage becomes a tax on writing the next module.
- **Every claim must be provable.** Nothing goes in a README, `SPEC.md` or the
  help text unless the code backs it. That governs **behaviour and numbers** —
  not **availability**. `versions-le` is not on crates.io yet, and an install
  line written ahead of that publish is **staged, not forbidden**: write it,
  and let the release commit be what makes it true. What must not happen is
  the page *reading as live* while the registry disagrees — which is why the
  publish and the line that announces it belong in one commit.
- **Run the binary, not only the tests.** Three of the crate's regression tests
  came from running it against a real repository, and all three were invisible
  to a green suite.
