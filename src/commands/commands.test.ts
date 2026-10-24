import { beforeEach, describe, expect, it } from 'vitest';
import {
	_clipboardText,
	_createDocument,
	_createExtensionContext,
	_openedDocuments,
	_registeredCommands,
	_resetMockState,
	_setActiveEditor,
	_setClipboardError,
	_setConfig,
	_setOpenDocumentError,
	_setReadFileError,
	_setWorkspaceFiles,
	_shownDocumentOptions,
	_shownMessages,
	executedBuiltins,
	Uri,
	ViewColumn,
	workspace,
} from '../__mocks__/vscode';
import { registerOpenSettingsCommand } from '../config/settings';
import type { Telemetry } from '../telemetry/telemetry';
import { createNotifier } from '../ui/notifier';
import type { StatusBar } from '../ui/statusBar';
import { generateHelpContent, registerHelpCommand } from './help';
import { registerCommands } from './index';

function makeDeps() {
	const flashes: string[] = [];
	const telemetry: Telemetry = { event: () => {}, dispose: () => {} };
	const statusBar: StatusBar = { flash: (text) => flashes.push(text) };
	return {
		deps: {
			notifier: createNotifier(),
			statusBar,
			telemetry,
			ratingPrompt: { recordSuccess: async () => {} },
		},
		flashes,
	};
}

async function runCommand(id: string, ...args: unknown[]): Promise<void> {
	const handler = _registeredCommands().get(id);
	if (!handler) throw new Error(`command not registered: ${id}`);
	await handler(...args);
}

function report(): string {
	const last = _openedDocuments().at(-1);
	if (!last) throw new Error('no report was opened');
	return last.getText();
}

const TREE = {
	'/w/api/Cargo.toml':
		'[package]\nrust-version = "1.88"\n[dependencies]\nregex = "1"\nserde = "1.0.200"\n',
	'/w/web/Cargo.toml': '[dependencies]\nregex = "2"\nserde = "1.0"\n',
	'/w/package.json': '{"dependencies": {"left-pad": "git+https://x/y.git"}}',
	'/w/.github/workflows/ci.yml':
		'jobs:\n  t:\n    steps:\n      - uses: dtolnay/rust-toolchain@1.85\n',
	'/w/node_modules/x/package.json': '{"dependencies": {"regex": "3"}}',
	'/w/k8s/deploy.yml': 'uses: x@v1\n',
};

let flashes: string[] = [];
beforeEach(() => {
	_resetMockState();
	const made = makeDeps();
	flashes = made.flashes;
	registerCommands(_createExtensionContext() as never, made.deps);
});

const useWorkspace = (files: Record<string, string>) => {
	_setWorkspaceFiles(files);
	workspace.workspaceFolders = [
		{ uri: Uri.file('/w'), name: 'w', index: 0 },
	] as never;
};

describe('versions-le.compare', () => {
	it('errors when there is no folder to compare', async () => {
		await runCommand('versions-le.compare');
		expect(_shownMessages()[0]).toMatchObject({
			kind: 'error',
			message: 'Open a folder or workspace to compare its manifests',
		});
	});

	it('finds every finding across the tree, labelled by path, and never reads node_modules or a non-workflow yml', async () => {
		useWorkspace(TREE);
		await runCommand('versions-le.compare');
		const text = report();
		expect(text).toContain('4 manifest(s)');
		expect(text).toContain(
			'- **disjoint-constraint** · cargo · `regex`: "1" (api/Cargo.toml) and "2" (web/Cargo.toml) cannot both be satisfied by one version',
		);
		expect(text).toContain('  - `api/Cargo.toml` · `dependencies.regex` = `1`');
		expect(text).toContain(
			'**msrv-mismatch** · ci · `rust`: CI builds on 1.85.0, below the declared minimum 1.88.0 in api/Cargo.toml',
		);
		expect(text).toContain(
			'`.github/workflows/ci.yml`:4 · `toolchain` = `1.85`',
		);
		expect(text).toContain('**unknown_grammar** · npm · `left-pad`');
		expect(text).not.toContain('node_modules');
		expect(text).not.toContain('k8s');
		expect(flashes.at(-1)).toMatch(/finding\(s\)$/);
	});

	it('compares only the folder picked in the Explorer, labelled relative to it', async () => {
		useWorkspace(TREE);
		await runCommand('versions-le.compare', Uri.file('/w/api'));
		const text = report();
		expect(text).toContain('1 manifest(s)');
		expect(text).toContain('`Cargo.toml` · cargo');
	});

	it('compares the workspace when the argument is not a folder, as a keybinding passes an editor', async () => {
		useWorkspace(TREE);
		await runCommand('versions-le.compare', { document: {}, selection: {} });
		expect(report()).toContain('4 manifest(s)');
	});

	it('names a manifest that does not parse, and one that is not text', async () => {
		_setConfig('versions-le.notificationsLevel', 'all');
		useWorkspace({
			'/w/Cargo.toml': 'a = ',
			'/w/go.mod': new Uint8Array([0xff, 0xfe]) as never,
		});
		await runCommand('versions-le.compare');
		const text = report();
		expect(text).toContain('## Could not be read (2)');
		expect(text).toContain('- `Cargo.toml`: not valid TOML');
		expect(text).toContain('- `go.mod`: not UTF-8 text');
	});

	it('names a file that could not be read apart from one that is not text', async () => {
		useWorkspace({
			'/w/Cargo.toml': '[dependencies]\nserde = "1"\n',
			'/w/go.mod': new Uint8Array([0xff, 0xfe]) as never,
			'/w/package.json': '{"dependencies": {"a": "^1"}}',
		});
		_setReadFileError(
			'/w/Cargo.toml',
			Object.assign(new Error('EACCES'), { code: 'NoPermissions' }),
		);
		await runCommand('versions-le.compare');
		const text = report();
		expect(text).toContain('## Could not be read (2)');
		expect(text).toContain('- `Cargo.toml`: could not be read (NoPermissions)');
		expect(text).toContain('- `go.mod`: not UTF-8 text');
		expect(text).not.toContain('Cargo.toml`: not');
	});

	it('reads a manifest written with a byte order mark as the CLI reads it', async () => {
		useWorkspace({
			'/w/package.json': '\uFEFF{"dependencies": {"a": "latest"}}',
			'/w/go.mod': '\uFEFFmodule x\n\ngo 1.22\n\nrequire example.com/p v1.2.3\n',
		});
		await runCommand('versions-le.compare');
		const text = report();
		expect(text).toContain('2 manifest(s), 3 constraint(s)');
		expect(text).toContain('**floating-pin** · npm · `a`');
		expect(text).not.toContain('Could not be read');
	});

	it('leaves out what the exclude setting names', async () => {
		_setConfig('versions-le.exclude', ['web/**']);
		useWorkspace(TREE);
		await runCommand('versions-le.compare');
		expect(report()).not.toContain('web/Cargo.toml');
	});

	it('reads an exclude pattern holding a comma or a brace group as one pattern', async () => {
		_setConfig('versions-le.exclude', ['**/a,b/**', '**/{skip,also}/**']);
		useWorkspace({
			'/w/keep/Cargo.toml': '[dependencies]\nx = "1"\n',
			'/w/b/Cargo.toml': '[dependencies]\nx = "1"\n',
			'/w/a,b/Cargo.toml': '[dependencies]\nx = "2"\n',
			'/w/skip/Cargo.toml': '[dependencies]\nx = "3"\n',
			'/w/also/Cargo.toml': '[dependencies]\nx = "4"\n',
		});
		await runCommand('versions-le.compare');
		const text = report();
		expect(text).toContain('- `keep/Cargo.toml`');
		expect(text).toContain('- `b/Cargo.toml`');
		expect(text).not.toContain('a,b/');
		expect(text).not.toContain('skip/');
		expect(text).not.toContain('also/');
	});

	it('says so when there are no manifests at all', async () => {
		_setConfig('versions-le.notificationsLevel', 'all');
		useWorkspace({ '/w/README.md': '# hi' });
		await runCommand('versions-le.compare');
		expect(report()).toContain('No manifests found.');
		expect(
			_shownMessages().some((m) => m.message === 'No manifests found'),
		).toBe(true);
	});

	it('says every manifest could not be read, rather than that none was found', async () => {
		_setConfig('versions-le.notificationsLevel', 'all');
		useWorkspace({
			'/w/Cargo.toml': new Uint8Array([0xff, 0xfe]) as never,
			'/w/sub/go.mod': new Uint8Array([0xff]) as never,
		});
		await runCommand('versions-le.compare');
		const text = report();
		expect(text).toContain('## Could not be read (2)');
		expect(text).not.toContain('No manifests found.');
		const toasts = _shownMessages().map((m) => [m.kind, m.message]);
		expect(toasts).toContainEqual(['error', '2 manifest(s) could not be read']);
		expect(toasts).not.toContainEqual(['info', 'No manifests found']);
	});

	it('opens the report beside the editor, or in the active column when asked', async () => {
		useWorkspace(TREE);
		await runCommand('versions-le.compare');
		expect(_shownDocumentOptions().at(-1)).toEqual({
			preview: false,
			viewColumn: ViewColumn.Beside,
		});
		_setConfig('versions-le.openResultsSideBySide', false);
		await runCommand('versions-le.compare');
		expect(_shownDocumentOptions().at(-1)).toEqual({ preview: false });
	});

	it('leaves the clipboard alone unless asked', async () => {
		useWorkspace(TREE);
		await runCommand('versions-le.compare');
		expect(_clipboardText()).toBe('');
	});

	it('copies the report when asked to', async () => {
		_setConfig('versions-le.copyToClipboardEnabled', true);
		useWorkspace(TREE);
		await runCommand('versions-le.compare');
		expect(_clipboardText()).toBe(report());
	});

	it('names the file without its line when positions are off', async () => {
		useWorkspace(TREE);
		await runCommand('versions-le.compare');
		const lines = (report().match(/`[^`]+`:\d+ · /g) ?? []).length;
		expect(lines).toBeGreaterThan(0);

		_setConfig('versions-le.showPositions', false);
		await runCommand('versions-le.compare');
		expect(report()).not.toMatch(/`[^`]+`:\d+ · /);
		// Only the line number goes: every site is still listed, by file and key.
		expect((report().match(/^ {2}- `/gm) ?? []).length).toBeGreaterThanOrEqual(
			lines,
		);
	});

	it('says the results could not open, and announces nothing else', async () => {
		_setConfig('versions-le.notificationsLevel', 'all');
		useWorkspace(TREE);
		_setOpenDocumentError(new Error('the editor refused'));
		await runCommand('versions-le.compare');
		expect(_shownMessages().map((m) => [m.kind, m.message])).toEqual([
			['error', 'Could not open results'],
		]);
		expect(flashes).toEqual([]);
	});

	it('warns when the clipboard refuses, and the report is still open', async () => {
		_setConfig('versions-le.notificationsLevel', 'all');
		_setConfig('versions-le.copyToClipboardEnabled', true);
		useWorkspace(TREE);
		_setClipboardError(new Error('no clipboard here'));
		await runCommand('versions-le.compare');
		expect(report()).toContain('Versions-LE report');
		expect(_shownMessages().map((m) => [m.kind, m.message])).toContainEqual([
			'warning',
			'Could not copy to clipboard',
		]);
		expect(flashes.at(-1)).toMatch(/finding\(s\)$/);
	});

	it('decides positions for the clipboard separately from the report', async () => {
		_setConfig('versions-le.copyToClipboardEnabled', true);
		_setConfig('versions-le.clipboardIncludesPositions', false);
		useWorkspace(TREE);
		await runCommand('versions-le.compare');
		expect(report()).toMatch(/`[^`]+`:\d+ · /);
		expect(_clipboardText()).not.toMatch(/`[^`]+`:\d+ · /);
	});
});

describe('settings and help', () => {
	it('opens the settings filtered to this extension', async () => {
		const { deps } = makeDeps();
		registerOpenSettingsCommand(
			_createExtensionContext() as never,
			deps.telemetry,
		);
		await runCommand('versions-le.openSettings');
		expect(executedBuiltins.at(-1)).toMatchObject({ args: ['versions-le.'] });
	});

	it('opens the help, which names every finding and every refusal', async () => {
		const { deps } = makeDeps();
		registerHelpCommand(_createExtensionContext() as never, deps.telemetry);
		await runCommand('versions-le.help');
		for (const word of [
			'disjoint-constraint',
			'malformed-constraint',
			'constraint-conflict',
			'msrv-mismatch',
			'prerelease-in-production',
			'floating-pin',
			'unknown_grammar',
			'cross_ecosystem',
			'per_job_tool_version',
		]) {
			expect(generateHelpContent()).toContain(word);
		}
	});
});
