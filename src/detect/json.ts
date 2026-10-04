/**
 * `serde_json::from_str::<Value>` as serde_json 1.0 answers it — the reader
 * behind every `package.json` the crate parses.
 *
 * Not `JSON.parse`: what serde_json refuses — a number out of `f64` range, a
 * control character in a string, nesting past 128 — is a manifest the crate
 * reports as "not valid JSON", and `JSON.parse` accepts some of those. A
 * repeated key keeps its last value, and an object's keys come back in sorted
 * byte order, as `Value`'s `BTreeMap` holds them without `preserve_order`.
 *
 * Transcribed from i18n-le's reader of the same crate, which matches
 * serde_json's paths byte for byte; here every array element is read as a
 * value, because `Value` reads them all.
 */

import { compareStrings } from './semver';

/** A `serde_json::Value`, as far as the crate looks into one. */
export type Node =
	| { readonly kind: 'text'; readonly text: string }
	| {
			readonly kind: 'object';
			readonly entries: ReadonlyArray<readonly [string, Node]>;
	  }
	| { readonly kind: 'other' };

const OTHER: Node = Object.freeze({ kind: 'other' });

class SyntaxFailure extends Error {}

const ERRORS = Object.freeze({
	eofList: 'EOF while parsing a list',
	eofObject: 'EOF while parsing an object',
	eofString: 'EOF while parsing a string',
	eofValue: 'EOF while parsing a value',
	colon: 'expected `:`',
	listCommaOrEnd: 'expected `,` or `]`',
	objectCommaOrEnd: 'expected `,` or `}`',
	ident: 'expected ident',
	value: 'expected value',
	escape: 'invalid escape',
	number: 'invalid number',
	range: 'number out of range',
	control: 'control character (\\u0000-\\u001F) found while parsing a string',
	key: 'key must be a string',
	loneSurrogate: 'lone leading surrogate in hex escape',
	hexEnd: 'unexpected end of hex escape',
	trailingComma: 'trailing comma',
	trailingCharacters: 'trailing characters',
	recursion: 'recursion limit exceeded',
});

const U64_MAX = 0xffff_ffff_ffff_ffffn;
const I32_MAX = 2_147_483_647;
const POW10: readonly number[] = Array.from({ length: 309 }, (_, i) =>
	Number(`1e${i}`),
);

// `ignoreBOM`, or a string that begins with U+FEFF loses it: the decoder
// treats a leading byte-order mark as framing, and here it is content.
const decoder = new TextDecoder('utf-8', { ignoreBOM: true });

/**
 * Parse a document, or throw an `Error` whose message is serde_json's.
 *
 * The input must already be text: serde_json is handed a `&str`, and so is
 * this.
 */
export function parseJson(text: string): Node {
	const reader = new Reader(new TextEncoder().encode(text));
	try {
		const node = reader.value();
		reader.end();
		return node;
	} catch (error) {
		if (error instanceof SyntaxFailure) throw new Error(error.message);
		throw error;
	}
}

class Reader {
	private index = 0;
	private remainingDepth = 128;

	constructor(private readonly bytes: Uint8Array) {}

	// ---- position and errors -------------------------------------------

	private at(i: number): string {
		let start = 0;
		let line = 1;
		for (let k = 0; k < i; k++) {
			if (this.bytes[k] === 0x0a) {
				line++;
				start = k + 1;
			}
		}
		return `at line ${line} column ${i - start}`;
	}

	/** serde_json's `error`: the position of what was just consumed. */
	private error(reason: string): SyntaxFailure {
		return new SyntaxFailure(`${reason} ${this.at(this.index)}`);
	}

	/** serde_json's `peek_error`: the position of the byte looked at. */
	private peekError(reason: string): SyntaxFailure {
		return new SyntaxFailure(
			`${reason} ${this.at(Math.min(this.bytes.length, this.index + 1))}`,
		);
	}

	// ---- bytes ---------------------------------------------------------

	private peek(): number | undefined {
		return this.bytes[this.index];
	}

	private peekOrNull(): number {
		return this.bytes[this.index] ?? 0;
	}

	private next(): number | undefined {
		if (this.index >= this.bytes.length) return undefined;
		return this.bytes[this.index++];
	}

	private whitespace(): number | undefined {
		for (;;) {
			const byte = this.peek();
			if (byte !== 0x20 && byte !== 0x0a && byte !== 0x09 && byte !== 0x0d)
				return byte;
			this.index++;
		}
	}

	end(): void {
		if (this.whitespace() !== undefined)
			throw this.peekError(ERRORS.trailingCharacters);
	}

	// ---- deserialize_any -----------------------------------------------

	value(): Node {
		const peek = this.whitespace();
		if (peek === undefined) throw this.peekError(ERRORS.eofValue);

		switch (peek) {
			case 0x6e: // n
				this.index++;
				this.ident('ull');
				return OTHER;
			case 0x74: // t
				this.index++;
				this.ident('rue');
				return OTHER;
			case 0x66: // f
				this.index++;
				this.ident('alse');
				return OTHER;
			case 0x2d: // -
				this.index++;
				this.integer();
				return OTHER;
			case 0x22: // "
				this.index++;
				return { kind: 'text', text: this.string() };
			case 0x5b: // [
				this.descend();
				this.index++;
				this.sequence();
				this.remainingDepth++;
				this.endSequence();
				return OTHER;
			case 0x7b: {
				// {
				this.descend();
				this.index++;
				const entries = this.map();
				this.remainingDepth++;
				this.endMap();
				return { kind: 'object', entries: sortedLastWins(entries) };
			}
			default:
				if (peek >= 0x30 && peek <= 0x39) {
					this.integer();
					return OTHER;
				}
				throw this.peekError(ERRORS.value);
		}
	}

	private descend(): void {
		this.remainingDepth--;
		if (this.remainingDepth === 0) throw this.peekError(ERRORS.recursion);
	}

	private ident(rest: string): void {
		for (const expected of rest) {
			const next = this.next();
			if (next === undefined) throw this.error(ERRORS.eofValue);
			if (next !== expected.charCodeAt(0)) throw this.error(ERRORS.ident);
		}
	}

	// ---- objects and arrays --------------------------------------------

	private map(): Array<readonly [string, Node]> {
		const entries: Array<readonly [string, Node]> = [];
		let first = true;
		for (;;) {
			const peek = this.whitespace();
			if (peek === undefined) throw this.peekError(ERRORS.eofObject);
			if (peek === 0x7d) return entries;
			if (first) {
				first = false;
				if (peek !== 0x22) throw this.peekError(ERRORS.key);
			} else if (peek === 0x2c) {
				this.index++;
				const after = this.whitespace();
				if (after === 0x7d) throw this.peekError(ERRORS.trailingComma);
				if (after === undefined) throw this.peekError(ERRORS.eofValue);
				if (after !== 0x22) throw this.peekError(ERRORS.key);
			} else {
				throw this.peekError(ERRORS.objectCommaOrEnd);
			}
			this.index++;
			const key = this.string();
			this.colon();
			entries.push([key, this.value()]);
		}
	}

	private colon(): void {
		const peek = this.whitespace();
		if (peek === 0x3a) {
			this.index++;
			return;
		}
		if (peek === undefined) throw this.peekError(ERRORS.eofObject);
		throw this.peekError(ERRORS.colon);
	}

	private endMap(): void {
		const peek = this.whitespace();
		if (peek === 0x7d) {
			this.index++;
			return;
		}
		if (peek === 0x2c) throw this.peekError(ERRORS.trailingComma);
		if (peek === undefined) throw this.peekError(ERRORS.eofObject);
		throw this.peekError(ERRORS.trailingCharacters);
	}

	/** `Value` reads every element, inside the recursion limit. */
	private sequence(): void {
		let first = true;
		for (;;) {
			const peek = this.whitespace();
			if (peek === undefined) throw this.peekError(ERRORS.eofList);
			if (peek === 0x5d) return;
			if (first) {
				first = false;
			} else if (peek === 0x2c) {
				this.index++;
				const after = this.whitespace();
				if (after === 0x5d) throw this.peekError(ERRORS.trailingComma);
				if (after === undefined) throw this.peekError(ERRORS.eofValue);
			} else {
				throw this.peekError(ERRORS.listCommaOrEnd);
			}
			this.value();
		}
	}

	private endSequence(): void {
		const peek = this.whitespace();
		if (peek === 0x5d) {
			this.index++;
			return;
		}
		if (peek === 0x2c) {
			this.index++;
			if (this.whitespace() === 0x5d)
				throw this.peekError(ERRORS.trailingComma);
			throw this.peekError(ERRORS.trailingCharacters);
		}
		if (peek === undefined) throw this.peekError(ERRORS.eofList);
		throw this.peekError(ERRORS.trailingCharacters);
	}

	// ---- numbers, read only far enough to know whether they fit ---------
	// The sign never changes whether a number is in range, so it is not carried.

	private integer(): void {
		const next = this.next();
		if (next === undefined) throw this.error(ERRORS.eofValue);
		if (next === 0x30) {
			if (isDigit(this.peekOrNull())) throw this.peekError(ERRORS.number);
			this.number(0n);
			return;
		}
		if (next < 0x31 || next > 0x39) throw this.error(ERRORS.number);

		let significand = BigInt(next - 0x30);
		for (;;) {
			const c = this.peekOrNull();
			if (!isDigit(c)) {
				this.number(significand);
				return;
			}
			const digit = BigInt(c - 0x30);
			if (overflowsU64(significand, digit)) {
				this.longInteger(significand);
				return;
			}
			this.index++;
			significand = significand * 10n + digit;
		}
	}

	private number(significand: bigint): void {
		const c = this.peekOrNull();
		if (c === 0x2e) this.decimal(significand, 0);
		else if (c === 0x65 || c === 0x45) this.exponent(significand, 0);
	}

	private decimal(start: bigint, exponentBefore: number): void {
		this.index++;
		let significand = start;
		let exponentAfter = 0;
		for (;;) {
			const c = this.peekOrNull();
			if (!isDigit(c)) break;
			const digit = BigInt(c - 0x30);
			if (overflowsU64(significand, digit)) {
				this.decimalOverflow(significand, exponentBefore + exponentAfter);
				return;
			}
			this.index++;
			significand = significand * 10n + digit;
			exponentAfter--;
		}
		if (exponentAfter === 0) {
			if (this.peek() !== undefined) throw this.peekError(ERRORS.number);
			throw this.peekError(ERRORS.eofValue);
		}
		const exponent = exponentBefore + exponentAfter;
		const c = this.peekOrNull();
		if (c === 0x65 || c === 0x45) this.exponent(significand, exponent);
		else this.fromParts(significand, exponent);
	}

	private exponent(significand: bigint, starting: number): void {
		this.index++;
		let positiveExponent = true;
		const sign = this.peekOrNull();
		if (sign === 0x2b) this.index++;
		else if (sign === 0x2d) {
			this.index++;
			positiveExponent = false;
		}
		const next = this.next();
		if (next === undefined) throw this.error(ERRORS.eofValue);
		if (!isDigit(next)) throw this.error(ERRORS.number);
		let exp = next - 0x30;
		for (;;) {
			const c = this.peekOrNull();
			if (!isDigit(c)) break;
			this.index++;
			const digit = c - 0x30;
			if (
				exp >= Math.floor(I32_MAX / 10) &&
				(exp > Math.floor(I32_MAX / 10) || digit > I32_MAX % 10)
			) {
				this.exponentOverflow(significand === 0n, positiveExponent);
				return;
			}
			exp = exp * 10 + digit;
		}
		const final = positiveExponent
			? saturate(starting + exp)
			: saturate(starting - exp);
		this.fromParts(significand, final);
	}

	private longInteger(significand: bigint): void {
		let exponent = 0;
		for (;;) {
			const c = this.peekOrNull();
			if (isDigit(c)) {
				this.index++;
				exponent++;
				continue;
			}
			if (c === 0x2e) {
				this.decimal(significand, exponent);
				return;
			}
			if (c === 0x65 || c === 0x45) {
				this.exponent(significand, exponent);
				return;
			}
			this.fromParts(significand, exponent);
			return;
		}
	}

	private decimalOverflow(significand: bigint, exponent: number): void {
		while (isDigit(this.peekOrNull())) this.index++;
		const c = this.peekOrNull();
		if (c === 0x65 || c === 0x45) this.exponent(significand, exponent);
		else this.fromParts(significand, exponent);
	}

	private exponentOverflow(
		zeroSignificand: boolean,
		positiveExponent: boolean,
	): void {
		if (!zeroSignificand && positiveExponent) throw this.error(ERRORS.range);
		while (isDigit(this.peekOrNull())) this.index++;
	}

	/** `f64_from_parts` without `float_roundtrip`: only whether it overflows. */
	private fromParts(significand: bigint, start: number): void {
		let f = Number(significand);
		let exponent = start;
		for (;;) {
			const pow = POW10[Math.abs(exponent)];
			if (pow !== undefined) {
				if (exponent >= 0) {
					f *= pow;
					if (!Number.isFinite(f)) throw this.error(ERRORS.range);
				}
				return;
			}
			if (f === 0) return;
			if (exponent >= 0) throw this.error(ERRORS.range);
			f /= 1e308;
			exponent += 308;
		}
	}

	// ---- strings -------------------------------------------------------

	/** `parse_str` with validation on, as a `&str` input gets. */
	private string(): string {
		const bytes = this.bytes;
		let start = this.index;
		const parts: string[] = [];
		for (;;) {
			while (this.index < bytes.length) {
				const b = bytes[this.index] as number;
				if (b === 0x22 || b === 0x5c || b < 0x20) break;
				this.index++;
			}
			if (this.index === bytes.length) throw this.error(ERRORS.eofString);
			const b = bytes[this.index] as number;
			if (b === 0x22) {
				parts.push(decoder.decode(bytes.subarray(start, this.index)));
				this.index++;
				return parts.join('');
			}
			if (b === 0x5c) {
				parts.push(decoder.decode(bytes.subarray(start, this.index)));
				this.index++;
				parts.push(this.escape());
				start = this.index;
				continue;
			}
			this.index++;
			throw this.error(ERRORS.control);
		}
	}

	private escape(): string {
		const ch = this.next();
		if (ch === undefined) throw this.error(ERRORS.eofString);
		switch (ch) {
			case 0x22:
				return '"';
			case 0x5c:
				return '\\';
			case 0x2f:
				return '/';
			case 0x62:
				return '\b';
			case 0x66:
				return '\f';
			case 0x6e:
				return '\n';
			case 0x72:
				return '\r';
			case 0x74:
				return '\t';
			case 0x75:
				return this.unicodeEscape();
			default:
				throw this.error(ERRORS.escape);
		}
	}

	private unicodeEscape(): string {
		const n = this.hex();
		if (n >= 0xdc00 && n <= 0xdfff) throw this.error(ERRORS.loneSurrogate);
		if (n < 0xd800 || n > 0xdbff) return String.fromCharCode(n);

		if (this.peekOrEof() !== 0x5c) {
			this.index++;
			throw this.error(ERRORS.hexEnd);
		}
		this.index++;
		if (this.peekOrEof() !== 0x75) {
			this.index++;
			throw this.error(ERRORS.hexEnd);
		}
		this.index++;
		const n2 = this.hex();
		if (n2 < 0xdc00 || n2 > 0xdfff) throw this.error(ERRORS.loneSurrogate);
		return String.fromCharCode(n, n2);
	}

	private peekOrEof(): number {
		const b = this.peek();
		if (b === undefined) throw this.error(ERRORS.eofString);
		return b;
	}

	private hex(): number {
		if (this.bytes.length - this.index < 4) {
			this.index = this.bytes.length;
			throw this.error(ERRORS.eofString);
		}
		let value = 0;
		let valid = true;
		for (let k = 0; k < 4; k++) {
			const digit = hexValue(this.bytes[this.index + k] as number);
			if (digit < 0) valid = false;
			value = value * 16 + digit;
		}
		this.index += 4;
		if (!valid) throw this.error(ERRORS.escape);
		return value;
	}
}

/** `Value`'s map: sorted by key in byte order, a repeated key keeping its last value. */
function sortedLastWins(
	entries: ReadonlyArray<readonly [string, Node]>,
): Array<readonly [string, Node]> {
	const map = new Map<string, Node>();
	for (const [key, value] of entries) map.set(key, value);
	return [...map.entries()].sort(([a], [b]) => compareStrings(a, b));
}

function isDigit(byte: number): boolean {
	return byte >= 0x30 && byte <= 0x39;
}

function hexValue(byte: number): number {
	if (byte >= 0x30 && byte <= 0x39) return byte - 0x30;
	if (byte >= 0x41 && byte <= 0x46) return byte - 0x41 + 10;
	if (byte >= 0x61 && byte <= 0x66) return byte - 0x61 + 10;
	return -1;
}

/** serde_json's `overflow!(a * 10 + b, u64::MAX)`. */
function overflowsU64(a: bigint, b: bigint): boolean {
	return a >= U64_MAX / 10n && (a > U64_MAX / 10n || b > U64_MAX % 10n);
}

/** i32 saturating arithmetic, as `saturating_add` and `saturating_sub` do. */
function saturate(value: number): number {
	return Math.max(-2_147_483_648, Math.min(I32_MAX, value));
}
