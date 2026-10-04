import {
	disjoint,
	floorOf,
	malformedReason,
	mentionsPrerelease,
	type Range,
	rangeOf,
	rangesEqual,
} from './grammar';
import { ECOSYSTEMS, type Ecosystem, ecosystemOrder } from './heuristics';
import { type Entry, isDevelopment, type Refusal, type Site } from './parser';
import {
	compareStrings,
	compareVersions,
	formatVersion,
	type Version,
} from './semver';

/**
 * The checks — the crate's `compare.rs`. Every grouping is by ecosystem **and**
 * name, ordered as the crate's `BTreeMap` orders it.
 */

export interface Finding {
	readonly code: string;
	readonly severity: 'error' | 'warning' | 'info';
	readonly ecosystem: Ecosystem;
	readonly name: string;
	readonly message: string;
	readonly sites: Site[];
}

const rank = (severity: string) =>
	severity === 'error' ? 0 : severity === 'warning' ? 1 : 2;

export function analyse(entries: readonly Entry[]): {
	findings: Finding[];
	refusals: Refusal[];
} {
	const findings = [
		...conflicts(entries),
		...msrv(entries),
		...prereleases(entries),
		...malformed(entries),
		...floating(entries),
	];
	findings.sort(
		(a, b) =>
			rank(a.severity) - rank(b.severity) ||
			compareStrings(a.code, b.code) ||
			ecosystemOrder(a.ecosystem) - ecosystemOrder(b.ecosystem) ||
			compareStrings(a.name, b.name),
	);
	return {
		findings,
		refusals: [...crossEcosystem(entries), ...perJobToolVersions(entries)],
	};
}

function groupByName(
	entries: readonly Entry[],
	keep: (entry: Entry) => boolean,
): Array<[Ecosystem, string, Entry[]]> {
	const groups = new Map<string, [Ecosystem, string, Entry[]]>();
	for (const entry of entries.filter(keep)) {
		const id = `${entry.ecosystem}\u0000${entry.name}`;
		const group = groups.get(id);
		if (group) group[2].push(entry);
		else groups.set(id, [entry.ecosystem, entry.name, [entry]]);
	}
	return [...groups.values()].sort(
		(a, b) =>
			ecosystemOrder(a[0]) - ecosystemOrder(b[0]) || compareStrings(a[1], b[1]),
	);
}

const sameSite = (a: Site, b: Site) =>
	a.file === b.file &&
	a.key === b.key &&
	a.constraint === b.constraint &&
	a.line === b.line;

function sites(entries: readonly Entry[]): Site[] {
	const sorted = entries
		.map((entry) => entry.site)
		.sort(
			(a, b) => compareStrings(a.file, b.file) || compareStrings(a.key, b.key),
		);
	return sorted.filter(
		(one, index) => index === 0 || !sameSite(one, sorted[index - 1] as Site),
	);
}

function distinctConstraints(entries: readonly Entry[]): string[] {
	const sorted = entries
		.map((entry) => entry.site.constraint)
		.sort(compareStrings);
	return sorted.filter(
		(one, index) => index === 0 || one !== sorted[index - 1],
	);
}

function distinctRanges(entries: readonly Entry[]): number {
	const seen: Range[] = [];
	for (const entry of entries) {
		const r = rangeOf(entry.constraint);
		if (r !== undefined && !seen.some((held) => rangesEqual(held, r)))
			seen.push(r);
	}
	return seen.length;
}

const distinctFiles = (entries: readonly Entry[]) =>
	new Set(entries.map((entry) => entry.site.file)).size;
const perJob = (entry: Entry) =>
	entry.ecosystem === 'ci' && entry.kind === 'tool';

function conflicts(entries: readonly Entry[]): Finding[] {
	const comparable = (entry: Entry) =>
		entry.kind !== 'msrv' &&
		!perJob(entry) &&
		rangeOf(entry.constraint) !== undefined;
	const out: Finding[] = [];
	for (const [ecosystem, name, group] of groupByName(entries, comparable)) {
		if (distinctRanges(group) < 2) continue;
		const constraints = distinctConstraints(group);
		const clash = firstDisjointPair(group);
		out.push(
			clash
				? {
						code: 'disjoint-constraint',
						severity: 'error',
						ecosystem,
						name,
						message: `"${clash[0].site.constraint}" (${clash[0].site.file}) and "${clash[1].site.constraint}" (${clash[1].site.file}) cannot both be satisfied by one version`,
						sites: sites(group),
					}
				: {
						code: 'constraint-conflict',
						severity: 'warning',
						ecosystem,
						name,
						message: `constrained ${constraints.length} different ways across ${distinctFiles(group)} files: ${constraints.join(', ')}`,
						sites: sites(group),
					},
		);
	}
	return out;
}

function perJobToolVersions(entries: readonly Entry[]): Refusal[] {
	return groupByName(
		entries,
		(entry) => perJob(entry) && rangeOf(entry.constraint) !== undefined,
	)
		.filter(([, , group]) => distinctRanges(group) > 1)
		.map(([ecosystem, name, group]) => ({
			reason: 'per_job_tool_version',
			ecosystem,
			name,
			message: `installed as ${distinctConstraints(group).join(' and ')} by different jobs; a CI tool version belongs to the job that installs it, so these were not compared`,
			sites: sites(group),
		}));
}

function firstDisjointPair(
	group: readonly Entry[],
): [Entry, Entry] | undefined {
	for (let i = 0; i < group.length; i++) {
		for (let j = i + 1; j < group.length; j++) {
			const [left, right] = [group[i] as Entry, group[j] as Entry];
			const [one, other] = [
				rangeOf(left.constraint),
				rangeOf(right.constraint),
			];
			if (one !== undefined && other !== undefined && disjoint(one, other))
				return [left, right];
		}
	}
	return undefined;
}

function msrv(entries: readonly Entry[]): Finding[] {
	const declared = entries.filter(
		(entry) => entry.kind === 'msrv' && entry.ecosystem === 'cargo',
	);
	const findings: Finding[] = [];
	if (distinctRanges(declared) > 1) {
		const constraints = distinctConstraints(declared);
		findings.push({
			code: 'msrv-mismatch',
			severity: 'warning',
			ecosystem: 'cargo',
			name: 'rust',
			message: `rust-version is declared ${constraints.length} different ways: ${constraints.join(', ')}`,
			sites: sites(declared),
		});
	}
	// `max_by` keeps the last of equal maxima.
	let highest: [Version, Entry] | undefined;
	for (const entry of declared) {
		const r = rangeOf(entry.constraint);
		const floor = r === undefined ? undefined : floorOf(r);
		if (floor === undefined) continue;
		if (highest === undefined || compareVersions(floor, highest[0]) >= 0)
			highest = [floor, entry];
	}
	if (highest === undefined) return findings;
	for (const pin of entries.filter(
		(entry) => entry.kind === 'msrv' && entry.ecosystem === 'ci',
	)) {
		const r = rangeOf(pin.constraint);
		const floor = r === undefined ? undefined : floorOf(r);
		if (floor === undefined || compareVersions(floor, highest[0]) >= 0)
			continue;
		findings.push({
			code: 'msrv-mismatch',
			severity: 'warning',
			ecosystem: 'ci',
			name: 'rust',
			message: `CI builds on ${formatVersion(floor)}, below the declared minimum ${formatVersion(highest[0])} in ${highest[1].site.file}`,
			sites: sites([pin, highest[1]]),
		});
	}
	return findings;
}

function prereleases(entries: readonly Entry[]): Finding[] {
	const shipped = (entry: Entry) => {
		const r = rangeOf(entry.constraint);
		return (
			!isDevelopment(entry.kind) && r !== undefined && mentionsPrerelease(r)
		);
	};
	return groupByName(entries, shipped).map(([ecosystem, name, group]) => ({
		code: 'prerelease-in-production',
		severity: 'warning',
		ecosystem,
		name,
		message: `a prerelease constraint outside dev dependencies: ${distinctConstraints(group).join(', ')}`,
		sites: sites(group),
	}));
}

function malformed(entries: readonly Entry[]): Finding[] {
	const out: Finding[] = [];
	for (const [ecosystem, name, group] of groupByName(
		entries,
		(entry) => malformedReason(entry.constraint) !== undefined,
	)) {
		const reason = group
			.map((entry) => malformedReason(entry.constraint))
			.find((one) => one !== undefined);
		if (reason === undefined) continue;
		out.push({
			code: 'malformed-constraint',
			severity: 'error',
			ecosystem,
			name,
			message: `${reason}: ${distinctConstraints(group).join(', ')}`,
			sites: sites(group),
		});
	}
	return out;
}

function floating(entries: readonly Entry[]): Finding[] {
	const out: Finding[] = [];
	for (const [ecosystem, name, group] of groupByName(
		entries,
		(entry) => entry.floats !== undefined,
	)) {
		const why = group
			.map((entry) => entry.floats)
			.find((one) => one !== undefined);
		if (why === undefined) continue;
		out.push({
			code: 'floating-pin',
			severity: 'info',
			ecosystem,
			name,
			message: why,
			sites: sites(group),
		});
	}
	return out;
}

function crossEcosystem(entries: readonly Entry[]): Refusal[] {
	const byName = new Map<string, Map<Ecosystem, Entry>>();
	for (const entry of entries.filter((one) => one.kind !== 'msrv')) {
		let seen = byName.get(entry.name);
		if (!seen) {
			seen = new Map();
			byName.set(entry.name, seen);
		}
		if (!seen.has(entry.ecosystem)) seen.set(entry.ecosystem, entry);
	}
	return [...byName.entries()]
		.sort(([a], [b]) => compareStrings(a, b))
		.filter(([, seen]) => seen.size > 1)
		.map(([name, seen]) => {
			const ordered = ECOSYSTEMS.filter((ecosystem) => seen.has(ecosystem));
			return {
				reason: 'cross_ecosystem',
				ecosystem: null,
				name,
				message: `appears in ${ordered.join(' and ')}; different ecosystems name different things, so these were not compared`,
				sites: sites(ordered.map((ecosystem) => seen.get(ecosystem) as Entry)),
			};
		});
}
