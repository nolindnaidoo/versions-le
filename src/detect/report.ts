import { analyse, type Finding } from './compare';
import { ECOSYSTEM_OF, type Ecosystem, type ManifestKind } from './heuristics';
import { type Entry, parse, type Refusal } from './parser';
import { compareStrings } from './semver';

/** The crate's `scan::report_for`: every manifest in hand, compared as one set. */

export interface Document {
	readonly kind: ManifestKind;
	readonly label: string;
	readonly content: string;
}

export interface Diagnostic {
	readonly severity: 'error';
	readonly code: string;
	readonly file: string;
	readonly message: string;
}

export interface Report {
	readonly schema: 1;
	readonly manifests: Array<{
		readonly path: string;
		readonly ecosystem: Ecosystem;
		readonly entries: number;
	}>;
	readonly findings: Finding[];
	readonly refusals: Refusal[];
	readonly diagnostics: Diagnostic[];
	readonly summary: {
		readonly manifests: number;
		readonly entries: number;
		readonly findings: number;
		readonly refusals: number;
		readonly errors: number;
		readonly warnings: number;
		readonly infos: number;
	};
}

export function reportFor(documents: readonly Document[]): Report {
	const entries: Entry[] = [];
	const refusals: Refusal[] = [];
	const diagnostics: Diagnostic[] = [];
	const manifests: Report['manifests'] = [];
	for (const document of documents) {
		const parsed = parse(document.kind, document.label, document.content);
		manifests.push({
			path: document.label,
			ecosystem: ECOSYSTEM_OF[document.kind],
			entries: parsed.entries.length,
		});
		for (const message of parsed.errors)
			diagnostics.push({
				severity: 'error',
				code: 'parse-error',
				file: document.label,
				message,
			});
		entries.push(...parsed.entries);
		refusals.push(...parsed.refusals);
	}
	const analysed = analyse(entries);
	const merged = mergeRefusals([...refusals, ...analysed.refusals]);
	const count = (severity: string) =>
		analysed.findings.filter((finding) => finding.severity === severity)
			.length + diagnostics.filter((d) => d.severity === severity).length;
	return {
		schema: 1,
		manifests,
		findings: analysed.findings,
		refusals: merged,
		diagnostics,
		summary: {
			manifests: manifests.length,
			entries: entries.length,
			findings: analysed.findings.length,
			refusals: merged.length,
			errors: count('error'),
			warnings: count('warning'),
			infos: count('info'),
		},
	};
}

function mergeRefusals(refusals: readonly Refusal[]): Refusal[] {
	const merged: Refusal[] = [];
	for (const refusal of refusals) {
		const same = merged.find(
			(held) =>
				held.reason === refusal.reason &&
				held.name === refusal.name &&
				held.ecosystem === refusal.ecosystem &&
				held.message === refusal.message,
		);
		if (same) same.sites.push(...refusal.sites);
		else merged.push({ ...refusal, sites: [...refusal.sites] });
	}
	return merged.sort(
		(a, b) =>
			compareStrings(a.reason, b.reason) || compareStrings(a.name, b.name),
	);
}
