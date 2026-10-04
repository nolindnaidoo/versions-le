import * as vscode from 'vscode';
import type { Telemetry } from '../telemetry/telemetry';
import type { Notifier } from '../ui/notifier';
import type { RatingPrompt } from '../ui/ratingPrompt';
import type { StatusBar } from '../ui/statusBar';
import { compareWorkspace } from './compare';

export interface CommandDependencies {
	notifier: Notifier;
	ratingPrompt: RatingPrompt;
	statusBar: StatusBar;
	telemetry: Telemetry;
}

export function registerCommands(
	context: vscode.ExtensionContext,
	deps: CommandDependencies,
): void {
	context.subscriptions.push(
		// The Explorer passes the folder it was invoked on; the palette and the
		// keybinding pass nothing, or an editor's own argument, which is not a folder.
		vscode.commands.registerCommand(
			'versions-le.compare',
			async (picked?: unknown) =>
				compareWorkspace(deps, isUri(picked) ? picked : undefined),
		),
	);
}

function isUri(value: unknown): value is vscode.Uri {
	return (
		typeof value === 'object' &&
		value !== null &&
		typeof (value as vscode.Uri).scheme === 'string' &&
		typeof (value as vscode.Uri).path === 'string'
	);
}
