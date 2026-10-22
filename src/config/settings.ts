import * as vscode from 'vscode';
import { guarded } from '../commands/guard';
import type { Telemetry } from '../telemetry/telemetry';
import { createNotifier } from '../ui/notifier';

// Command to navigate users directly to versions-le settings
export function registerOpenSettingsCommand(
	context: vscode.ExtensionContext,
	telemetry: Telemetry,
): void {
	context.subscriptions.push(
		vscode.commands.registerCommand(
			'versions-le.openSettings',
			guarded(createNotifier(), async () => {
				telemetry.event('command', { name: 'openSettings' });
				// Open Settings UI filtered by exact setting prefix to avoid unrelated matches
				await vscode.commands.executeCommand(
					'workbench.action.openSettings',
					'versions-le.',
				);
			}),
		),
	);
}
