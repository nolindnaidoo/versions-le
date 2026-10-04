import * as vscode from 'vscode';
import type { Site } from '../detect/parser';
import type { Report } from '../detect/report';

/**
 * The report a person reads, as Markdown: findings by severity, each with every
 * site that produced it, then what was deliberately not compared and why, then
 * the manifests that could not be read.
 */
export function formatReport(report: Report): string {
	const { summary } = report;
	const lines: string[] = [`# ${vscode.l10n.t('Versions-LE report')}`, ''];
	lines.push(
		vscode.l10n.t(
			'{0} manifest(s), {1} constraint(s): {2} error(s), {3} warning(s), {4} info, {5} not compared',
			summary.manifests,
			summary.entries,
			summary.errors,
			summary.warnings,
			summary.infos,
			summary.refusals,
		),
		'',
	);
	if (summary.manifests === 0) {
		lines.push(vscode.l10n.t('No manifests found.'), '');
		return lines.join('\n');
	}

	for (const severity of ['error', 'warning', 'info'] as const) {
		const findings = report.findings.filter(
			(finding) => finding.severity === severity,
		);
		if (findings.length === 0) continue;
		lines.push(`## ${heading(severity)} (${findings.length})`, '');
		for (const finding of findings) {
			lines.push(
				`- **${finding.code}** · ${finding.ecosystem} · ${code(finding.name)}: ${finding.message}`,
			);
			for (const one of finding.sites) lines.push(`  - ${site(one)}`);
		}
		lines.push('');
	}
	if (report.findings.length === 0)
		lines.push(vscode.l10n.t('No version problems found.'), '');

	if (report.refusals.length > 0) {
		lines.push(
			`## ${vscode.l10n.t('Not compared ({0})', report.refusals.length)}`,
			'',
		);
		for (const refusal of report.refusals) {
			lines.push(
				`- **${refusal.reason}** · ${refusal.ecosystem ?? '—'} · ${code(refusal.name)}: ${refusal.message}`,
			);
			for (const one of refusal.sites) lines.push(`  - ${site(one)}`);
		}
		lines.push('');
	}

	if (report.diagnostics.length > 0) {
		lines.push(
			`## ${vscode.l10n.t('Could not be read ({0})', report.diagnostics.length)}`,
			'',
		);
		for (const diagnostic of report.diagnostics)
			lines.push(`- ${code(diagnostic.file)}: ${diagnostic.message}`);
		lines.push('');
	}

	lines.push(`## ${vscode.l10n.t('Manifests')}`, '');
	for (const manifest of report.manifests)
		lines.push(
			`- ${code(manifest.path)} · ${manifest.ecosystem} · ${manifest.entries}`,
		);
	lines.push('');
	return lines.join('\n');
}

function heading(severity: 'error' | 'warning' | 'info'): string {
	if (severity === 'error') return vscode.l10n.t('Errors');
	if (severity === 'warning') return vscode.l10n.t('Warnings');
	return vscode.l10n.t('Info');
}

/** Where a constraint was written: file, key, the constraint itself, and a line where the reader knows it. */
function site(one: Site): string {
	const where =
		one.line === undefined ? code(one.file) : `${code(one.file)}:${one.line}`;
	const constraint = one.constraint === '' ? '' : ` = ${code(one.constraint)}`;
	return `${where} · ${code(one.key)}${constraint}`;
}

/** Text as a code span. A code span cannot escape a backtick, so one becomes a quote. */
function code(text: string): string {
	return `\`${text.replace(/`/g, "'").replace(/\r?\n/g, ' ')}\``;
}
