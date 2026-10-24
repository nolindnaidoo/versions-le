import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
	_createExtensionContext,
	_resetMockState,
	_statusBarItems,
} from './__mocks__/vscode';
import { createStatusBar } from './ui/statusBar';

/**
 * What the extension holds, and whether it lets go.
 *
 * An extension host stays up for days, so a resource created per command and
 * never released is a real leak, and a deactivation that leaves a listener or
 * a timer behind is one too. The mock counts what is alive: created and not
 * disposed, registered and not removed.
 */

beforeEach(() => {
	_resetMockState();
});

describe('dispose is complete and safe to repeat', () => {
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
