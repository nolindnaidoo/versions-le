# Instructions for AI coding assistants

Read [AGENTS.md](AGENTS.md) first — it is the engineering-standards
document for this crate and the source of truth for layout, control-flow
style, the settled decisions, testing requirements and the definition of
done. [SPEC.md](SPEC.md) defines the product behavior. AGENTS.md wins on
any conflict.

- Before declaring any change complete, run exactly what CI runs:
  `cargo fmt --all --check`,
  `cargo clippy --all-targets -- -D warnings`,
  `cargo test --locked`. All three must pass.
- Never add inline `#[allow(...)]`. Fix the lint, or add a commented
  relaxation to `[lints.clippy]` in `Cargo.toml`. One is there already,
  with its reason.
- New logic goes in `detect/` when it is pure (it must then be unit
  tested), and in `discover.rs` / `scan.rs` only when it needs the
  filesystem. A `std::fs` call in `detect/` is a bug.
- **Refusal is the design.** A constraint in a grammar this tool does
  not model is named in `refusals` and excluded from comparison, never
  approximated into a range. The `grammar-*` half of `fixtures/` exists
  to fail the build if that changes. Never manufacture a conflict out of
  a syntax you did not model.
- **`Unknown` blames the tool, `Malformed` blames the manifest.** When
  in doubt, blame the tool.
- **Comparison never crosses an ecosystem.** `msrv-mismatch` is the one
  bridge and it names both keys explicitly; adding a second is a spec
  change, not a patch.
- **A conflict needs two distinct modelled ranges, not two distinct
  strings.** `>=20` and `>=20.0.0` are one requirement typed twice.
- **The exit code is the product.** 0 clean, 1 findings, 2 malformed
  question. No manifests at all is 0 — do not "improve" that into a
  failure.
- **`--hidden` does not control `.github`.** A workflow lives in a
  hidden directory, so that one is always walked; making this flag
  behave like the sibling crates' would ship the CI half switched off.
- **Corpus documents are stored flat and dot-free**, mapped to logical
  paths in `corpus.rs`. `cargo package` skips dotfiles and a corpus of
  them ships a crate that cannot run its own tests.
- Write a regression test for every bug you fix. Three of the existing
  ones came from running the binary against a real repository, and all
  three were invisible to a green suite — so **run the binary, not only
  the tests**.
