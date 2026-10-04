export type NotificationLevel = 'all' | 'important' | 'silent';

/** The extension's settings, read once per command and frozen. */
export interface Configuration {
	readonly copyToClipboardEnabled: boolean;
	readonly exclude: readonly string[];
	readonly notificationsLevel: NotificationLevel;
	readonly openResultsSideBySide: boolean;
	readonly statusBarEnabled: boolean;
	readonly telemetryEnabled: boolean;
}
