import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
	_createExtensionContext,
	_openedDocuments,
	_registeredCommands,
	_resetMockState,
	_setConfig,
	_setWorkspaceFiles,
	_shownMessages,
	_statusBarItems,
	executedBuiltins,
	Uri,
	workspace,
} from './__mocks__/vscode';
import { activate } from './extension';

/**
 * Every command, started a second time while the first is suspended.
 *
 * The policy the code implements is that each run is independent: no command
 * holds state another run reads, so two runs deliver two results. What they
 * share is the status bar, which the later run resets. Neither corrupts a
 * result, and both ends are asserted here.
 */

function command(id: string): (...args: unknown[]) => Promise<void> {
	const handler = _registeredCommands().get(id);
	if (!handler) throw new Error(`command not registered: ${id}`);
	return async (...args) => {
		await handler(...args);
	};
}

const IDLE = '$(versions) Versions-LE';

beforeEach(() => {
	_resetMockState();
	_setConfig('versions-le.notificationsLevel', 'all');
	_setWorkspaceFiles({
		'/w/api/Cargo.toml': '[dependencies]\nregex = "1"\n',
		'/w/web/Cargo.toml': '[dependencies]\nregex = "2"\n',
	});
	workspace.workspaceFolders = [{ uri: Uri.file('/w'), name: 'w', index: 0 }];
	activate(_createExtensionContext() as never);
});

describe('two invocations at once', () => {
	it('compare: both deliver their report, and the bar ends idle', async () => {
		vi.useFakeTimers();
		try {
			const compare = command('versions-le.compare');
			await Promise.all([compare(), compare(Uri.file('/w'))]);
			const reports = _openedDocuments().map((d) => d.getText());
			expect(reports).toHaveLength(2);
			for (const report of reports) {
				expect(report).toContain('**disjoint-constraint** · cargo · `regex`');
			}
			expect(_shownMessages().filter((m) => m.kind === 'warning')).toHaveLength(
				2,
			);
			expect(_statusBarItems()[0]?.text).toBe('$(eye) 1 finding(s)');
			vi.advanceTimersByTime(2000);
			expect(_statusBarItems()[0]?.text).toBe(IDLE);
			expect(vi.getTimerCount()).toBe(0);
		} finally {
			vi.useRealTimers();
		}
	});

	it('compare on the workspace and on a folder: two reports, each its own', async () => {
		await Promise.all([
			command('versions-le.compare')(),
			command('versions-le.compare')(Uri.file('/w/api')),
		]);
		const reports = _openedDocuments().map((d) => d.getText());
		expect(reports).toHaveLength(2);
		expect(reports.filter((r) => r.includes('2 manifest(s)'))).toHaveLength(1);
		expect(reports.filter((r) => r.includes('1 manifest(s)'))).toHaveLength(1);
	});

	it('help and openSettings: two documents, two settings calls', async () => {
		await Promise.all([
			command('versions-le.help')(),
			command('versions-le.help')(),
			command('versions-le.openSettings')(),
			command('versions-le.openSettings')(),
		]);
		expect(_openedDocuments()).toHaveLength(2);
		expect(executedBuiltins).toHaveLength(2);
	});
});
