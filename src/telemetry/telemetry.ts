import * as vscode from 'vscode';
import { readConfig } from '../config/config';

export interface Telemetry {
	event(name: string, properties?: Record<string, string>): void;
	dispose(): void;
}

export function createTelemetry(): Telemetry {
	let outputChannel: vscode.OutputChannel | undefined;

	function getOutputChannel(): vscode.OutputChannel {
		if (!outputChannel) {
			outputChannel = vscode.window.createOutputChannel('Versions-LE');
		}
		return outputChannel;
	}

	return Object.freeze({
		event(name: string, properties?: Record<string, string>): void {
			const config = readConfig();
			if (!config.telemetryEnabled) return;

			const channel = getOutputChannel();
			const timestamp = new Date().toISOString();
			const propsStr = properties ? ` ${JSON.stringify(properties)}` : '';
			channel.appendLine(`[${timestamp}] ${name}${propsStr}`);
		},
		dispose(): void {
			outputChannel?.dispose();
			outputChannel = undefined;
		},
	});
}
