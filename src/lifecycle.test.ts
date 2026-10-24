import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
	_createExtensionContext,
	_live,
	_outputChannels,
	_registeredCommands,
	_resetMockState,
	_setConfig,
	_setLmApi,
	_setWorkspaceFiles,
	_statusBarItems,
	Uri,
	workspace,
} from './__mocks__/vscode';
import { activate } from './extension';
import { createTelemetry } from './telemetry/telemetry';
import { createStatusBar } from './ui/statusBar';

/**
 * What the extension holds, and whether it lets go.
 *
 * An extension host stays up for days, so a resource created per command and
 * never released is a real leak, and a deactivation that leaves a listener or
 * a timer behind is one too. The mock counts what is alive: created and not
 * disposed, registered and not removed.
 */

async function run(id: string, ...args: unknown[]): Promise<void> {
	const handler = _registeredCommands().get(id);
	if (!handler) throw new Error(`command not registered: ${id}`);
	await handler(...args);
}

function openWorkspace(): void {
	_setWorkspaceFiles({
		'/w/api/Cargo.toml': '[dependencies]\nregex = "1"\n',
		'/w/web/Cargo.toml': '[dependencies]\nregex = "2"\n',
	});
	workspace.workspaceFolders = [{ uri: Uri.file('/w'), name: 'w', index: 0 }];
}

beforeEach(() => {
	_resetMockState();
	_setConfig('versions-le.telemetryEnabled', true);
	_setLmApi(true);
});

describe('dispose is complete and safe to repeat', () => {
	it('status bar', () => {
		const context = _createExtensionContext();
		createStatusBar(context as never);
		for (const disposable of context.subscriptions) disposable.dispose();
		expect(_statusBarItems()[0]?.disposed).toBe(true);
		expect(_live().configListeners).toBe(0);
		expect(() => {
			for (const disposable of context.subscriptions) disposable.dispose();
		}).not.toThrow();
	});

	it('telemetry channel', () => {
		const telemetry = createTelemetry();
		telemetry.event('x');
		telemetry.dispose();
		expect(_outputChannels()[0]?.disposed).toBe(true);
		expect(() => telemetry.dispose()).not.toThrow();
	});

	it('a flash after dispose schedules nothing', () => {
		// A command finishing after deactivation flashed the disposed item and
		// set a restore timer that outlived the extension.
		vi.useFakeTimers();
		try {
			const context = _createExtensionContext();
			const statusBar = createStatusBar(context as never);
			for (const disposable of context.subscriptions) disposable.dispose();
			statusBar.flash('3 finding(s)');
			expect(vi.getTimerCount()).toBe(0);
			expect(_statusBarItems()[0]?.text).toBe('$(versions) Versions-LE');
		} finally {
			vi.useRealTimers();
		}
	});
});

describe('deactivation leaves nothing behind', () => {
	it('after every command has run once', async () => {
		openWorkspace();
		const context = _createExtensionContext();
		activate(context as never);
		await run('versions-le.compare');
		await run('versions-le.compare', Uri.file('/w/api'));
		await run('versions-le.openSettings');
		await run('versions-le.help');
		expect(_live()).toEqual({
			statusBarItems: 1,
			outputChannels: 1,
			configListeners: 1,
			commands: 3,
			mcpProviders: 1,
		});

		for (const disposable of context.subscriptions) disposable.dispose();
		expect(_live()).toEqual({
			statusBarItems: 0,
			outputChannels: 0,
			configListeners: 0,
			commands: 0,
			mcpProviders: 0,
		});
	});
});

describe('repeated runs do not grow', () => {
	it('fifty compares, fifty help commands', async () => {
		openWorkspace();
		activate(_createExtensionContext() as never);
		await run('versions-le.compare');
		await run('versions-le.help');
		const afterOne = _live();
		for (let i = 0; i < 49; i++) {
			await run('versions-le.compare');
			await run('versions-le.help');
		}
		expect(_live()).toEqual(afterOne);
	});
});
