/**
 * Rust's string questions, answered the way Rust answers them: `trim` strips
 * Unicode `White_Space`, which is not JavaScript's set (U+0085 is whitespace
 * to Rust and not to `trim`; U+FEFF is the reverse).
 */
const SPACE =
	'\\t\\n\\v\\f\\r \\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000';
const LEADING = new RegExp(`^[${SPACE}]+`);
const TRAILING = new RegExp(`[${SPACE}]+$`);
const ONE = new RegExp(`^[${SPACE}]$`, 'u');

/** `char::is_whitespace`: Unicode `White_Space`. */
export const isWhitespace = (character: string): boolean => ONE.test(character);

export const trimStart = (text: string): string => text.replace(LEADING, '');
export const trimEnd = (text: string): string => text.replace(TRAILING, '');
export const trim = (text: string): string => trimEnd(trimStart(text));

const encoder = new TextEncoder();

/** How many bytes `text` is in UTF-8: what Rust's `len()` counts. */
export const byteLength = (text: string): number => encoder.encode(text).length;

/** `u8::is_ascii_whitespace`: space, tab, LF, FF, CR — not VT. */
export function isAsciiWhitespace(character: string): boolean {
	return (
		character === ' ' ||
		character === '\t' ||
		character === '\n' ||
		character === '\f' ||
		character === '\r'
	);
}

const SPLIT = new RegExp(`[${SPACE}]+`);

/** `str::split_whitespace`: Unicode `White_Space` runs, empty pieces dropped. */
export const splitWhitespace = (text: string): string[] =>
	text.split(SPLIT).filter((piece) => piece !== '');

/** `str::lines`: split on `\n`, a trailing `\r` dropped from each, no final empty line. */
export function lines(text: string): string[] {
	if (text === '') return [];
	const out = text.split('\n');
	if (out.at(-1) === '') out.pop();
	return out.map((line) => (line.endsWith('\r') ? line.slice(0, -1) : line));
}
