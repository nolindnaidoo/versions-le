# Changelog

All notable changes to Versions-LE will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

This file covers the **VS Code extension**, the npm MCP server and the
npm command line. The Rust CLI in `crate/` keeps its own
[CHANGELOG](crate/CHANGELOG.md). From 1.2.0 every surface carries one
version and releases from one tag. The entries below 1.0.0 describe this
repository while it held the CLI alone.

## [Unreleased]

### Fixed

- When the editor refused to open the report, the command still flashed the
  finding count in the status bar and announced the version problems in a
  toast, describing a report nobody was handed. It announces the failure to
  open and nothing more.
- A status bar flash after the extension was deactivated, from a command
  that finished late, wrote to the disposed item and set a restore timer
  that outlived the extension. It does nothing.
- The MCP server held every byte of a frame until its newline, with no
  ceiling, so a client that streamed without one grew the process without
  bound and got no answer. A frame over 16 MiB is refused with a parse error
  and the next line is served.
- A failure inside a command, such as the editor refusing to open a document
  or a dialog going away, escaped as an unhandled rejection the editor
  reported in its own words, home directory included. Every command reports
  it as one redacted, localized error.
- The crate release workflow read a crates.io answer it could not parse, or
  a failed request, as "unpublished" and would have let the publish run. It
  refuses to publish without a version list.

## [1.2.2] - 2026-10-18

### Changed

- **The listing carries the words people search for.** Open VSX ranks a
  match in the name five times a match in the description, and "Versions-LE"
  holds neither "dependency" nor "version", so a search for "version
  mismatch" put it 69th on 2026-10-18. The name is "Versions-LE: Dependency
  Version Checker", the description opens with "Dependency version checker",
  where a card has room for about forty characters, and the keywords are cut
  from 17 to 10 phrases someone would search for.

## [1.2.1] - 2026-10-17

### Fixed

- **`mcp` no longer ends the session on a frame that is not UTF-8**, in
  the crate or the npm package. Such a frame is dropped like one that is not
  JSON, and the requests after it are served. The npm server reads frames
  with `src/cli/frames.ts`, the crate server's framing in one file shared
  across the family, and `check:cli-pipes` now feeds both servers eighteen
  malformed frames and requests and compares the answers.

## [1.2.0] - 2026-10-17

### Changed

- **One version for every surface.** The extension, the MCP server on
  npm, the new npm command line and the Rust crate all release as 1.2.0
  from the tag `v1.2.0`. The crate jumps from 0.1.5 to join them, and CI
  now fails a commit where any of them disagree.

### Added

- An npm command line, `versions-le`, built in `cli/` from `src/cli/`. It
  is the Rust CLI's surface over this extension's detection engine: the
  same flags, walk, `--exclude` globs, findings, refusals, exit codes and
  JSON report, and `versions-le mcp` serves the same two tools with the
  same bytes. The exclusions are globset's, transcribed and checked
  against globset itself. `scripts/check-cli-parity.ts` runs both binaries
  over one tree on three operating systems and fails on any byte of
  difference, and `scripts/check-cli-pipes.ts` does the same with stdout
  or stderr closed early. It is published as `versions-le` on npm, beside
  `versions-le-mcp`.

### Fixed

- The npm MCP server read `"arguments": null` as an empty object, where
  the crate's server refuses it as not being an object. The shared
  `compare_versions` tool now answers both the same way.

### Fixed

- The rating prompt's Rate button opened the Marketplace review page for
  every install, including one from Open VSX in VSCodium, Cursor or
  Windsurf, because it told the registries apart by a publisher name the
  Open VSX listing does not carry. It reads the editor's name instead, and
  an Open VSX install rates on Open VSX.

## [1.1.0] - 2026-10-07

### Added

- A rating prompt. On the 3rd successful use, one notification asks whether
  you would rate the extension, with *Rate*, *Later* and *Don't Ask Again*.
  *Rate* opens the registry your copy was installed from: the VS Code
  Marketplace or Open VSX. *Later*, or dismissing it, asks once more on the
  20th use; that second ask is the last. It never appears on activation or
  after a failed run, and it is never shown if you have set
  `notificationsLevel` to `important` or `silent` yourself. The answer
  follows you through Settings Sync. Translated into all 12 locales.
- Positions are now a setting. `versions-le.showPositions` decides whether the
  output gives the line each constraint is written on, and
  `versions-le.clipboardIncludesPositions` decides the same for the copy on
  the clipboard. Both are on by default, so the output is what it was.

### Changed

- No command is bound to a key by default any more. The one default this
  extension shipped sat on a key the editor, the system or another LE
  extension already used. Every command can still be given a key under
  Keyboard Shortcuts.

## [1.0.1] - 2026-10-04

### Fixed

- The Open VSX links and the Open VSX downloads badge in the README and the
  npm README pointed at a namespace the listing has left, so they led nowhere.
  The listing is under `nolindnaidoo` now, and so are they.

### Removed

- The Zed extension in `zed/`, with the CI job that built it and the workflow
  that synced it. It was never listed in Zed's registry.

## [1.0.0] - 2026-10-04

### Added

- **The VS Code extension.** `Versions-LE: Compare Versions` compares every
  manifest in the workspace — or under a folder picked in the Explorer — and
  reports each dependency constrained more than one way, each pair no version
  can satisfy, CI toolchains below the declared `rust-version`, prereleases in
  production and floating pins, with every site that produced them, then what
  was deliberately not compared and why. `versions-le.exclude` leaves
  manifests out.
- **The MCP server in the VSIX and on npm** as `versions-le-mcp`: the same
  `compare_versions` tool the Rust CLI serves, answering identically.
- **The engine is a port of the crate's**, with serde_json, the toml crate
  (TOML 1.1) and semver transcribed, held to the crate by the shared corpus, a
  differential that feeds both servers thousands of generated manifest sets —
  broken JSON and TOML included — and a check that both servers define the
  tool identically.
- Localized into twelve languages: the manifest and every runtime string.
- A Zed extension that runs the MCP server as a context server.

## [0.1.1] - 2026-08-14

What changed in the crate is in
[`crate/CHANGELOG.md`](crate/CHANGELOG.md). This section is the
repository around it.

### Added

- **A terminal demo** at [`assets/demo.gif`](assets/demo.gif), driving
  the real binary over the manifests in
  [`assets/demo/`](assets/demo/). [`assets/demo.tape`](assets/demo.tape)
  is the `vhs` script that produced it, so `cd assets && vhs demo.tape`
  reproduces the recording rather than leaving an artifact nobody can
  regenerate. Both sit above `crate/`, where `cargo package` cannot
  reach them.

### Changed

- **New icon artwork.** All sixteen tools were redrawn in one style, so
  the family reads as one set wherever the cards sit side by side. The
  framing is unchanged — the drawing fills 65.8% of an 800×800 canvas
  and every smaller size is derived from that one file rather than drawn
  again.

### Fixed

- **The README's images resolve away from GitHub.** They were repository
  paths, which crates.io and every other renderer resolves against its
  own origin, so the demo and the icon were broken everywhere this file
  is read that is not this repository. They are absolute URLs now.

## [0.1.0] - 2026-08-12

First release. The Rust CLI and MCP server in [`crate/`](crate/): five
manifest readers, six checks, four refusal reasons, and both surfaces.
`cargo install versions-le`, or build from source.

### Added

- **The tool.** Reads `package.json`, `Cargo.toml`, `pyproject.toml`,
  `go.mod` and `.github/workflows/*.yml`, and reports where the same
  dependency is constrained inconsistently across them — including where
  two constraints cannot both be satisfied. One JSON report on stdout, a
  human summary on stderr, and an exit code a CI step can fail on: 0
  clean, 1 findings, 2 malformed question. Full detail in
  [`crate/CHANGELOG.md`](crate/CHANGELOG.md) and
  [`crate/SPEC.md`](crate/SPEC.md).
- **The MCP server** (`versions-le mcp`) with `compare_versions`
  (contents in, no filesystem) and `versions_le_check` (a directory in).
- **Repository documentation** — this file, [README.md](README.md),
  [AGENTS.md](AGENTS.md), [CLAUDE.md](CLAUDE.md), [GEMINI.md](GEMINI.md)
  and [LICENSE](LICENSE). The root files are routers; the crate's own
  `AGENTS.md` and `SPEC.md` remain the source of truth.
- **Four hardening suites**, each carrying the shape of bug it exists to
  catch:
  - `crate/tests/hazards.rs` — a byte-order mark on every manifest kind,
    a manifest that is not UTF-8, a document that parses but is not a
    manifest, symlinks and symlink loops, a FIFO named `Cargo.toml`,
    permission denied, a path over 260 characters, an empty file, a
    50 MB manifest, and a workspace whose `members` point outside the
    tree. Built at runtime; every case a platform cannot express skips
    by name.
  - `crate/tests/platform.rs` — every path the report uses as a
    manifest's identity is forward-slashed on every OS, plus
    case-folding filesystems, reserved Windows device names, CRLF
    manifests, and independence from `TZ`.
  - `crate/tests/fuzz.rs` — time-boxed and seeded, over generated
    constraint strings: enormous prerelease chains, hundreds of `||`
    alternatives, thousands of intersected comparators, unicode
    versions, and every grammar the tool declines to model. Never
    panics, never hangs, always a well-formed report, and **never
    fabricates a conflict out of a grammar it does not model**.
  - `crate/tests/budget.rs` — a wall-clock ceiling on a seeded corpus
    plus two linearity checks: four times the manifests, and four times
    the dependencies in one manifest.
- **A coverage matrix** in `crate/src/detect/corpus.rs`: every
  ecosystem, every manifest kind, every finding code and every refusal
  reason reachable from a real fixture — and nothing in the code that
  the corpus cannot produce. It prints a marker line and CI greps for
  it, because `cargo test <filter>` exits 0 when the filter matches
  nothing.
- **CI jobs** for the five above, on the three-OS matrix where the
  platform is the point — plus one for `crate/tests/scenarios.rs`, which
  had none.

### Fixed

- **Report paths carried a Windows path prefix verbatim**, so a manifest
  named as its own root was labelled
  `\\?\C:/Users/runneradmin/…/Cargo.toml`. `canonicalize` returns an
  extended-length path on Windows and nothing turned it back, which also
  gave one file two identities depending on whether the caller had
  canonicalized. Fixed in `discover::normalise`, and the prefix decision
  is now an exhaustive `match` so it cannot be dropped again on the two
  platforms that never see a prefix.
- The crate's install instructions claimed `cargo install versions-le`.
  It is not on crates.io yet, and a README may not say it is.
- `crate/tests/scenarios.rs` expected 500 refusal rows from 500 crates
  inheriting one workspace dependency, from before refusals for one name
  merged into a single row carrying every site. Nothing set
  `VERSIONS_LE_SCENARIOS`, so the suite had never run and the stale
  expectation was invisible. It now asserts the merge — one row, 500
  sites — and CI runs it.

### The finding that came out of dogfooding

Run against a real workspace, the tool reported
`core = { path = "../core", version = "0.7.7" }` as a **`floating-pin`**.
That reading is right and it is the point of the crate: beside a `path`,
a bare `version` is still a *caret* requirement in Cargo —
`[0.7.7, 0.8.0)` — and not the exact pin the workspace's own
documentation claimed it had. Only `=0.7.7` is that pin. The corpus now
carries the pair as `pin-cargo-path-caret.toml` and
`pin-cargo-path-exact.toml`, with a unit test in the reader, one in the
grammar, and one driving the built binary, so the one-character
difference cannot collapse.

[0.1.0]: https://crates.io/crates/versions-le/0.1.0
[0.1.1]: https://crates.io/crates/versions-le/0.1.1
