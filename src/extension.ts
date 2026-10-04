import type * as vscode from 'vscode';
import { registerCommands } from './commands';
import { registerHelpCommand } from './commands/help';
import { registerOpenSettingsCommand } from './config/settings';
import { registerMcpProvider } from './mcp/provider';
import { createTelemetry } from './telemetry/telemetry';
import { createNotifier } from './ui/notifier';
import { createStatusBar } from './ui/statusBar';

export function activate(context: vscode.ExtensionContext): void {
	const telemetry = createTelemetry();
	const notifier = createNotifier();
	const statusBar = createStatusBar(context);

	// Register disposables
	context.subscriptions.push(telemetry);

	// Register all commands with utilities
	registerCommands(context, {
		telemetry,
		notifier,
		statusBar,
	});

	// Register settings and help commands
	registerOpenSettingsCommand(context, telemetry);
	registerHelpCommand(context, telemetry);

	// Offer the bundled MCP server to agent mode, where the host supports it
	registerMcpProvider(context);

	telemetry.event('extension-activated');
}

export function deactivate(): void {
	// Extensions are automatically disposed via context.subscriptions
}
