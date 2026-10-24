import * as vscode from 'vscode';
import { readConfig } from '../config/config';

export interface StatusBar {
	flash(text: string): void;
}

const IDLE = '$(versions) Versions-LE';

export function createStatusBar(context: vscode.ExtensionContext): StatusBar {
	const item = vscode.window.createStatusBarItem(
		vscode.StatusBarAlignment.Left,
		100,
	);
	item.text = IDLE;
	item.tooltip = vscode.l10n.t('Run Versions-LE: Compare Versions');
	item.command = 'versions-le.compare';
	context.subscriptions.push(item);

	function updateVisibility(): void {
		if (readConfig().statusBarEnabled) item.show();
		else item.hide();
	}

	updateVisibility();
	context.subscriptions.push(
		vscode.workspace.onDidChangeConfiguration((e) => {
			if (e.affectsConfiguration('versions-le.statusBar.enabled'))
				updateVisibility();
		}),
	);

	let hideTimer: NodeJS.Timeout | undefined;
	// A command that finishes after deactivation must not write to the disposed
	// item or set a timer that outlives the extension.
	let disposed = false;
	context.subscriptions.push({
		dispose(): void {
			disposed = true;
			if (hideTimer) clearTimeout(hideTimer);
			hideTimer = undefined;
		},
	});

	return Object.freeze({
		flash(text: string): void {
			// A short-lived status text instead of a notification.
			if (disposed || !readConfig().statusBarEnabled) return;
			item.text = `$(eye) ${text}`;
			if (hideTimer) clearTimeout(hideTimer);
			hideTimer = setTimeout(() => {
				item.text = IDLE;
				hideTimer = undefined;
			}, 2000);
		},
	});
}
