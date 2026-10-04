import { beforeEach, describe, expect, it } from 'vitest';
import {
	_clipboardText,
	_createDocument,
	_createExtensionContext,
	_openedDocuments,
	_registeredCommands,
	_resetMockState,
	_setActiveEditor,
	_setConfig,
	_setWorkspaceFiles,
	_shownMessages,
	executedBuiltins,
	Uri,
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
		deps: { notifier: createNotifier(), statusBar, telemetry },
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
		expect(text).toContain('- `go.mod`: not readable as UTF-8 text');
	});

	it('leaves out what the exclude setting names', async () => {
		_setConfig('versions-le.exclude', ['web/**']);
		useWorkspace(TREE);
		await runCommand('versions-le.compare');
		expect(report()).not.toContain('web/Cargo.toml');
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

	it('copies the report when asked to', async () => {
		_setConfig('versions-le.copyToClipboardEnabled', true);
		useWorkspace(TREE);
		await runCommand('versions-le.compare');
		expect(_clipboardText()).toBe(report());
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
