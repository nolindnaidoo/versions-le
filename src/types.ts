export type NotificationLevel = 'all' | 'important' | 'silent';

/** The extension's settings, read once per command and frozen. */
export interface Configuration {
	/** Whether the copy on the clipboard carries positions, whatever the screen shows. */
	readonly clipboardIncludesPositions: boolean;
	readonly copyToClipboardEnabled: boolean;
	readonly exclude: readonly string[];
	readonly notificationsLevel: NotificationLevel;
	readonly openResultsSideBySide: boolean;
	/** Whether the output gives the line each constraint is written on. */
	readonly showPositions: boolean;
	readonly statusBarEnabled: boolean;
	readonly telemetryEnabled: boolean;
}
