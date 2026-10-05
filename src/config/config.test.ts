import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { _resetMockState, _setConfig } from '../__mocks__/vscode';
import {
	CONFIG_DEFAULTS,
	isValidNotificationLevel,
	readConfig,
} from './config';

describe('config defaults parity with package.json', () => {
	const manifest = JSON.parse(
		readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8'),
	) as {
		contributes: {
			configuration: { properties: Record<string, { default: unknown }> };
		};
	};
	const props = manifest.contributes.configuration.properties;
	const KEY_MAP: Record<string, keyof typeof CONFIG_DEFAULTS> = {
		'versions-le.clipboardIncludesPositions': 'clipboardIncludesPositions',
		'versions-le.copyToClipboardEnabled': 'copyToClipboardEnabled',
		'versions-le.exclude': 'exclude',
		'versions-le.notificationsLevel': 'notificationsLevel',
		'versions-le.openResultsSideBySide': 'openResultsSideBySide',
		'versions-le.showPositions': 'showPositions',
		'versions-le.statusBar.enabled': 'statusBarEnabled',
		'versions-le.telemetryEnabled': 'telemetryEnabled',
	};

	it('covers every declared setting', () => {
		expect(Object.keys(props).sort()).toEqual(Object.keys(KEY_MAP).sort());
	});

	for (const [manifestKey, defaultsKey] of Object.entries(KEY_MAP)) {
		it(`${manifestKey} default matches`, () => {
			expect(CONFIG_DEFAULTS[defaultsKey]).toEqual(props[manifestKey]?.default);
		});
	}
});

describe('readConfig', () => {
	afterEach(() => _resetMockState());

	it('keeps only non-empty strings from the exclude list', () => {
		_setConfig('versions-le.exclude', [
			'examples/**',
			3,
			'',
			'  ',
			'fixtures/**',
		]);
		expect(readConfig().exclude).toEqual(['examples/**', 'fixtures/**']);
		_setConfig('versions-le.exclude', 'examples/**');
		expect(readConfig().exclude).toEqual([]);
	});

	it('falls back to the default for a value of the wrong type', () => {
		_setConfig('versions-le.openResultsSideBySide', 'yes');
		expect(readConfig().openResultsSideBySide).toBe(true);
	});
});

describe('isValidNotificationLevel', () => {
	it('accepts the three declared levels and nothing else', () => {
		for (const level of ['all', 'important', 'silent'])
			expect(isValidNotificationLevel(level)).toBe(true);
		expect(isValidNotificationLevel('verbose')).toBe(false);
	});
});
