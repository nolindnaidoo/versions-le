import * as vscode from 'vscode';
import { readConfig } from '../config/config';
import { manifestKind } from '../detect/heuristics';
import {
	type Diagnostic,
	type Document,
	type Report,
	reportFor,
} from '../detect/report';
import { compareStrings } from '../detect/semver';
import { formatReport } from '../report/format';
import type { CommandDependencies } from './index';
import { showReport } from './output';

/** What a manifest search is: the five kinds the engine reads, and nothing else. */
export const MANIFEST_GLOB =
	'**/{package.json,Cargo.toml,pyproject.toml,go.mod,.github/workflows/*.yml,.github/workflows/*.yaml}';

/**
 * Directories never worth reading: a `node_modules` holds a `package.json` per
 * installed package, which are the resolver's output, not this repository's
 * constraints. The CLI skips the same three.
 */
const VENDORED = Object.freeze([
	'**/node_modules/**',
	'**/.git/**',
	'**/vendor/**',
]);

// The decoder's default drops a leading byte order mark, as the CLI's read
// does; kept in the text it made a package.json "not valid JSON" and hid the
// first line of a go.mod and the first key of a workflow.
const decoder = new TextDecoder('utf-8', { fatal: true });

const diagnostic = (file: string, message: string): Diagnostic => ({
	severity: 'error',
	code: 'unreadable',
	file,
	message,
});

/** The host's reason when it gives one: a `FileSystemError` carries a code such as `NoPermissions`. */
function reason(error: unknown): string {
	const code = (error as { code?: unknown } | null)?.code;
	return typeof code === 'string' && code !== '' ? ` (${code})` : '';
}

/**
 * Compare every manifest under a folder: the one picked in the Explorer, or
 * every workspace folder. Each manifest is labelled by its path relative to
 * that folder, which is also what decides that a `.yml` is a workflow.
 */
export async function compareWorkspace(
	deps: CommandDependencies,
	picked?: vscode.Uri,
): Promise<void> {
	deps.telemetry.event('command', { name: 'compare' });
	const roots =
		picked !== undefined
			? [picked]
			: (vscode.workspace.workspaceFolders ?? []).map((folder) => folder.uri);
	if (roots.length === 0) {
		deps.notifier.error(
			vscode.l10n.t('Open a folder or workspace to compare its manifests'),
		);
		return;
	}
	const config = readConfig();
	const exclude = `{${[...VENDORED, ...config.exclude].join(',')}}`;
	const qualify = roots.length > 1;
	const documents: Document[] = [];
	const unreadable: Diagnostic[] = [];
	for (const root of roots) {
		const found = await vscode.workspace.findFiles(
			new vscode.RelativePattern(root, MANIFEST_GLOB),
			exclude,
		);
		for (const uri of found) {
			const relative = uri.path.slice(root.path.replace(/\/$/, '').length + 1);
			const label = qualify
				? `${root.path.split('/').at(-1)}/${relative}`
				: relative;
			const kind = manifestKind(relative);
			if (kind === undefined) continue;
			// Each failure is named for what happened, so the answer is never
			// quietly narrower than it looks, and a file that went away between
			// the listing and the read is never called "not UTF-8 text".
			let bytes: Uint8Array;
			try {
				bytes = await vscode.workspace.fs.readFile(uri);
			} catch (error) {
				unreadable.push(diagnostic(label, `could not be read${reason(error)}`));
				continue;
			}
			try {
				documents.push({ kind, label, content: decoder.decode(bytes) });
			} catch {
				unreadable.push(diagnostic(label, 'not UTF-8 text'));
			}
		}
	}
	documents.sort((a, b) => compareStrings(a.label, b.label));
	const analysed = reportFor(documents);
	const report: Report = {
		...analysed,
		diagnostics: [...analysed.diagnostics, ...unreadable],
		summary: {
			...analysed.summary,
			errors: analysed.summary.errors + unreadable.length,
		},
	};

	const opened = await showReport(
		formatReport(report, config.showPositions),
		config,
		deps,
		formatReport(report, config.clipboardIncludesPositions),
	);
	deps.telemetry.event('compared', {
		manifests: String(report.summary.manifests),
		findings: String(report.summary.findings),
	});
	// A report nobody can see announces nothing more: the failure to open it
	// was shown, and a count or a toast after it would describe a result the
	// user was never handed.
	if (!opened) return;
	deps.statusBar.flash(
		vscode.l10n.t('{0} finding(s)', report.summary.findings),
	);
	if (report.summary.manifests === 0) {
		// Every manifest failing to read is a failed run, not an empty one, and
		// it is shown at every notification level.
		if (unreadable.length > 0)
			deps.notifier.error(
				vscode.l10n.t('{0} manifest(s) could not be read', unreadable.length),
			);
		else deps.notifier.info(vscode.l10n.t('No manifests found'));
		return;
	}
	const failing = report.summary.errors + report.summary.warnings;
	if (failing > 0)
		deps.notifier.warn(
			vscode.l10n.t(
				'{0} version problem(s) across {1} manifest(s)',
				failing,
				report.summary.manifests,
			),
		);
}
