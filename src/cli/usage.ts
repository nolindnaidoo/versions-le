/**
 * The command line's help text and the flags it names.
 *
 * Word for word the Rust CLI's, because both install as `versions-le` and a
 * person reading `--help` cannot tell which one they have.
 * `scripts/check-cli-parity.ts` compares the two outputs, and `cli.test.ts`
 * holds FLAGS equal to the flags this text names.
 */

export const USAGE = `usage: versions-le [options] <dir|file>...
       versions-le mcp
       versions-le --version | --help

Finds the version constraints in a repository's manifests — package.json,
Cargo.toml, pyproject.toml, go.mod and .github/workflows — and reports
where the same dependency is constrained inconsistently.

One JSON report on stdout when it is piped or redirected, human summary
on stderr. At a terminal stdout stays silent.

Options:
  --ecosystem <name>   only npm, cargo, python, go or ci; repeatable
  --fail-on <what>     conflict (default: exit 1 on anything above info)
                       or any (exit 1 on any finding, floating pins too)
  --strict             a refusal or an unreadable manifest exits 2
  --exclude <glob>     skip manifests matching this pattern; repeatable
  --hidden             descend hidden directories too
  --no-ignore          walk files that .gitignore excludes

Comparison never crosses an ecosystem: an npm \`semver\` and a Cargo
\`semver\` are unrelated packages that share a word. A constraint in a
grammar this tool does not model — PEP 440's ~=, a git or workspace
dependency, a commit-SHA action pin — is named in the report's refusals
and left out of the comparison, never guessed at.

Exit codes: 0 clean · 1 findings · 2 malformed question. Finding no
manifests at all is 0 — there is nothing to be in conflict with.`;

export const FLAGS: readonly string[] = Object.freeze([
	'--ecosystem',
	'--fail-on',
	'--strict',
	'--exclude',
	'--hidden',
	'--no-ignore',
]);
