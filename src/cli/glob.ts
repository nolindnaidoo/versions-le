/**
 * globset 0.4.20's `GlobBuilder` with `literal_separator(true)`, as the
 * crate's `Excludes` builds it, transcribed rather than approximated.
 *
 * globset compiles a glob to a byte regex (`(?-u)`), so `?`, `*` and a class
 * each match one byte of the path's UTF-8, never one character. This matches
 * the same way: the path becomes one JavaScript character per byte, and the
 * glob compiles to the regex text globset writes. Rust's `.` matches every
 * byte but `\n`, where JavaScript's also refuses `\r`, so `.` is written out.
 */

type Token =
	| { readonly t: 'literal'; readonly c: string }
	| { readonly t: 'any' | 'star' | 'prefix' | 'suffix' | 'between' }
	| {
			readonly t: 'class';
			readonly negated: boolean;
			readonly ranges: (readonly [string, string])[];
	  }
	| { readonly t: 'alternates'; readonly branches: readonly Token[][] };

/** Thrown for a glob globset refuses to build; the caller skips it. */
function refused(reason: string): Error {
	return Object.assign(new Error(reason), { name: 'GlobError' });
}

function parser(glob: string, windows: boolean) {
	const chars = [...glob];
	const isSeparator = (c: string | undefined) =>
		c === '/' || (windows && c === '\\');
	const stack: number[] = [];
	const branches: Token[][] = [[]];
	let index = 0;
	let prev: string | undefined;
	let cur: string | undefined;

	const bump = () => {
		prev = cur;
		cur = chars[index++];
		return cur;
	};
	const peek = () => chars[index];
	const last = () => branches[branches.length - 1] as Token[];
	const push = (token: Token): void => {
		last().push(token);
	};
	const literal = (c: string) => push({ t: 'literal', c });

	const star = () => {
		const before = prev;
		if (peek() !== '*') return push({ t: 'star' });
		bump();
		if (last().length === 0) {
			const next = peek();
			if (next !== undefined && !isSeparator(next)) {
				push({ t: 'star' });
				return push({ t: 'star' });
			}
			push({ t: 'prefix' });
			bump();
			return;
		}
		if (!isSeparator(before)) {
			if (branches.length <= 1 || (before !== ',' && before !== '{')) {
				push({ t: 'star' });
				return push({ t: 'star' });
			}
		}
		const next = peek();
		let suffix: boolean;
		if (next === undefined) {
			bump();
			suffix = true;
		} else if ((next === ',' || next === '}') && branches.length >= 2) {
			suffix = true;
		} else if (isSeparator(next)) {
			bump();
			suffix = false;
		} else {
			push({ t: 'star' });
			return push({ t: 'star' });
		}
		const popped = last().pop() as Token;
		if (popped.t === 'prefix' || popped.t === 'suffix') return push(popped);
		push({ t: suffix ? 'suffix' : 'between' });
	};

	const klass = () => {
		const ranges: [string, string][] = [];
		const negated = peek() === '!' || peek() === '^';
		if (negated) bump();
		let first = true;
		let inRange = false;
		const extend = (c: string) => {
			const range = ranges[ranges.length - 1] as [string, string];
			range[1] = c;
			if ((c.codePointAt(0) as number) < (range[0].codePointAt(0) as number)) {
				throw refused('invalid range');
			}
		};
		for (;;) {
			const c = bump();
			if (c === undefined) throw refused('unclosed class');
			if (c === ']' && !first) break;
			if (c === ']' || (c === '-' && first)) ranges.push([c, c]);
			else if (c === '-' && inRange) {
				extend('-');
				inRange = false;
			} else if (c === '-') inRange = true;
			else {
				if (inRange) extend(c);
				else ranges.push([c, c]);
				inRange = false;
			}
			first = false;
		}
		if (inRange) ranges.push(['-', '-']);
		push({ t: 'class', negated, ranges });
	};

	return (): Token[] => {
		for (let c = bump(); c !== undefined; c = bump()) {
			if (c === '?') push({ t: 'any' });
			else if (c === '*') star();
			else if (c === '[') klass();
			else if (c === '{') {
				stack.push(branches.length);
				branches.push([]);
			} else if (c === '}') {
				const start = stack.pop();
				if (start === undefined) throw refused('unopened alternates');
				push({ t: 'alternates', branches: branches.splice(start) });
			} else if (c === ',' && stack.length === 0) literal(',');
			else if (c === ',') branches.push([]);
			else if (c === '\\' && !windows) {
				const escaped = bump();
				if (escaped === undefined) throw refused('dangling escape');
				literal(escaped);
			} else if (c === '\\') literal('/');
			else literal(c);
		}
		if (branches.length > 1) throw refused('unclosed alternates');
		return branches[0] as Token[];
	};
}

/** regex_syntax's meta characters, escaped as `escape_into` escapes them. */
const META = new Set('\\.+*?()|[]{}^$#&-~');

function literalBytes(c: string): string {
	let out = '';
	for (const byte of Buffer.from(c, 'utf8')) {
		const char = String.fromCharCode(byte);
		if (byte > 0x7f) out += `\\x${byte.toString(16).padStart(2, '0')}`;
		else out += META.has(char) ? `\\${char}` : char;
	}
	return out;
}

function regexOf(tokens: readonly Token[]): string {
	let re = '';
	for (const token of tokens) {
		if (token.t === 'literal') re += literalBytes(token.c);
		if (token.t === 'any') re += '[^/]';
		if (token.t === 'star') re += '[^/]*';
		if (token.t === 'prefix') re += '(?:/?|[^\\n]*/)';
		if (token.t === 'suffix') re += '/[^\\n]*';
		if (token.t === 'between') re += '(?:/|/[^\\n]*/)';
		if (token.t === 'class') {
			const ranges = token.ranges.map(([low, high]) =>
				low === high
					? literalBytes(low)
					: `${literalBytes(low)}-${literalBytes(high)}`,
			);
			re += `[${token.negated ? '^' : ''}${ranges.join('')}]`;
		}
		if (token.t === 'alternates') {
			// `empty_alternates` is off: an empty branch is dropped, and so is
			// a group left with none.
			const parts = token.branches.map(regexOf).filter((part) => part !== '');
			if (parts.length > 0) re += `(?:${parts.join('|')})`;
		}
	}
	return re;
}

/** The glob as globset matches it, or null where globset refuses to build it. */
export function compileGlob(glob: string, windows: boolean): RegExp | null {
	let tokens: Token[];
	try {
		tokens = parser(glob, windows)();
	} catch (error) {
		if (error instanceof Error && error.name === 'GlobError') return null;
		throw error;
	}
	const only = tokens[0];
	const body =
		tokens.length === 1 && only?.t === 'prefix' ? '[^\\n]*' : regexOf(tokens);
	return new RegExp(`^${body}$`);
}

/** A path as globset's byte regex sees it: one character per UTF-8 byte. */
export function asBytes(path: string): string {
	return Buffer.from(path, 'utf8').toString('latin1');
}
