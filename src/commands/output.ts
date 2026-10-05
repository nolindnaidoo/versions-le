import * as vscode from 'vscode';
import type { Configuration } from '../types';
import type { CommandDependencies } from './index';

/** Open the report beside the editor, and copy it when the setting asks. */
export async function showReport(
	report: string,
	config: Configuration,
	deps: CommandDependencies,
	// The copy is its own text: whether it carries positions is a separate setting.
	forClipboard: string = report,
): Promise<void> {
	try {
		const document = await vscode.workspace.openTextDocument({
			content: report,
			language: 'markdown',
		});
		await vscode.window.showTextDocument(document, {
			preview: false,
			...(config.openResultsSideBySide
				? { viewColumn: vscode.ViewColumn.Beside }
				: {}),
		});
	} catch {
		deps.notifier.error(vscode.l10n.t('Could not open results'));
	}
	if (!config.copyToClipboardEnabled) return;
	// An unavailable clipboard must not fail the scan; the report is open.
	try {
		await vscode.env.clipboard.writeText(forClipboard);
	} catch {
		deps.notifier.warn(vscode.l10n.t('Could not copy to clipboard'));
	}
}
