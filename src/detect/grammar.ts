import {
	type Comparator,
	compareVersions,
	parsePrerelease,
	parseVersion,
	parseVersionReq,
	type Version,
	versionsEqual,
} from './semver';
import { splitWhitespace, trim } from './text';

/**
 * The constraint grammars — the crate's `grammar.rs`: what a constraint string
 * means as a union of intervals on the version line, or why it is not
 * compared.
 */

interface Bound {
	readonly version: Version;
	readonly inclusive: boolean;
}

/** One stretch of the version line. `undefined` is unbounded. */
interface Interval {
	readonly lower: Bound | undefined;
	readonly upper: Bound | undefined;
}

export interface Range {
	readonly intervals: readonly Interval[];
}

export type Constraint =
	| { readonly kind: 'range'; readonly range: Range }
	| { readonly kind: 'unknown'; readonly reason: string }
	| { readonly kind: 'malformed'; readonly reason: string };

const range = (intervals: Interval[]): Constraint => ({
	kind: 'range',
	range: { intervals },
});
const unknown = (reason: string): Constraint => ({ kind: 'unknown', reason });
const malformed = (reason: string): Constraint => ({
	kind: 'malformed',
	reason,
});
const ANY: Interval = { lower: undefined, upper: undefined };

export const rangeOf = (constraint: Constraint): Range | undefined =>
	constraint.kind === 'range' ? constraint.range : undefined;
export const malformedReason = (constraint: Constraint): string | undefined =>
	constraint.kind === 'malformed' ? constraint.reason : undefined;

/** The lowest version the range admits; none when any alternative is open below. */
export function floorOf(r: Range): Version | undefined {
	if (r.intervals.some((interval) => interval.lower === undefined))
		return undefined;
	let lowest: Version | undefined;
	for (const interval of r.intervals) {
		const version = (interval.lower as Bound).version;
		if (lowest === undefined || compareVersions(version, lowest) < 0)
			lowest = version;
	}
	return lowest;
}

export function mentionsPrerelease(r: Range): boolean {
	return r.intervals.some((interval) =>
		[interval.lower, interval.upper].some(
			(bound) => bound !== undefined && bound.version.pre !== '',
		),
	);
}

const boundsEqual = (a: Bound | undefined, b: Bound | undefined) =>
	a === undefined || b === undefined
		? a === b
		: a.inclusive === b.inclusive && versionsEqual(a.version, b.version);

/** Structural equality, as `Range`'s derived `PartialEq`. */
export function rangesEqual(a: Range, b: Range): boolean {
	return (
		a.intervals.length === b.intervals.length &&
		a.intervals.every((interval, index) => {
			const other = b.intervals[index] as Interval;
			return (
				boundsEqual(interval.lower, other.lower) &&
				boundsEqual(interval.upper, other.upper)
			);
		})
	);
}

export function disjoint(left: Range, right: Range): boolean {
	return !left.intervals.some((one) =>
		right.intervals.some((other) => meet(one, other) !== undefined),
	);
}

function meet(left: Interval, right: Interval): Interval | undefined {
	const lower = maxBound(left.lower, right.lower);
	const upper = minBound(left.upper, right.upper);
	let inhabited = true;
	if (lower !== undefined && upper !== undefined) {
		const order = compareVersions(lower.version, upper.version);
		inhabited =
			order < 0 || (order === 0 && lower.inclusive && upper.inclusive);
	}
	return inhabited ? { lower, upper } : undefined;
}

function maxBound(
	left: Bound | undefined,
	right: Bound | undefined,
): Bound | undefined {
	if (left === undefined || right === undefined) return left ?? right;
	const order = compareVersions(left.version, right.version);
	if (order > 0) return left;
	if (order < 0) return right;
	return {
		version: left.version,
		inclusive: left.inclusive && right.inclusive,
	};
}

function minBound(
	left: Bound | undefined,
	right: Bound | undefined,
): Bound | undefined {
	if (left === undefined || right === undefined) return left ?? right;
	const order = compareVersions(left.version, right.version);
	if (order < 0) return left;
	if (order > 0) return right;
	return {
		version: left.version,
		inclusive: left.inclusive && right.inclusive,
	};
}

// ---- Cargo -------------------------------------------------------------

export function cargo(raw: string): Constraint {
	const text = trim(raw);
	if (text === '') return malformed('an empty version requirement');
	const request = parseVersionReq(text);
	if (request === undefined)
		return malformed('not a Cargo version requirement');
	if (request.length === 0) return range([ANY]);
	let interval: Interval = ANY;
	for (const comparator of request) {
		const merged = meet(interval, comparatorInterval(comparator));
		if (merged === undefined)
			return malformed('a requirement no version can satisfy');
		interval = merged;
	}
	return range([interval]);
}

function comparatorInterval(comparator: Comparator): Interval {
	const partial: Partial = {
		major: comparator.major,
		minor: comparator.minor,
		patch: comparator.patch,
		pre: comparator.pre,
	};
	switch (comparator.op) {
		case 'exact':
		case 'wildcard':
			return exactly(partial);
		case 'greater':
			return above(partial);
		case 'greaterEq':
			return atLeast(partial);
		case 'less':
			return below(partial);
		case 'lessEq':
			return atMost(partial);
		case 'tilde':
			return tilde(partial);
		case 'caret':
			return caret(partial);
	}
}

/** `rust-version` and go.mod's `go`: a floor, not a range. */
export function minimum(raw: string): Constraint {
	const p = partialOf(trim(raw));
	if (p === undefined) return malformed('not a version');
	if (p.major === undefined) return unknown('a wildcard minimum version');
	return range([atLeast(p)]);
}

// ---- npm ---------------------------------------------------------------

export function npm(raw: string): Constraint {
	const text = trim(raw);
	if (text === '') return range([ANY]);
	const unmodelled = npmUnmodelled(text);
	if (unmodelled !== undefined) return unknown(unmodelled);
	const intervals: Interval[] = [];
	for (const alternative of text.split('||')) {
		const set = npmSet(trim(alternative));
		if ('kind' in set) return set;
		if (set.interval !== undefined) intervals.push(set.interval);
	}
	if (intervals.length === 0)
		return malformed('a range no version can satisfy');
	return range(intervals);
}

function npmSet(
	text: string,
): { readonly interval: Interval | undefined } | Constraint {
	const tokens = npmTokens(text);
	if (tokens.length === 0) return { interval: ANY };
	if (tokens.length === 3 && tokens[1] === '-') {
		const low = partialOf(tokens[0] as string);
		const high = partialOf(tokens[2] as string);
		if (low === undefined || high === undefined)
			return malformed('not a version range');
		return {
			interval: { lower: atLeast(low).lower, upper: atMost(high).upper },
		};
	}
	let interval: Interval = ANY;
	for (const token of tokens) {
		const next = npmSimple(token);
		if ('kind' in next) return next;
		const merged = meet(interval, next);
		if (merged === undefined) return { interval: undefined };
		interval = merged;
	}
	return { interval };
}

const NPM_OPERATORS = Object.freeze(['>=', '<=', '>', '<', '=', '~']);

/** A lone operator joins the version after it: `>= 20` is `>=20`. */
function npmTokens(text: string): string[] {
	const out: string[] = [];
	let pending: string | undefined;
	for (const token of splitWhitespace(text)) {
		if (pending !== undefined) {
			out.push(`${pending}${token}`);
			pending = undefined;
			continue;
		}
		if (NPM_OPERATORS.includes(token) || token === '^') {
			pending = token;
			continue;
		}
		out.push(token);
	}
	if (pending !== undefined) out.push(pending);
	return out;
}

function npmSimple(token: string): Interval | Constraint {
	const [operator, rest] = splitOperator(token);
	if (operator !== '' && trim(rest) === '')
		return malformed('an operator with no version');
	const p = partialOf(rest);
	if (p === undefined) return npmRefusal(token);
	if (p.major === undefined) return ANY;
	switch (operator) {
		case '^':
			return caret(p);
		case '~':
			return tilde(p);
		case '>=':
			return atLeast(p);
		case '>':
			return above(p);
		case '<=':
			return atMost(p);
		case '<':
			return below(p);
		default:
			return exactly(p);
	}
}

function npmRefusal(token: string): Constraint {
	if (/^[A-Za-z][A-Za-z0-9-]*$/.test(token))
		return unknown('a dist tag is a moving target, not a version range');
	return malformed('not an npm version range');
}

function npmUnmodelled(text: string): string | undefined {
	if (text.includes('://') || text.startsWith('git@'))
		return 'a URL or git specifier resolves outside the registry';
	if (text.includes(':'))
		return 'a protocol specifier resolves outside the registry';
	if (text.includes('/') || text.startsWith('.'))
		return 'a path or repository shorthand resolves outside the registry';
	return undefined;
}

// ---- PEP 440 -----------------------------------------------------------

export function python(raw: string): Constraint {
	const text = trim(raw);
	if (text === '') return range([ANY]);
	let interval: Interval = ANY;
	for (const piece of text.split(',')) {
		const clause = trim(piece);
		if (clause === '') return malformed('an empty version clause');
		const next = pythonClause(clause);
		if ('kind' in next) return next;
		const merged = meet(interval, next);
		if (merged === undefined)
			return malformed('a specifier no version can satisfy');
		interval = merged;
	}
	return range([interval]);
}

function pythonClause(clause: string): Interval | Constraint {
	for (const [operator, reason] of [
		['~=', 'PEP 440 compatible-release (~=)'],
		['!=', 'PEP 440 version exclusion (!=)'],
		['===', 'PEP 440 arbitrary equality (===)'],
	] as const) {
		if (clause.startsWith(operator)) return unknown(reason);
	}
	const [operator, restRaw] = splitOperator(clause);
	const rest = trim(restRaw);
	if (rest.includes('*')) return unknown('a PEP 440 wildcard release clause');
	const p = pep440(rest);
	if (p === undefined) return pythonRefusal(rest);
	switch (operator) {
		case '>=':
			return atLeast(p);
		case '>':
			return aboveRelease(p);
		case '<=':
			return atMostRelease(p);
		case '<':
			return below(p);
		case '==':
			return exactlyRelease(p);
		default:
			return unknown('a PEP 440 clause with no operator');
	}
}

function pythonRefusal(text: string): Constraint {
	for (const marker of ['!', '+', 'post', 'dev', '*']) {
		if (text.includes(marker))
			return unknown('a PEP 440 version form this tool does not model');
	}
	return malformed('not a PEP 440 version');
}

function pep440(text: string): Partial | undefined {
	const split = splitPep440Pre(text);
	if (split === undefined) return undefined;
	const p = partialOf(split[0]);
	if (p === undefined) return undefined;
	return { ...p, pre: split[1] };
}

function splitPep440Pre(text: string): [string, string] | undefined {
	const boundary = text.search(/[A-Za-z]/);
	if (boundary === -1) return [text, ''];
	const release = text.slice(0, boundary);
	const tail = text.slice(boundary);
	const digit = tail.search(/[0-9]/);
	const at = digit === -1 ? 0 : digit;
	const label = tail.slice(0, at);
	const number = tail.slice(at);
	if (!['a', 'b', 'rc'].includes(label) || number === '') return undefined;
	const pre = parsePrerelease(`${label}.${number}`);
	if (pre === undefined) return undefined;
	return [release.replace(/\.+$/, ''), pre];
}

// ---- go.mod, action refs, CI tool versions -----------------------------

export function go(raw: string): Constraint {
	const version = parseVersion(trim(raw).replace(/^v+/, ''));
	if (version === undefined) return malformed('not a go module version');
	return range([point(version)]);
}

export function actionRef(raw: string): Constraint {
	const text = trim(raw);
	if (isCommitSha(text))
		return unknown('a commit SHA is a pin, but not a version');
	const p = partialOf(text);
	if (p === undefined)
		return unknown('a branch or tag name is a moving reference, not a version');
	if (p.major === undefined) return unknown('a wildcard action reference');
	return range([exactly(p)]);
}

export function isCommitSha(text: string): boolean {
	const hex = /^[0-9A-Fa-f]*$/.test(text);
	return (
		hex && (text.length === 40 || (text.length >= 7 && /[A-Fa-f]/.test(text)))
	);
}

export const MOVING_CHANNELS = Object.freeze([
	'latest',
	'stable',
	'nightly',
	'beta',
	'current',
	'node',
	'lts',
]);

export function ciTool(raw: string): Constraint {
	const text = trim(raw);
	if (MOVING_CHANNELS.includes(text) || text.startsWith('lts/')) {
		return unknown('a channel name is a moving target, not a version');
	}
	return npm(text);
}

// ---- partial versions --------------------------------------------------

interface Partial {
	/** `undefined` is a wildcard: `*`, `x`, or nothing. */
	readonly major: bigint | undefined;
	readonly minor: bigint | undefined;
	readonly patch: bigint | undefined;
	readonly pre: string;
}

const U64_MAX = 0xffff_ffff_ffff_ffffn;
const isWildcard = (text: string) =>
	text === '*' || text === 'x' || text === 'X';

/** `str::parse::<u64>`: an optional `+`, then digits. */
function parseU64(text: string): bigint | undefined {
	if (!/^\+?[0-9]+$/.test(text)) return undefined;
	const value = BigInt(text.replace(/^\+/, ''));
	return value > U64_MAX ? undefined : value;
}

function partialOf(input: string): Partial | undefined {
	let text = trim(input);
	if (text.startsWith('v') || text.startsWith('V')) text = text.slice(1);
	if (text === '' || isWildcard(text))
		return { major: undefined, minor: undefined, patch: undefined, pre: '' };
	text = text.split('+')[0] as string;
	let release = text;
	let pre = '';
	const dash = text.indexOf('-');
	if (dash !== -1) {
		release = text.slice(0, dash);
		const parsed = parsePrerelease(text.slice(dash + 1));
		if (parsed === undefined) return undefined;
		pre = parsed;
	}
	const numbers: Array<bigint | undefined> = [undefined, undefined, undefined];
	const segments = release.split('.');
	let index = 0;
	for (; index < 3; index++) {
		const segment = segments[index];
		if (segment === undefined || isWildcard(segment)) {
			if (segment !== undefined) index++;
			break;
		}
		const value = parseU64(segment);
		if (value === undefined) return undefined;
		numbers[index] = value;
	}
	const fourth = segments[index];
	if (fourth !== undefined && fourth !== '') return undefined;
	if (numbers[0] === undefined) return undefined;
	return { major: numbers[0], minor: numbers[1], patch: numbers[2], pre };
}

function splitOperator(token: string): [string, string] {
	for (const operator of ['>=', '<=', '==', '^', '~', '>', '<', '=']) {
		if (token.startsWith(operator))
			return [operator, token.slice(operator.length)];
	}
	return ['', token];
}

const saturating = (value: bigint) =>
	value + 1n > U64_MAX ? U64_MAX : value + 1n;
const build = (
	major: bigint,
	minor: bigint,
	patch: bigint,
	pre: string,
): Version => ({ major, minor, patch, pre, build: '' });
const floorVersion = (p: Partial) =>
	build(p.major ?? 0n, p.minor ?? 0n, p.patch ?? 0n, p.pre);

function ceiling(p: Partial): Version | undefined {
	if (p.major === undefined) return undefined;
	if (p.minor === undefined) return build(saturating(p.major), 0n, 0n, '');
	if (p.patch === undefined) return build(p.major, saturating(p.minor), 0n, '');
	return undefined;
}

const point = (version: Version): Interval => ({
	lower: { version, inclusive: true },
	upper: { version, inclusive: true },
});
const atLeast = (p: Partial): Interval => ({
	lower: { version: floorVersion(p), inclusive: true },
	upper: undefined,
});

function above(p: Partial): Interval {
	const version = ceiling(p);
	if (version !== undefined)
		return { lower: { version, inclusive: true }, upper: undefined };
	return {
		lower: { version: floorVersion(p), inclusive: false },
		upper: undefined,
	};
}

const below = (p: Partial): Interval => ({
	lower: undefined,
	upper: { version: floorVersion(p), inclusive: false },
});

function atMost(p: Partial): Interval {
	const version = ceiling(p);
	if (version !== undefined)
		return { lower: undefined, upper: { version, inclusive: false } };
	return {
		lower: undefined,
		upper: { version: floorVersion(p), inclusive: true },
	};
}

function exactly(p: Partial): Interval {
	const version = ceiling(p);
	if (version !== undefined)
		return {
			lower: { version: floorVersion(p), inclusive: true },
			upper: { version, inclusive: false },
		};
	return point(floorVersion(p));
}

const exactlyRelease = (p: Partial): Interval => point(floorVersion(p));
const aboveRelease = (p: Partial): Interval => ({
	lower: { version: floorVersion(p), inclusive: false },
	upper: undefined,
});
const atMostRelease = (p: Partial): Interval => ({
	lower: undefined,
	upper: { version: floorVersion(p), inclusive: true },
});

function tilde(p: Partial): Interval {
	const major = p.major ?? 0n;
	const upper =
		p.minor !== undefined
			? build(major, saturating(p.minor), 0n, '')
			: build(saturating(major), 0n, 0n, '');
	return bounded(floorVersion(p), upper);
}

function caret(p: Partial): Interval {
	const major = p.major ?? 0n;
	if (major > 0n)
		return bounded(floorVersion(p), build(saturating(major), 0n, 0n, ''));
	let upper: Version;
	if (p.minor === 0n && p.patch !== undefined)
		upper = build(0n, 0n, saturating(p.patch), '');
	else if (p.minor !== undefined)
		upper = build(0n, saturating(p.minor), 0n, '');
	else upper = build(1n, 0n, 0n, '');
	return bounded(floorVersion(p), upper);
}

const bounded = (lower: Version, upper: Version): Interval => ({
	lower: { version: lower, inclusive: true },
	upper: { version: upper, inclusive: false },
});
