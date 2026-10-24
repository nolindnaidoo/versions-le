/**
 * The crate's `scan::scan`: discover the manifests under each root, read them,
 * and build one report, with every manifest that could not be read named in
 * its diagnostics.
 */
import type { Ecosystem } from '../detect/heuristics';
import type { Refusal, Site } from '../detect/parser';
import { type Document, type Report, reportFor } from '../detect/report';
import type { FailOn } from './args';
import {
	type DiscoverOptions,
	discover,
	label,
	nameOf,
	read,
} from './discover';

export function scan(
	roots: readonly string[],
	options: DiscoverOptions,
): Report {
	const qualify = roots.length > 1;
	const documents: Document[] = [];
	const unread: [string, string][] = [];
	for (const root of roots) {
		for (const found of discover(root, options)) {
			try {
				const content = read(found.path);
				documents.push({
					kind: found.kind,
					label: label(found, root, qualify),
					content,
				});
			} catch (error) {
				unread.push([nameOf(found.path), (error as Error).message]);
			}
		}
	}
	const report = reportFor(documents);
	const diagnostics = [
		...report.diagnostics,
		...unread.map(([file, message]) => ({
			severity: 'error' as const,
			code: 'unreadable',
			file,
			message,
		})),
	];
	const count = (severity: string) =>
		report.findings.filter((finding) => finding.severity === severity).length +
		diagnostics.filter((diagnostic) => diagnostic.severity === severity).length;
	return {
		...report,
		diagnostics,
		summary: {
			...report.summary,
			errors: count('error'),
			warnings: count('warning'),
			infos: count('info'),
		},
	};
}

/** The two refusals that are an answer rather than a limit: neither is unanalysed. */
function isTheAnswer(refusal: Refusal): boolean {
	return (
		refusal.reason === 'cross_ecosystem' ||
		refusal.reason === 'per_job_tool_version'
	);
}

export function exitCode(
	report: Report,
	failOn: FailOn,
	strict: boolean,
): number {
	const unanalysed =
		report.diagnostics.length > 0 ||
		report.refusals.some((refusal) => !isTheAnswer(refusal));
	if (strict && unanalysed) return 2;
	const failing = report.findings.some(
		(finding) => failOn === 'any' || finding.severity !== 'info',
	);
	return failing ? 1 : 0;
}

export function summary(report: Report): string {
	const lines: string[] = [];
	for (const diagnostic of report.diagnostics)
		lines.push(`${diagnostic.file}: ${diagnostic.message}`);
	for (const finding of report.findings) {
		lines.push(
			`${finding.severity} ${finding.ecosystem}: ${finding.name} (${finding.message}) [${files(finding.sites)}]`,
		);
	}
	for (const refusal of report.refusals) {
		lines.push(
			`refused ${refusal.reason} ${refusal.name}: ${refusal.message} [${files(refusal.sites)}]`,
		);
	}
	lines.push(verdict(report.summary, report.diagnostics.length));
	return `${lines.join('\n')}\n`;
}

/** The crate's last line: a run in which every manifest failed to read is not one that found none. */
function verdict(summary: Report['summary'], unread: number): string {
	if (summary.manifests === 0) {
		return unread > 0 ? 'no manifests could be read' : 'no manifests found';
	}
	if (summary.findings === 0) {
		return `consistent — ${plural(summary.entries, 'constraint', 'constraints')} across ${plural(summary.manifests, 'manifest', 'manifests')}`;
	}
	return `${plural(summary.findings, 'finding', 'findings')} across ${plural(summary.manifests, 'manifest', 'manifests')} — ${summary.errors} error, ${summary.warnings} warning, ${summary.infos} info`;
}

/** Each file once, in the order the sites first name it. */
function files(sites: readonly Site[]): string {
	return [...new Set(sites.map((site) => site.file))].join(', ');
}

function plural(count: number, one: string, many: string): string {
	return `${count} ${count === 1 ? one : many}`;
}

export type { Ecosystem };
