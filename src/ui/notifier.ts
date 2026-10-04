import * as vscode from 'vscode';
import { readConfig } from '../config/config';
import { sanitizeErrorMessage } from '../utils/errors';

/**
 * All user notifications route through here so notificationsLevel
 * actually governs them: 'all' shows everything, 'important' shows
 * warnings and errors, 'silent' shows errors only. Every message is
 * sanitized before display.
 */
export interface Notifier {
	info(message: string): void;
	warn(message: string): void;
	error(message: string): void;
}

export function createNotifier(): Notifier {
	return Object.freeze({
		info(message: string): void {
			if (readConfig().notificationsLevel === 'all') {
				vscode.window.showInformationMessage(sanitizeErrorMessage(message));
			}
		},

		warn(message: string): void {
			if (readConfig().notificationsLevel !== 'silent') {
				vscode.window.showWarningMessage(sanitizeErrorMessage(message));
			}
		},

		error(message: string): void {
			vscode.window.showErrorMessage(sanitizeErrorMessage(message));
		},
	});
}
