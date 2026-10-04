import * as vscode from 'vscode';
import type { Telemetry } from '../telemetry/telemetry';

/**
 * Register the help command
 */
export function registerHelpCommand(
	context: vscode.ExtensionContext,
	telemetry: Telemetry,
): void {
	const disposable = vscode.commands.registerCommand(
		'versions-le.help',
		async () => {
			telemetry.event('command', { name: 'help' });
			await showHelp();
		},
	);
	context.subscriptions.push(disposable);
}

async function showHelp(): Promise<void> {
	const doc = await vscode.workspace.openTextDocument({
		content: generateHelpContent(),
		language: 'markdown',
	});
	await vscode.window.showTextDocument(doc, {
		preview: false,
		viewColumn: vscode.ViewColumn.Beside,
	});
}

/** Every claim here is the crate's behaviour, and the corpus pins it. */
export function generateHelpContent(): string {
	return [
		'# Versions-LE Help',
		'',
		"Finds where one dependency is constrained differently across a repository's manifests — and where two constraints cannot both be satisfied by any version. A constraint in a grammar the tool does not model is named and left out of the comparison, never guessed at.",
		'',
		'## Commands',
		'',
		'- **Compare Versions** (`Ctrl+Alt+V`, Mac `Cmd+Alt+V`): every manifest in the workspace, or under the folder picked in the Explorer.',
		'',
		'## What it reads',
		'',
		'| Manifest | Ecosystem | What |',
		'|---|---|---|',
		'| `package.json` | npm | dependency sections, `engines`, `packageManager` |',
		'| `Cargo.toml` | cargo | dependency sections, workspace and target dependencies, `rust-version` |',
		'| `pyproject.toml` | python | `requires-python`, `dependencies`, optional dependencies |',
		'| `go.mod` | go | the `go` directive and `require` |',
		'| `.github/workflows/*.yml` | ci | action `uses:` refs and `<tool>-version:` inputs |',
		'',
		'Comparison never crosses an ecosystem: an npm `semver` and a Cargo `semver` are unrelated packages that share a word. `node_modules`, `.git` and `vendor` are never read.',
		'',
		'## Findings',
		'',
		'| Code | Severity | When |',
		'|---|---|---|',
		'| `disjoint-constraint` | error | Two constraints on one dependency that no single version satisfies |',
		"| `malformed-constraint` | error | A constraint shaped like its ecosystem's grammar, and broken |",
		'| `constraint-conflict` | warning | One dependency constrained more than one way |',
		'| `msrv-mismatch` | warning | `rust-version` declared differently, or CI building below it |',
		'| `prerelease-in-production` | warning | A prerelease outside dev dependencies |',
		'| `floating-pin` | info | A pin that floats: `*`, a dist tag, a caret on 0.x, a branch ref |',
		'',
		'## Not compared',
		'',
		'| Reason | When |',
		'|---|---|',
		'| `unknown_grammar` | A git, path, workspace or URL specifier, or a PEP 440 form the tool does not model |',
		'| `ambiguous_version_string` | A workflow value that is not evidently a version, such as `${{ matrix.node }}` |',
		'| `cross_ecosystem` | One name in two ecosystems |',
		'| `per_job_tool_version` | A CI tool installed at different versions by different jobs |',
		'',
		'## Agents',
		'',
		"The bundled MCP server offers `compare_versions` to agent mode. It answers exactly as the `versions-le` command-line tool's server does.",
		'',
		'## Troubleshooting',
		'',
		'- **A manifest is missing from the report**: check `versions-le.exclude`, and that a workflow sits under `.github/workflows/`.',
		'- **"not valid TOML" or "not valid JSON"**: the manifest does not parse, so nothing in it was compared.',
		'',
	].join('\n');
}
