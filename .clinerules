# Contributor and agent instructions

**Read [AGENTS.md](AGENTS.md) before writing any code.** It carries the
engineering standard this repository is held to — control flow, error handling,
module shape — plus the architecture, the invariants and why each one exists.
[CLAUDE.md](CLAUDE.md) is the short version: gates and traps.

This file exists only to route you there. It is deliberately thin: the standard
lives in one place so it cannot drift between tools.

## Non-negotiables

- Guard clauses first. **No statement-position `else`** — two branches are an
  early return, many are a `match` or a lookup table. Value-position `if/else`
  is fine.
- Nesting stops at two levels inside a function.
- **`Result<T, String>` for fallible functions.** No `anyhow`, no `thiserror`
  in the library; one error enum only where a domain genuinely needs it.
- `#![forbid(unsafe_code)]`, crate-wide, no platform exemption.
- **No inline `#[allow(...)]` anywhere.** CI greps for it. A lint you mean to
  relax goes in `[lints.clippy]` in `crate/Cargo.toml` with a comment saying
  why.
- Flat modules. No layers, registries, managers or services, and no trait with
  a single implementation.
- **Refuse rather than guess.** Ambiguous input returns a named refusal reason,
  never a plausible answer. A test that passes by normalizing something that
  should have been refused is the bug this whole family exists to prevent.
- **stdout is protocol, stderr is human.** There is no `--json` flag, and exit
  codes are part of the API.
- Never report success you did not achieve.
- Comments explain **why**, never what.
- Commits are conventional (`fix:`, `feat:`, `docs:`…), imperative, and
  enforced by a hook and by CI.

## Before you commit

```bash
cd crate && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --locked
```

Coverage thresholds are a floor and are never lowered to make a build pass.
Every claim in a README or in help text must be provable against the code.
