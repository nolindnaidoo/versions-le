import * as vscode from 'vscode';
import type { Configuration, NotificationLevel } from '../types';

/**
 * The defaults, exported for the parity gate: `config.test.ts` asserts they
 * match every default declared in package.json, which is what stops the two
 * drifting apart.
 */
export const CONFIG_DEFAULTS = Object.freeze({
	copyToClipboardEnabled: false,
	exclude: [] as const,
	notificationsLevel: 'silent' as const,
	openResultsSideBySide: true,
	statusBarEnabled: true,
	telemetryEnabled: false,
});

export function readConfig(): Configuration {
	const config = vscode.workspace.getConfiguration('versions-le');
	return Object.freeze({
		copyToClipboardEnabled: readBoolean(
			config,
			'copyToClipboardEnabled',
			CONFIG_DEFAULTS.copyToClipboardEnabled,
		),
		exclude: readStrings(config, 'exclude'),
		notificationsLevel: readNotificationLevel(config),
		openResultsSideBySide: readBoolean(
			config,
			'openResultsSideBySide',
			CONFIG_DEFAULTS.openResultsSideBySide,
		),
		statusBarEnabled: readBoolean(
			config,
			'statusBar.enabled',
			CONFIG_DEFAULTS.statusBarEnabled,
		),
		telemetryEnabled: readBoolean(
			config,
			'telemetryEnabled',
			CONFIG_DEFAULTS.telemetryEnabled,
		),
	});
}

function readBoolean(
	config: vscode.WorkspaceConfiguration,
	key: string,
	defaultValue: boolean,
): boolean {
	const value = config.get(key, defaultValue);
	return typeof value === 'boolean' ? value : defaultValue;
}

/** A list of globs. Anything that is not a non-empty string is dropped rather than honoured. */
function readStrings(
	config: vscode.WorkspaceConfiguration,
	key: string,
): string[] {
	const raw = config.get<unknown>(key, []);
	return Array.isArray(raw)
		? raw.filter(
				(item): item is string =>
					typeof item === 'string' && item.trim() !== '',
			)
		: [];
}

export function isValidNotificationLevel(v: unknown): v is NotificationLevel {
	return v === 'all' || v === 'important' || v === 'silent';
}

function readNotificationLevel(
	config: vscode.WorkspaceConfiguration,
): NotificationLevel {
	const raw = config.get<string>(
		'notificationsLevel',
		CONFIG_DEFAULTS.notificationsLevel,
	);
	return isValidNotificationLevel(raw)
		? raw
		: CONFIG_DEFAULTS.notificationsLevel;
}
