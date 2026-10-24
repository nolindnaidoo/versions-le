import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	_createExtensionContext,
	_fireConfigChange,
	_resetMockState,
	_setConfig,
	_shownMessages,
	_statusBarItems,
} from '../__mocks__/vscode';
import { createNotifier } from './notifier';
import { createStatusBar } from './statusBar';

beforeEach(() => {
	_resetMockState();
});

describe('notifier levels', () => {
	it('silent (the default) suppresses info and warnings but shows errors', () => {
		const notifier = createNotifier();
		notifier.info('i');
		notifier.warn('w');
		notifier.error('e');
		expect(_shownMessages().map((m) => m.kind)).toEqual(['error']);
	});

	it('important shows warnings and errors, not info', () => {
		_setConfig('versions-le.notificationsLevel', 'important');
		const notifier = createNotifier();
		notifier.info('i');
		notifier.warn('w');
		notifier.error('e');
		expect(_shownMessages().map((m) => m.kind)).toEqual(['warning', 'error']);
	});

	it('all shows everything', () => {
		_setConfig('versions-le.notificationsLevel', 'all');
		const notifier = createNotifier();
		notifier.info('i');
		notifier.warn('w');
		notifier.error('e');
		expect(_shownMessages().map((m) => m.kind)).toEqual([
			'info',
			'warning',
			'error',
		]);
	});

	it('sanitizes every outgoing message', () => {
		const notifier = createNotifier();
		notifier.error('failed for /Users/alice/f with token=secret');
		expect(_shownMessages()[0]?.message).toBe(
			'failed for /Users/***/f with token=***',
		);
	});
});

describe('status bar', () => {
	afterEach(() => {
		vi.useRealTimers();
	});

	const IDLE = '$(versions) Versions-LE';

	it('shows by default, hides when the setting turns off, and shows again when it turns on', () => {
		createStatusBar(_createExtensionContext() as never);
		const item = _statusBarItems()[0];
		expect(item).toMatchObject({ text: IDLE, visible: true });
		expect(item?.tooltip).toBe('Run Versions-LE: Compare Versions');

		_setConfig('versions-le.statusBar.enabled', false);
		_fireConfigChange('versions-le.statusBar.enabled');
		expect(item?.visible).toBe(false);

		_setConfig('versions-le.statusBar.enabled', true);
		_fireConfigChange('versions-le.statusBar.enabled');
		expect(item?.visible).toBe(true);
	});

	it('starts hidden when the setting is off', () => {
		_setConfig('versions-le.statusBar.enabled', false);
		createStatusBar(_createExtensionContext() as never);
		expect(_statusBarItems()[0]?.visible).toBe(false);
	});

	it('shows a flash for two seconds, then its idle text again', () => {
		vi.useFakeTimers();
		const statusBar = createStatusBar(_createExtensionContext() as never);
		statusBar.flash('3 finding(s)');
		const item = _statusBarItems()[0];
		expect(item?.text).toBe('$(eye) 3 finding(s)');
		vi.advanceTimersByTime(1999);
		expect(item?.text).toBe('$(eye) 3 finding(s)');
		vi.advanceTimersByTime(1);
		expect(item?.text).toBe(IDLE);
		expect(vi.getTimerCount()).toBe(0);
	});

	it('a second flash restarts the clock rather than racing the first', () => {
		vi.useFakeTimers();
		const statusBar = createStatusBar(_createExtensionContext() as never);
		statusBar.flash('first');
		vi.advanceTimersByTime(1500);
		statusBar.flash('second');
		vi.advanceTimersByTime(1500);
		expect(_statusBarItems()[0]?.text).toBe('$(eye) second');
		expect(vi.getTimerCount()).toBe(1);
		vi.advanceTimersByTime(500);
		expect(_statusBarItems()[0]?.text).toBe(IDLE);
	});

	it('flash is a no-op when the status bar is disabled', () => {
		vi.useFakeTimers();
		_setConfig('versions-le.statusBar.enabled', false);
		const statusBar = createStatusBar(_createExtensionContext() as never);
		statusBar.flash('nope');
		expect(_statusBarItems()[0]?.text).toBe(IDLE);
		expect(vi.getTimerCount()).toBe(0);
	});
});
