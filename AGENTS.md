# AGENTS.md — Versions-LE

Technical source of truth for this repo. README.md is the user-facing doc; this file is for anyone (human or agent) changing the code.

This repo hosts **two products**: the extension at the root (this document's scope) and the Rust CLI in `crate/` (its own `AGENTS.md` + `SPEC.md`, which defines what the tool is allowed to say). The shared corpus lives at `crate/fixtures/`. `scripts/check-detection-parity.ts` fails CI when this extension drifts from it, `scripts/check-detection-differential.ts` feeds both MCP servers generated manifest sets, broken JSON and TOML included, and fails on any difference, and `scripts/check-mcp-definition.ts` fails when the two servers define `compare_versions` differently.

## What this is

A VS Code extension that compares the version constraints in every manifest of a workspace — `package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod` and GitHub workflows — and reports where one dependency is constrained differently, where two constraints cannot both be satisfied, where CI builds below a declared `rust-version`, and the pins that float. A constraint in a grammar it does not model is named and left out of every comparison. It opens a Markdown report and edits nothing. No network access, no filesystem writes.

## Architecture

```
extension.ts             activate(): telemetry/notifier/statusBar -> registerCommands()
commands/                compare (find the manifests under a folder, read them, report)
detect/                  THE engine, a port of the crate — pure, no vscode:
  heuristics.ts          heuristics.rs: which manifest a path is, the version-evidence gate,
                         and std::path's normalisation
  parser.ts              parser.rs: the five manifest readers
  grammar.ts             grammar.rs: Cargo, npm, PEP 440, go and CI constraints as
                         interval unions
  compare.ts             compare.rs: the six checks and two of the refusals
  report.ts              scan::report_for and the refusal merge
  semver.ts              semver 1.0.28: Version / VersionReq parsing and ordering
  json.ts                serde_json 1.0.151 into Value: sorted keys, the last of a repeat
  toml.ts                toml 1.1.6 (TOML 1.1): toml_parser's grammar, toml_datetime's dates,
                         the toml crate's table rules, and toml_writer's Display
  text.ts                Rust's trim, split_whitespace and lines
report/format.ts         the Markdown report
mcp/                     the npm server: tools.ts (compare_versions), transport.ts
ui/, config/, telemetry/ as in every sibling
types.ts                 shared types only — no logic
```

Conventions: factory functions + `Object.freeze` (classes only inside the parsers), guard clauses, dependency bags typed inline at the consumer — see **Code style** below. Both the manifest and the runtime strings are localized into 12 locales; see **Toolchain**.

## The engine is the crate's

`src/detect/` is a port of `crate/src/detect/`, held to it three ways: the corpus (`check-detection-parity.ts`, `corpus.test.ts`), the differential (thousands of generated manifest sets through both MCP servers), and the definition check. A difference is a bug in one of them.

- **Three crates are transcribed, not approximated.** Which `package.json` is "not valid JSON" is serde_json's answer; which `Cargo.toml` is "not valid TOML" is the toml crate's, under TOML 1.1; what a Cargo requirement means and how two versions order is semver's. `JSON.parse`, any npm TOML library and node-semver each answer differently somewhere, and a manifest one surface reads and the other refuses is a difference in every finding it carries.
- **The toml crate's table rules, not the spec's.** The crate lets a dotted key extend a table a header created, and walks a dotted key into the last table of an array of tables — both outside what toml-test calls valid. `toml.ts` transcribes the crate's `document.rs` state machine (`implicit`, `dotted`, `inline`, and the table under construction held apart from the document) rather than the spec.
- **`toml::Value`'s Display reaches the report.** A Cargo dependency spelled as neither a string nor a table is recorded as written, through toml_writer: Rust's plain-decimal `f64`, the plainest string quoting the text allows, inline tables with sorted keys.
- **Byte order wherever the crate sorts.** Every `BTreeMap` and `sort` in the crate orders strings by UTF-8 bytes, which differs from JavaScript's UTF-16 order past the BMP; `compareStrings` is code-point order, which is byte order.
- **Sorted maps.** Without `preserve_order`, serde_json's and the toml crate's maps iterate keys in sorted order, so dependency sections are read in that order and the report follows it.

## Code style

These are not preferences to weigh against convenience. They are the shape the
code is expected to take, and a review rejects work that ignores them. The
reason each one exists is stated, because a rule without a reason gets
cargo-culted into places it does not belong.

### Control flow

**Guard clauses first, then the work.** Every function opens with its
preconditions, each one returning immediately. The body that follows is the
happy path at a single indent level, and it reads top to bottom.

```ts
// Yes — preconditions leave, then the real work runs unindented.
function extract(document: TextDocument, config: Configuration): Result {
	if (!document) return EMPTY;
	if (!isSupported(document.languageId)) return unsupported(document.languageId);

	const text = document.getText();
	if (!text.trim()) return EMPTY;

	return runExtraction(text, config);
}
```

**No `else`. No `else if`.** An `else` is a guard clause that has not been
extracted yet. Two branches become an early return; many branches become a
lookup table or a `switch` that returns from every arm. This is the rule that
does the most work in practice — it is what keeps nesting flat, keeps diffs
small, and stops a function growing a second responsibility inside its own
`else`.

```ts
// No.
if (kind === 'hex') {
	return parseHex(value);
} else if (kind === 'rgb') {
	return parseRgb(value);
} else {
	return null;
}

// Yes — a table. Adding a format touches one line and no control flow.
const PARSERS: Readonly<Record<ColorKind, Parser>> = Object.freeze({
	hex: parseHex,
	rgb: parseRgb,
	hsl: parseHsl,
});

function parse(kind: ColorKind, value: string): Color | null {
	const parser = PARSERS[kind];
	if (!parser) return null;
	return parser(value);
}
```

**Maximum nesting is two levels inside a function.** A third level means the
inner block wants to be its own named function. Loops containing conditionals
containing conditionals are where bugs hide, because no reader holds all three
conditions at once.

**Truthy checks.** `if (!value)` rather than
`if (value === undefined || value === null || value === '')`. The exception is
real and must be respected: when `0`, `''` or `false` are legitimate values,
test explicitly (`value === undefined`, `Number.isFinite(value)`). A threshold
of `0`, an empty string that means "cleared", and `false` from `applyEdit` have
all been live bugs in this family — the terse form is the default, not a
licence to ignore the domain.

### Errors

**Every error path is handled and says something true.** A message names what
failed, why, and what state the user is now in. "Extraction failed" is not a
message; "Could not replace the document contents: the edit was rejected" is.

**Never swallow.** No empty `catch`, no `catch { return null }` that erases a
cause the caller needed, no `|| true`, no `continue-on-error`. If a failure is
genuinely ignorable, the `catch` says why in a comment.

**Failures are values where the caller must react.** A parse failure that the
user should see is reported through the callback or return value the caller
supplied — not thrown past it, and never turned into a silent empty result.
Reserve `throw` for programmer error and for unwinding to a command's outer
handler, which is the one place that decides what the user sees.

**Never report success you did not achieve.** Check what the API returned.
`vscode.workspace.applyEdit` resolves `false` for a read-only document; a
cancelled operation delivers nothing. Announcing a count over work that never
happened is the single most repeated defect in this family's history.

### Data

**Immutable by default.** `readonly` on every interface field, `ReadonlyArray`
on every collection you do not own, `Object.freeze` on returned config and
result objects. Never mutate a parameter. Build a new value and return it.
Where a mutable working copy is genuinely needed, derive the mutable type
(`type Draft<T> = { -readonly [K in keyof T]: T[K] }`) rather than
hand-maintaining a second parallel interface that drifts.

**Composition over inheritance.** Factory functions returning frozen objects,
not classes and not `extends`. Dependencies arrive as a parameter — a typed
deps bag — so a test supplies a fake without a framework. There is no
inheritance hierarchy anywhere in this fleet and there should never be one.

```ts
export function createNotifier(deps: Readonly<{ config: Configuration }>): Notifier {
	return Object.freeze({
		showInfo: (message: string) => { /* ... */ },
		showError: (message: string) => { /* ... */ },
	});
}
```

### Structure

**Logic and presentation are separate, always.** Extraction, analysis and
conversion modules compute and return data. They never call
`vscode.window.*`, never format a user-facing sentence, never decide whether a
notification is shown. `ui/` renders; `commands/` orchestrates. The test for
whether you got this right: a logic module should be unit-testable without the
`vscode` mock at all.

**Where a UI framework is involved, the same rule applies to the render.**
Compute above, return markup below. A render body holds no conditionals beyond
a trivial ternary, no data shaping, no derivation — those are named values or
functions above it. Anything else produces JSX no one can read, and it hides
the logic from the tests.

**Commands are thin.** A command reads config, calls logic, hands the result to
the UI layer, and handles failure. When a command file grows a parser or a
formatter, that code belongs in `extraction/` or `ui/`.

**No god files.** Past ~300 lines, a file is doing more than one job and wants
splitting along the seam that is already visible in its exports. `types.ts`
holds types only — no logic, ever.

**Separation of concerns, without ceremony.** One module per real concept, not
one per function. A `utils/` folder of single-line files is as unmaintainable
as a god file; both make you read the whole tree to understand one path.

**Define it once.** Duplicate regexes, duplicate `fullDocumentRange`,
duplicate "is this a supported scheme" checks — each has already shipped as a
bug in this family, because copies drift and only one copy gets fixed. When you
find yourself writing something that exists elsewhere, move it to a shared
module in the same commit.

### Comments

Comments explain **why**, never what. A comment restating the code is noise
that goes stale. A comment recording the reason a non-obvious choice was made —
the constraint, the bug it prevents, the API quirk it works around — is the
most valuable line in the file, and it is what keeps the next person from
"simplifying" it back into a defect.

---

## Invariants (things that were once broken — keep them true)

- **Comparison never crosses an ecosystem.** One name in two ecosystems is a `cross_ecosystem` refusal, never a comparison.
- **The rating prompt is asked at most twice per user, and only after a run that delivered its result.** `rating/policy.ts` holds the thresholds (3 uses on at least the second distinct day, then once more 22 uses on, which is the 25th when the first ask was on the 3rd) and has no `vscode` import; `ui/ratingPrompt.ts` shows it. Both files, and their tests, are byte-identical across the family — change one repo and copy to the rest. The ask is written to `globalState` *before* the toast is awaited, because a window closed with the toast up never resolves it and an ask counted only on an answer would reappear on every launch. It bypasses the notifier on purpose: where `notificationsLevel` defaults to a quiet level, the effective level would silence it for everyone. What it honours instead is a level the user set themselves — any explicit value other than `all` means never ask. **Rate opens the registry this copy was installed from**, read off the publisher in `context.extension.id`: the Marketplace build is `nolindnaidoo`, the Open VSX build is `OffensiveEdge`, and any other publisher (a fork, a local package) has no listing and is never asked. Never hook it to activation, and never to a path that can fail.
- **A grammar the engine does not model is refused, never approximated.** It is an `unknown_grammar` refusal and takes part in no comparison.
- **A manifest that does not parse is named, never dropped.** It is a diagnostic; the manifests that parsed still answer. One that is not UTF-8 text is named too.
- **`node_modules`, `.git` and `vendor` are never read**, whatever the exclude setting says.
- **The bundle must be self-contained.** The VSIX ships `dist/extension.js` only; `scripts/check-bundle.js` (run in `vscode:prepublish` and CI) does a static require scan AND loads the bundle with `vscode` stubbed.
- **`CONFIG_DEFAULTS` must equal package.json defaults.** `config.test.ts` asserts parity over every declared setting.
- **Every declared setting must have a consumer, and every declared command must be registered.** The integration suite checks the second in a real host.
- **nls catalogues stay in key-parity**, and every translation is in its own language — `letools-site`'s `check-locales` holds that across the family.
- **The MCP server must never reference `vscode`**, and launching it needs `ELECTRON_RUN_AS_NODE=1`; `scripts/e2e-vsix.js` spawns the installed server exactly as `provider.ts` does.

## Toolchain

- **Runtime targets:** `engines.vscode` is the supported floor and `@types/vscode` is pinned to it **exactly**. A caret there lets the type surface drift ahead of the version users actually run, so code compiles against APIs that are not there at runtime. Dependabot is configured to never bump it.
- **Build:** esbuild bundle (`bun run build`, `build:prod` minified). `tsc` is typecheck-only (`noEmit`) and covers test files. TypeScript 7.
- **Unit tests:** vitest 4; `vscode` aliased to `src/__mocks__/vscode.ts` (stateful mock with `_reset/_set` helpers). Coverage provider `v8`, thresholds enforced at **75 lines / 80 functions / 60 branches / 75 statements**. These are a backstop against a module nobody tested, not a target — they sit well below where the code actually is, and they are not raised to track it. Lower them only with a reason; do not raise them to chase a number.
- **Integration tests:** `bun run test:integration` — `@vscode/test-cli` launches a real VS Code (config in `.vscode-test.mjs`, tests compiled via `tsconfig.it.json` to `out-test/`). That project targets `node16` module resolution; TypeScript 7 removed `node10`, which `"Node"` resolved to.
- **Installed-VSIX tests:** `bun run test:e2e-vsix` installs the built `.vsix` into a clean VS Code profile and drives it. This is the only test that exercises the artifact users receive, and it runs in CI.
- **Lint/format:** Biome (tabs, single quotes). `__fixtures__`/`__snapshots__` are exempt — formatting fixtures would corrupt goldens. `biome.json` is one of the files `letools-site/scripts/check-fleet.ts` holds byte-identical across all ten repos; change it in one, copy it to the rest, and let `bun run check:fleet ../` say whether a copy was missed.
- **Packaging:** `bun run package` → `release/*.vsix`. `.vscodeignore` is an allow-list; the VSIX ships only the bundle, the MCP server, the catalogues, the icon and the docs. Packaging uses `--no-dependencies`: the bundle is self-contained, so walking the npm tree served no purpose and broke after any dependency change.
- **Localization:** two separate mechanisms. The 12 `package.nls.*.json` catalogues in `src/i18n/` localize **manifest** strings (VS Code `%key%` substitution) and are copied to the package root at prepublish, then removed by `clean:i18n`. The 12 `l10n/bundle.l10n.*.json` catalogues localize **runtime** strings via `vscode.l10n.t()`, enabled by `"l10n": "./l10n"` in package.json. They fail independently: a working manifest says nothing about the runtime bundles. The rules that keep both correct are under **Code style** above.

- **npm package:** `bun run build:npm` assembles `mcp/` and writes its version from the root manifest, so the two can never claim the same version while carrying different code. `bun run check:npm-package` packs it, installs the tarball into a throwaway project and drives the *installed* binary through a handshake — which is what `npx` does, minus the registry. Run it before publishing: a version cannot be reused, and the unpublish window is 72 hours.

## Generated documentation

Two README sections are generated. Do not hand-edit the content between their markers.

- `bun run test:coverage && bun run coverage:readme` writes the Testing section from `coverage/coverage-summary.json`. CI runs `coverage:readme:check`, which fails when the committed numbers no longer match a real run — coverage is compared within 1 percentage point (it is not bit-identical across machines), while test counts are derived from source and must match exactly.
- `bun run benchmark && bun run perf:readme` writes the Performance section from a real run of `extract` over generated documents. This is **not** checked in CI: throughput is machine-specific, so a hosted runner would fail it for reasons that say nothing about the code. The host is printed with the numbers instead.

A sibling's README once carried hand-written test counts and throughput figures that drifted until they were false. Generating them is what stops that recurring.

## Security & automation

- **CodeQL** runs on push, PR and weekly (`javascript-typescript` + `actions`), configured in `.github/codeql-config.yml`. Test files and fixtures are excluded on purpose: they contain inputs that are supposed to look dangerous, and scanning them produces findings that can only ever be dismissed.
- **Dependabot** (`bun` ecosystem, not `npm` — the npm updater rewrites `package.json` without regenerating `bun.lock`, so its PRs can never pass the frozen-lockfile gate) opens grouped weekly PRs. `cargo` covers `crate/`.
- **Auto-merge** is workflow-driven, not GitHub-native: `main` has no required status checks, so native auto-merge would land a PR before CI started. `dependabot-auto-merge.yml` waits for every other check on the head commit to pass, then merges any update but a major, runtime dependencies included: a merge publishes nothing, and the VSIX only ships from a manual release. Majors need a human. After a merge it dispatches CI on `main`, because a merge made with `GITHUB_TOKEN` starts no push run and each PR was only tested against its own base. The workflow is byte-identical across all sixteen tool repos.
- **Actions are pinned to commit SHAs.** A tag is mutable and this repo holds a publish token. The trailing `# vX.Y.Z` comment is what Dependabot reads and rewrites.
- **Branch safety:** a `main-safety` ruleset blocks deletion and force-push. Pushes to `main` are otherwise unrestricted by design.
- Secret scanning and push protection are enabled. `VSCE_PAT` and `OVSX_PAT` live in repo secrets and in Doppler (`extensions` / `prd`).

## Agent and editor instructions

Every major coding assistant looks for its own instruction file, so each one is
present and each is a thin pointer to this document:

| File | Tool |
|---|---|
| `AGENTS.md` | the standard itself — OpenAI Codex and others read this directly |
| `CLAUDE.md` | Claude Code |
| `GEMINI.md` | Gemini CLI |
| `.cursorrules`, `.cursor/rules/project.mdc` | Cursor (legacy and current formats) |
| `.windsurfrules` | Windsurf |
| `.clinerules` | Cline |
| `.github/copilot-instructions.md` | GitHub Copilot |

**Keep them thin.** They restate the non-negotiables and route the reader here;
they must never grow a second copy of the standard, because a copy drifts and
then two tools disagree about the same repository. Change the standard here,
and only the pointer's short list if a non-negotiable itself changed.

None of them ship: `.vscodeignore` is an allow-list, so the VSIX is unaffected.

## Git identity

Every commit uses the GitHub noreply address:

```
13629544+nolindnaidoo@users.noreply.github.com
```

A real address in commit metadata is public forever — GitHub's API serves it
for any public repo, and scrapers harvest it. Never set a real address in
`user.email`, globally or repo-locally, and never commit with one. GitHub's
*Block command line pushes that expose my email* is the backstop; the global
config is the default. A repo-local `user.email` silently overrides the global
one, so check `git config user.email` in a fresh clone before the first commit.

## Commits

Subjects use a conventional prefix — `feat:`, `fix:`, `docs:`, `test:`, `ci:`,
`build:`, `chore:`, `refactor:`, `perf:`, `revert:` — an optional `(scope)`,
and an imperative summary with no trailing period. The body says why the
change was needed and what it prevents; a subject alone is rarely enough to
reconstruct a decision six months later.

This is enforced, not just documented:

- **`commit-msg` hook** — `bun run hooks:install` points `core.hooksPath` at
  `.githooks/`, and `prepare` runs it on install, so a fresh clone is wired
  after `bun install`. It rejects the message before the commit exists.
- **CI** — the `Commit messages` job runs the same validator over the pushed
  range. The hook is skippable with `--no-verify`; this is not, so skipping it
  delays the failure rather than avoiding it.

Both call one implementation, `scripts/commit-lint.js`, so the rules cannot
drift apart. Check a branch yourself with `bun run lint:commits`. Merge commits
are exempt — git writes those subjects, not a person. Only the commits in a
push are checked, so history predating the gate is left alone.

## Release

**Three files carry the extension version, and CI fails unless all three agree.**
`package.json`, `mcp/package.json` and `server.json` (**both** `.version` and
`.packages[0].version`). The same CI step pins registry identity:
`server.json.name` must equal `mcp/package.json.mcpName`, and
`server.json.packages[0].identifier` must equal `mcp/package.json.name`.

**This gate is not relaxable.** The MCP registry verifies ownership by reading
`mcpName` out of the *published* npm package, so a mismatch is only discoverable
after the version is spent — and a version can never be republished. That is why
this is a gate rather than a convention.

The crate version is deliberately **outside** this gate — `crate/Cargo.toml`
moves on its own cadence, and the crate sitting at 0.x while the extension starts at 1.0 is intended.

1. Bump `version` in package.json and write the CHANGELOG entry. The entry must describe what actually changed, including bug fixes — it ships inside the VSIX and renders on the listing page.
2. Regenerate the README sections (`coverage:readme`, and `perf:readme` if behaviour changed) and commit them.
3. CI green on all three OSes. That includes lint, typecheck, coverage, the bundle gate, packaging, integration tests, and the installed-VSIX e2e.
4. Tag the commit being released, so the tag is the artifact rather than an approximation of it.
5. Dispatch the `Release` workflow. It takes two independent opt-ins — `marketplace` (default **on**) and `openvsx` (default **off**) — because a version cannot be republished, so a run that publishes one registry and fails on the other is only recoverable by re-running with the failed target alone. It validates credentials before doing anything irreversible.

**Open VSX defaults off deliberately.** `ovsx publish` takes no namespace argument; it derives the namespace from `publisher` in the VSIX. Enabling it publishes to whatever `package.json` currently names, with no confirmation.

**The npm package ships from the same release**, as a third opt-in on the `Release` workflow. It publishes with the **`NPM_TOKEN`** repository secret, the same token every package in the family uses, kept in Doppler under `extensions` / `prd`. It expires and has to be replaced in both places when it does. Trusted publishing was written into this workflow and never set up on npm, so that step failed every time it ran until it was changed to the token.

## Known limitations (documented, not bugs)

- Everything in `crate/SPEC.md`'s non-goals holds here: no edits, no dependency resolution, no lockfiles, no network.
- `versions-le.exclude` takes VS Code globs, matched against paths relative to the folder being compared; the CLI's `--exclude` takes globset patterns. The two agree on ordinary patterns such as `examples/**`.
- In a multi-root workspace each manifest is labelled with its workspace folder's name; the CLI qualifies labels with the root as typed.
- A finding's message is the engine's English, identical to the CLI's.

