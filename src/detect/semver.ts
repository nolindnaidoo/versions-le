/**
 * The `semver` crate 1.0.28, transcribed for what the crate's grammar uses:
 * `Version::parse`, `VersionReq::parse`, `Prerelease::new`, and the ordering
 * and display of a version.
 *
 * Only acceptance and values matter here — every caller discards the error —
 * so a parse that fails returns `undefined` rather than semver's message.
 * Numbers are `bigint` because semver's are `u64`.
 */

export interface Version {
	readonly major: bigint;
	readonly minor: bigint;
	readonly patch: bigint;
	/** Dot-separated identifiers; empty is no prerelease. */
	readonly pre: string;
	readonly build: string;
}

export type Op =
	| 'exact'
	| 'greater'
	| 'greaterEq'
	| 'less'
	| 'lessEq'
	| 'tilde'
	| 'caret'
	| 'wildcard';

export interface Comparator {
	readonly op: Op;
	readonly major: bigint;
	readonly minor: bigint | undefined;
	readonly patch: bigint | undefined;
	readonly pre: string;
}

const U64_MAX = 0xffff_ffff_ffff_ffffn;

class Failure extends Error {}
const fail = (): never => {
	throw new Failure();
};

/** `numeric_identifier`: digits, no leading zero, within u64. */
function numeric(input: string): [bigint, string] {
	let length = 0;
	let value = 0n;
	while (
		length < input.length &&
		input.charAt(length) >= '0' &&
		input.charAt(length) <= '9'
	) {
		if (value === 0n && length > 0) fail();
		value = value * 10n + BigInt(input.charCodeAt(length) - 48);
		if (value > U64_MAX) fail();
		length++;
	}
	if (length === 0) fail();
	return [value, input.slice(length)];
}

function dot(input: string): string {
	if (!input.startsWith('.')) fail();
	return input.slice(1);
}

function wildcard(input: string): string | undefined {
	return input.startsWith('*') || input.startsWith('x') || input.startsWith('X')
		? input.slice(1)
		: undefined;
}

/** `identifier`: dot-separated `[0-9A-Za-z-]` segments; a numeric prerelease segment has no leading zero. */
function identifier(input: string, prerelease: boolean): [string, string] {
	let accumulated = 0;
	let segment = 0;
	let nondigit = false;
	for (;;) {
		const character = input.charAt(accumulated + segment);
		if (/^[A-Za-z-]$/.test(character)) {
			segment++;
			nondigit = true;
		} else if (/^[0-9]$/.test(character)) {
			segment++;
		} else {
			const boundary = character;
			if (segment === 0) {
				if (accumulated === 0 && boundary !== '.') return ['', input];
				fail();
			}
			if (
				prerelease &&
				segment > 1 &&
				!nondigit &&
				input.slice(accumulated).startsWith('0')
			)
				fail();
			accumulated += segment;
			if (boundary === '.') {
				accumulated++;
				segment = 0;
				nondigit = false;
			} else {
				return [input.slice(0, accumulated), input.slice(accumulated)];
			}
		}
	}
}

function attempt<T>(read: () => T): T | undefined {
	try {
		return read();
	} catch (error) {
		if (error instanceof Failure) return undefined;
		throw error;
	}
}

export function parseVersion(text: string): Version | undefined {
	return attempt(() => {
		if (text === '') fail();
		let [major, rest] = numeric(text);
		rest = dot(rest);
		const [minor, afterMinor] = numeric(rest);
		rest = dot(afterMinor);
		const [patch, afterPatch] = numeric(rest);
		rest = afterPatch;
		if (rest === '') return { major, minor, patch, pre: '', build: '' };
		let pre = '';
		if (rest.startsWith('-')) {
			[pre, rest] = identifier(rest.slice(1), true);
			if (pre === '') fail();
		}
		let build = '';
		if (rest.startsWith('+')) {
			[build, rest] = identifier(rest.slice(1), false);
			if (build === '') fail();
		}
		if (rest !== '') fail();
		return { major, minor, patch, pre, build };
	});
}

/** `Prerelease::new`: the whole text one prerelease identifier. */
export function parsePrerelease(text: string): string | undefined {
	return attempt(() => {
		const [pre, rest] = identifier(text, true);
		if (rest !== '') fail();
		return pre;
	});
}

function op(input: string): [Op, string] {
	if (input.startsWith('=')) return ['exact', input.slice(1)];
	if (input.startsWith('>=')) return ['greaterEq', input.slice(2)];
	if (input.startsWith('>')) return ['greater', input.slice(1)];
	if (input.startsWith('<=')) return ['lessEq', input.slice(2)];
	if (input.startsWith('<')) return ['less', input.slice(1)];
	if (input.startsWith('~')) return ['tilde', input.slice(1)];
	if (input.startsWith('^')) return ['caret', input.slice(1)];
	return ['caret', input];
}

const trimSpaces = (text: string) => text.replace(/^ +/, '');

function comparator(input: string): [Comparator, string] {
	let [operator, text] = op(input);
	const defaultOp = input.length === text.length;
	text = trimSpaces(text);
	const [major, afterMajor] = numeric(text);
	text = afterMajor;
	let hasWildcard = false;
	let minor: bigint | undefined;
	let patch: bigint | undefined;
	if (text.startsWith('.')) {
		text = text.slice(1);
		const afterWildcard = wildcard(text);
		if (afterWildcard !== undefined) {
			hasWildcard = true;
			if (defaultOp) operator = 'wildcard';
			text = afterWildcard;
		} else {
			[minor, text] = numeric(text);
		}
	}
	if (text.startsWith('.')) {
		text = text.slice(1);
		const afterWildcard = wildcard(text);
		if (afterWildcard !== undefined) {
			if (defaultOp) operator = 'wildcard';
			text = afterWildcard;
		} else if (hasWildcard) {
			fail();
		} else {
			[patch, text] = numeric(text);
		}
	}
	let pre = '';
	if (patch !== undefined && text.startsWith('-')) {
		[pre, text] = identifier(text.slice(1), true);
		if (pre === '') fail();
	}
	if (patch !== undefined && text.startsWith('+')) {
		let build: string;
		[build, text] = identifier(text.slice(1), false);
		if (build === '') fail();
	}
	return [{ op: operator, major, minor, patch, pre }, trimSpaces(text)];
}

const MAX_COMPARATORS = 32;

/** `VersionReq::parse`: comma-separated comparators, or a lone wildcard. */
export function parseVersionReq(input: string): Comparator[] | undefined {
	return attempt(() => {
		const text = trimSpaces(input);
		const afterWildcard = wildcard(text);
		if (afterWildcard !== undefined) {
			if (trimSpaces(afterWildcard) === '') return [];
			fail();
		}
		const out: Comparator[] = [];
		let rest = text;
		for (;;) {
			const [one, after] = comparator(rest);
			out.push(one);
			if (after === '') return out;
			if (!after.startsWith(',')) fail();
			if (out.length === MAX_COMPARATORS) fail();
			rest = trimSpaces(after.slice(1));
		}
	});
}

// ---- ordering --------------------------------------------------------

const allDigits = (text: string) => /^[0-9]*$/.test(text);

/** Byte order, which is code-point order: what Rust's `str` comparison is. */
export function compareStrings(left: string, right: string): number {
	const a = Array.from(left);
	const b = Array.from(right);
	for (let index = 0; index < Math.min(a.length, b.length); index++) {
		const x = (a[index] as string).codePointAt(0) as number;
		const y = (b[index] as string).codePointAt(0) as number;
		if (x !== y) return x < y ? -1 : 1;
	}
	return a.length === b.length ? 0 : a.length < b.length ? -1 : 1;
}

const sign = (n: number) => (n < 0 ? -1 : n > 0 ? 1 : 0);
const compareNumbers = (a: number, b: number) => sign(a - b);
const compareBig = (a: bigint, b: bigint) => (a < b ? -1 : a > b ? 1 : 0);

export function comparePrerelease(left: string, right: string): number {
	if (left === right) return 0;
	if (left === '') return 1;
	if (right === '') return -1;
	const lhs = left.split('.');
	const rhs = right.split('.');
	for (let index = 0; index < lhs.length; index++) {
		const a = lhs[index] as string;
		const b = rhs[index];
		if (b === undefined) return 1;
		const [da, db] = [allDigits(a), allDigits(b)];
		let ordering: number;
		if (da && db)
			ordering = compareNumbers(a.length, b.length) || compareStrings(a, b);
		else if (da) return -1;
		else if (db) return 1;
		else ordering = compareStrings(a, b);
		if (ordering !== 0) return ordering;
	}
	return rhs.length > lhs.length ? -1 : 0;
}

export function compareBuild(left: string, right: string): number {
	if (left === right) return 0;
	const lhs = left.split('.');
	const rhs = right.split('.');
	for (let index = 0; index < lhs.length; index++) {
		const a = lhs[index] as string;
		const b = rhs[index];
		if (b === undefined) return 1;
		const [da, db] = [allDigits(a), allDigits(b)];
		let ordering: number;
		if (da && db) {
			const [va, vb] = [a.replace(/^0+/, ''), b.replace(/^0+/, '')];
			ordering =
				compareNumbers(va.length, vb.length) ||
				compareStrings(va, vb) ||
				compareNumbers(a.length, b.length);
		} else if (da) return -1;
		else if (db) return 1;
		else ordering = compareStrings(a, b);
		if (ordering !== 0) return ordering;
	}
	return rhs.length > lhs.length ? -1 : 0;
}

/** `Version`'s derived `Ord`: major, minor, patch, pre, build. */
export function compareVersions(left: Version, right: Version): number {
	return (
		compareBig(left.major, right.major) ||
		compareBig(left.minor, right.minor) ||
		compareBig(left.patch, right.patch) ||
		comparePrerelease(left.pre, right.pre) ||
		compareBuild(left.build, right.build)
	);
}

export const versionsEqual = (left: Version, right: Version) =>
	compareVersions(left, right) === 0;

export function formatVersion(version: Version): string {
	let out = `${version.major}.${version.minor}.${version.patch}`;
	if (version.pre !== '') out += `-${version.pre}`;
	if (version.build !== '') out += `+${version.build}`;
	return out;
}
