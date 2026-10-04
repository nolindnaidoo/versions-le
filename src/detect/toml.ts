/**
 * `toml::from_str::<toml::Value>` (toml 1.1.6, TOML 1.1) and `toml::Value`'s
 * `Display` — the reader behind every `Cargo.toml` and `pyproject.toml`, and
 * the writer that records a dependency spelled as neither a string nor a table.
 *
 * Acceptance is the contract: a document toml refuses is a manifest the crate
 * reports as "not valid TOML" and reads nothing from. toml_parser reports
 * every departure from the TOML 1.1 grammar as an error and the toml crate
 * fails on any error, so this is a strict parser of that grammar, with dates
 * checked as toml_datetime checks them and the table rules the toml crate
 * enforces. A table's keys come back in sorted byte order, as `toml::Table`
 * holds them without `preserve_order`.
 */
import { compareStrings } from './semver';

export interface Datetime {
	readonly date?: {
		readonly year: number;
		readonly month: number;
		readonly day: number;
	};
	readonly time?: {
		readonly hour: number;
		readonly minute: number;
		readonly second?: number;
		readonly nanosecond?: number;
	};
	readonly offset?: 'Z' | number;
}

export type TomlValue =
	| { readonly type: 'string'; readonly value: string }
	| { readonly type: 'integer'; readonly value: bigint }
	| { readonly type: 'float'; readonly value: number }
	| { readonly type: 'boolean'; readonly value: boolean }
	| { readonly type: 'datetime'; readonly value: Datetime }
	| { readonly type: 'array'; readonly value: TomlValue[] }
	| { readonly type: 'table'; readonly value: Map<string, TomlValue> };

class Invalid extends Error {}
const invalid = (): never => {
	throw new Invalid();
};

/**
 * The toml crate's `DeTable` flags. `implicit`: created on the way to
 * something else; `dotted`: created by a dotted key; `inline`: written as an
 * inline table, and never extended afterwards.
 */
interface TableNode {
	readonly kind: 'table';
	implicit: boolean;
	dotted: boolean;
	inline: boolean;
	readonly entries: Map<string, Node>;
}
interface ArrayNode {
	readonly kind: 'array';
	/** An array of tables, which `[[…]]` appends to and a dotted key reaches the last of. */
	readonly tables: boolean;
	readonly items: Node[];
}
interface ValueNode {
	readonly kind: 'value';
	readonly value: TomlValue;
}
type Node = TableNode | ArrayNode | ValueNode;

const table = (): TableNode => ({
	kind: 'table',
	implicit: false,
	dotted: false,
	inline: false,
	entries: new Map(),
});

const isBareKeyChar = (c: string) => /^[A-Za-z0-9_-]$/.test(c);
const isWs = (c: string) => c === ' ' || c === '\t';

/** Non-EOL characters a comment may hold: tab, printable ASCII, anything above it. */
function commentChar(code: number): boolean {
	return code === 0x09 || (code >= 0x20 && code <= 0x7e) || code >= 0x80;
}

class Parser {
	private at = 0;
	/** `State`: the document, the table under construction, and the header that names it. */
	private root = table();
	private current = table();
	private header:
		| { readonly path: string[]; readonly key: string; readonly array: boolean }
		| undefined;

	constructor(private readonly text: string) {
		if (text.startsWith('\uFEFF')) this.at = 1;
	}

	private peek(offset = 0): string {
		return this.text.charAt(this.at + offset);
	}

	private eof(): boolean {
		return this.at >= this.text.length;
	}

	private skipWs(): void {
		while (isWs(this.peek())) this.at++;
	}

	private comment(): void {
		if (this.peek() !== '#') return;
		this.at++;
		while (!this.eof()) {
			const code = this.text.codePointAt(this.at) as number;
			if (code === 0x0a || (code === 0x0d && this.peek(1) === '\n')) return;
			if (!commentChar(code)) invalid();
			this.at += code > 0xffff ? 2 : 1;
		}
	}

	/** A newline, or the end of the document. A lone CR is invalid. */
	private newline(): boolean {
		if (this.peek() === '\n') {
			this.at++;
			return true;
		}
		if (this.peek() === '\r') {
			if (this.peek(1) !== '\n') invalid();
			this.at += 2;
			return true;
		}
		return false;
	}

	/** Whitespace, comments and newlines, as arrays and inline tables allow between items. */
	private skipWsCommentNewline(): void {
		for (;;) {
			this.skipWs();
			this.comment();
			if (!this.newline()) return;
		}
	}

	parse(): Map<string, Node> {
		while (!this.eof()) {
			this.skipWs();
			const c = this.peek();
			if (c === '[') this.tableHeader();
			else if (c !== '#' && c !== '\n' && c !== '\r' && c !== '') this.keyval();
			this.skipWs();
			this.comment();
			if (!this.newline() && !this.eof()) invalid();
		}
		this.finishTable();
		return this.root.entries;
	}

	// ---- keys ----------------------------------------------------------

	private simpleKey(): string {
		const c = this.peek();
		if (c === '"') return this.basicString();
		if (c === "'") return this.literalString();
		const start = this.at;
		while (isBareKeyChar(this.peek())) this.at++;
		if (this.at === start) invalid();
		return this.text.slice(start, this.at);
	}

	private key(): string[] {
		const parts = [this.simpleKey()];
		for (;;) {
			const save = this.at;
			this.skipWs();
			if (this.peek() !== '.') {
				this.at = save;
				return parts;
			}
			this.at++;
			this.skipWs();
			parts.push(this.simpleKey());
		}
	}

	// ---- tables: the toml crate's `document.rs` -------------------------

	private tableHeader(): void {
		const array = this.peek(1) === '[';
		this.at += array ? 2 : 1;
		this.skipWs();
		const path = this.key();
		this.skipWs();
		if (array) {
			if (this.peek() !== ']' || this.peek(1) !== ']') invalid();
			this.at += 2;
		} else {
			if (this.peek() !== ']') invalid();
			this.at++;
		}
		this.finishTable();
		this.startTable({
			path: path.slice(0, -1),
			key: path.at(-1) as string,
			array,
		});
	}

	private startTable(header: {
		readonly path: string[];
		readonly key: string;
		readonly array: boolean;
	}): void {
		if (!header.array) {
			const parent = descend(this.root, header.path, false);
			const old = parent.entries.get(header.key);
			if (old !== undefined) {
				parent.entries.delete(header.key);
				if (old.kind === 'table' && old.implicit && !old.dotted)
					this.current = old;
				else invalid();
			}
		}
		this.current.implicit = false;
		this.current.dotted = false;
		this.header = header;
	}

	private finishTable(): void {
		const previous = this.current;
		this.current = table();
		const header = this.header;
		this.header = undefined;
		if (header === undefined) {
			this.root = previous;
			return;
		}
		const parent = descend(this.root, header.path, false);
		if (header.array) {
			let entry = parent.entries.get(header.key);
			if (entry === undefined) {
				entry = { kind: 'array', tables: true, items: [] };
				parent.entries.set(header.key, entry);
			}
			if (entry.kind !== 'array' || !entry.tables) invalid();
			(entry as ArrayNode).items.push(previous);
			return;
		}
		parent.entries.set(header.key, previous);
	}

	/** `capture_key_value`. */
	private keyval(): void {
		const path = this.key();
		this.skipWs();
		if (this.peek() !== '=') invalid();
		this.at++;
		this.skipWs();
		const value = this.value();
		const dotted = path.length > 1;
		const parent = descend(this.current, path.slice(0, -1), dotted);
		if (dotted && !parent.implicit) invalid();
		const key = path.at(-1) as string;
		if (parent.entries.has(key)) invalid();
		parent.entries.set(key, value);
	}

	// ---- values --------------------------------------------------------

	private value(): Node {
		const c = this.peek();
		if (c === '"') {
			return {
				kind: 'value',
				value: {
					type: 'string',
					value: this.text.startsWith('"""', this.at)
						? this.mlBasicString()
						: this.basicString(),
				},
			};
		}
		if (c === "'") {
			return {
				kind: 'value',
				value: {
					type: 'string',
					value: this.text.startsWith("'''", this.at)
						? this.mlLiteralString()
						: this.literalString(),
				},
			};
		}
		if (c === '[') return this.array();
		if (c === '{') return this.inlineTable();
		return { kind: 'value', value: this.scalar() };
	}

	private array(): Node {
		this.at++;
		const items: Node[] = [];
		this.skipWsCommentNewline();
		while (this.peek() !== ']') {
			items.push(this.value());
			this.skipWsCommentNewline();
			if (this.peek() === ',') {
				this.at++;
				this.skipWsCommentNewline();
			} else if (this.peek() !== ']') {
				invalid();
			}
		}
		this.at++;
		return { kind: 'array', tables: false, items };
	}

	/** TOML 1.1: newlines, comments and a trailing comma are allowed inside. `inline_table.rs`. */
	private inlineTable(): Node {
		this.at++;
		const result = table();
		result.inline = true;
		this.skipWsCommentNewline();
		while (this.peek() !== '}') {
			const path = this.key();
			this.skipWs();
			if (this.peek() !== '=') invalid();
			this.at++;
			this.skipWs();
			const value = this.value();
			const parent = descendInline(result, path.slice(0, -1));
			if (parent.dotted === (path.length === 1)) invalid();
			const key = path.at(-1) as string;
			if (parent.entries.has(key)) invalid();
			parent.entries.set(key, value);
			this.skipWsCommentNewline();
			if (this.peek() === ',') {
				this.at++;
				this.skipWsCommentNewline();
			} else if (this.peek() !== '}') {
				invalid();
			}
		}
		this.at++;
		return result;
	}

	// ---- strings -------------------------------------------------------

	private basicString(): string {
		this.at++;
		let out = '';
		for (;;) {
			if (this.eof()) invalid();
			const code = this.text.codePointAt(this.at) as number;
			const c = String.fromCodePoint(code);
			if (c === '"') {
				this.at++;
				return out;
			}
			if (c === '\\') {
				out += this.escape();
				continue;
			}
			if (
				!(
					code === 0x09 ||
					code === 0x20 ||
					code === 0x21 ||
					(code >= 0x23 && code <= 0x7e) ||
					code >= 0x80
				)
			)
				invalid();
			out += c;
			this.at += c.length;
		}
	}

	private escape(): string {
		this.at++;
		const c = this.peek();
		this.at++;
		const simple: Record<string, string> = {
			b: '\b',
			e: '\x1b',
			f: '\f',
			n: '\n',
			r: '\r',
			t: '\t',
			'\\': '\\',
			'"': '"',
		};
		if (simple[c] !== undefined) return simple[c] as string;
		const digits = c === 'x' ? 2 : c === 'u' ? 4 : c === 'U' ? 8 : 0;
		if (digits === 0) invalid();
		const hex = this.text.slice(this.at, this.at + digits);
		if (!new RegExp(`^[0-9A-Fa-f]{${digits}}$`).test(hex)) invalid();
		this.at += digits;
		const code = Number.parseInt(hex, 16);
		if (code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) invalid();
		return String.fromCodePoint(code);
	}

	private mlBasicString(): string {
		this.at += 3;
		this.newline();
		let out = '';
		for (;;) {
			if (this.eof()) invalid();
			if (this.text.startsWith('"""', this.at)) {
				let quotes = 3;
				while (this.peek(quotes) === '"') quotes++;
				if (quotes > 5) invalid();
				out += '"'.repeat(quotes - 3);
				this.at += quotes;
				return out;
			}
			const code = this.text.codePointAt(this.at) as number;
			const c = String.fromCodePoint(code);
			if (c === '\\') {
				// A line-ending backslash swallows the newline and the whitespace after it.
				let probe = this.at + 1;
				while (isWs(this.text.charAt(probe))) probe++;
				const nl = this.text.charAt(probe);
				if (
					nl === '\n' ||
					(nl === '\r' && this.text.charAt(probe + 1) === '\n')
				) {
					this.at = probe;
					for (;;) {
						const d = this.peek();
						if (isWs(d)) this.at++;
						else if (!this.newline()) break;
					}
					continue;
				}
				out += this.escape();
				continue;
			}
			if (c === '\n' || c === '\r') {
				this.newline();
				out += '\n';
				continue;
			}
			if (!(code === 0x09 || (code >= 0x20 && code <= 0x7e) || code >= 0x80))
				invalid();
			out += c;
			this.at += c.length;
		}
	}

	private literalString(): string {
		this.at++;
		const start = this.at;
		for (;;) {
			if (this.eof()) invalid();
			const code = this.text.codePointAt(this.at) as number;
			if (code === 0x27) {
				const value = this.text.slice(start, this.at);
				this.at++;
				return value;
			}
			if (!(code === 0x09 || (code >= 0x20 && code <= 0x7e) || code >= 0x80))
				invalid();
			this.at += code > 0xffff ? 2 : 1;
		}
	}

	private mlLiteralString(): string {
		this.at += 3;
		this.newline();
		let out = '';
		for (;;) {
			if (this.eof()) invalid();
			if (this.text.startsWith("'''", this.at)) {
				let quotes = 3;
				while (this.peek(quotes) === "'") quotes++;
				if (quotes > 5) invalid();
				out += "'".repeat(quotes - 3);
				this.at += quotes;
				return out;
			}
			const code = this.text.codePointAt(this.at) as number;
			if (code === 0x0a || code === 0x0d) {
				this.newline();
				out += '\n';
				continue;
			}
			if (!(code === 0x09 || (code >= 0x20 && code <= 0x7e) || code >= 0x80))
				invalid();
			const c = String.fromCodePoint(code);
			out += c;
			this.at += c.length;
		}
	}

	// ---- unquoted scalars ----------------------------------------------

	private scalar(): TomlValue {
		const rest = this.text.slice(this.at);
		const take = (length: number, value: TomlValue): TomlValue => {
			const after = rest.charAt(length);
			if (after !== '' && !/^[ \t\r\n,\]}#]$/.test(after)) invalid();
			this.at += length;
			return value;
		};
		let match = /^(true|false)/.exec(rest);
		if (match)
			return take(match[0].length, {
				type: 'boolean',
				value: match[0] === 'true',
			});

		match =
			/^[0-9]{4}-[0-9]{2}-[0-9]{2}(?:[Tt ][0-9]{2}:[0-9]{2}(?::[0-9]{2}(?:\.[0-9]+)?)?(?:[Zz]|[+-][0-9]{2}:[0-9]{2})?)?/.exec(
				rest,
			);
		if (match) {
			let raw = match[0];
			// A space joins a date to a time only when a time follows it.
			if (
				raw.length > 10 &&
				raw.charAt(10) === ' ' &&
				!/^[0-9]{2}:/.test(raw.slice(11))
			)
				raw = raw.slice(0, 10);
			return take(raw.length, { type: 'datetime', value: parseDatetime(raw) });
		}
		match = /^[0-9]{2}:[0-9]{2}(?::[0-9]{2}(?:\.[0-9]+)?)?/.exec(rest);
		if (match)
			return take(match[0].length, {
				type: 'datetime',
				value: parseDatetime(match[0]),
			});

		match = /^[+-]?(?:inf|nan)/.exec(rest);
		if (match) {
			const raw = match[0];
			const value = raw.endsWith('nan')
				? Number.NaN
				: raw.startsWith('-')
					? -Infinity
					: Infinity;
			return take(raw.length, { type: 'float', value });
		}

		match =
			/^0x[0-9A-Fa-f](?:_?[0-9A-Fa-f])*|^0o[0-7](?:_?[0-7])*|^0b[01](?:_?[01])*/.exec(
				rest,
			);
		if (match) {
			const raw = match[0];
			const value = BigInt(
				`0${raw.charAt(1)}${raw.slice(2).replace(/_/g, '')}`,
			);
			if (value > 0x7fff_ffff_ffff_ffffn) invalid();
			return take(raw.length, { type: 'integer', value });
		}

		const decimal = '[+-]?(?:0|[1-9](?:_?[0-9])*)';
		const digits = '[0-9](?:_?[0-9])*';
		match = new RegExp(
			`^${decimal}(?:\\.${digits})?(?:[eE][+-]?${digits})?`,
		).exec(rest);
		if (match) {
			const raw = match[0];
			const clean = raw.replace(/_/g, '');
			if (/[.eE]/.test(raw)) {
				const value = Number(clean);
				if (!Number.isFinite(value)) invalid();
				return take(raw.length, { type: 'float', value });
			}
			const value = BigInt(clean);
			if (value > 0x7fff_ffff_ffff_ffffn || value < -0x8000_0000_0000_0000n)
				invalid();
			return take(raw.length, { type: 'integer', value });
		}
		return invalid();
	}
}

/** `document::descend_path`. */
function descend(
	start: TableNode,
	path: readonly string[],
	dotted: boolean,
): TableNode {
	let current = start;
	for (const key of path) {
		const existing = current.entries.get(key);
		if (existing === undefined) {
			const created = table();
			created.implicit = true;
			created.dotted = dotted;
			current.entries.set(key, created);
			current = created;
		} else if (existing.kind === 'array') {
			if (!existing.tables) invalid();
			const last = existing.items.at(-1);
			if (last?.kind !== 'table') return invalid();
			current = last;
		} else if (existing.kind === 'table') {
			if (existing.inline) invalid();
			if (dotted && existing.implicit) existing.dotted = true;
			if (dotted && !existing.implicit) invalid();
			current = existing;
		} else {
			invalid();
		}
	}
	return current;
}

/** `inline_table::descend_path`: always dotted, and no arrays. */
function descendInline(start: TableNode, path: readonly string[]): TableNode {
	let current = start;
	for (const key of path) {
		const existing = current.entries.get(key);
		if (existing === undefined) {
			const created = table();
			created.implicit = true;
			created.dotted = true;
			created.inline = true;
			current.entries.set(key, created);
			current = created;
		} else if (existing.kind === 'table') {
			if (!existing.implicit) invalid();
			current = existing;
		} else {
			invalid();
		}
	}
	return current;
}

/** toml_datetime's `Datetime::from_str`, for text the scanner already shaped. */
export function parseDatetime(raw: string): Datetime {
	const date = /^([0-9]{4})-([0-9]{2})-([0-9]{2})/.exec(raw);
	let rest = raw;
	let result: {
		date?: NonNullable<Datetime['date']>;
		time?: NonNullable<Datetime['time']>;
		offset?: NonNullable<Datetime['offset']>;
	} = {};
	if (date) {
		const [year, month, day] = [
			Number(date[1]),
			Number(date[2]),
			Number(date[3]),
		];
		if (month < 1 || month > 12) invalid();
		const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
		const max =
			month === 2 ? (leap ? 29 : 28) : [4, 6, 9, 11].includes(month) ? 30 : 31;
		if (day < 1 || day > max) invalid();
		result = { date: { year, month, day } };
		rest = raw.slice(10);
		if (rest === '') return result;
		rest = rest.slice(1);
	}
	const time = /^([0-9]{2}):([0-9]{2})(?::([0-9]{2})(?:\.([0-9]+))?)?/.exec(
		rest,
	) as RegExpExecArray;
	const [hour, minute] = [Number(time[1]), Number(time[2])];
	const second = time[3] === undefined ? undefined : Number(time[3]);
	const nanosecond =
		time[4] === undefined
			? undefined
			: Number(`${time[4]}000000000`.slice(0, 9));
	if (hour > 23 || minute > 59 || (second ?? 0) > 60) invalid();
	result.time = {
		hour,
		minute,
		...(second === undefined ? {} : { second }),
		...(nanosecond === undefined ? {} : { nanosecond }),
	};
	rest = rest.slice(time[0].length);
	if (rest === '') return result;
	if (!result.date) invalid();
	if (rest === 'Z' || rest === 'z') return { ...result, offset: 'Z' };
	const offset = /^([+-])([0-9]{2}):([0-9]{2})$/.exec(rest);
	if (!offset) return invalid();
	const [hours, minutes] = [Number(offset[2]), Number(offset[3])];
	if (hours > 23 || minutes > 59) invalid();
	return {
		...result,
		offset: (offset[1] === '-' ? -1 : 1) * (hours * 60 + minutes),
	};
}

function toValue(node: Node): TomlValue {
	if (node.kind === 'value') return node.value;
	if (node.kind === 'array')
		return { type: 'array', value: node.items.map(toValue) };
	const entries = [...node.entries.entries()].sort(([a], [b]) =>
		compareStrings(a, b),
	);
	return {
		type: 'table',
		value: new Map(entries.map(([key, child]) => [key, toValue(child)])),
	};
}

/** The document as a table, or `undefined` where toml would refuse it. */
export function parseToml(text: string): Map<string, TomlValue> | undefined {
	try {
		const entries = new Parser(text).parse();
		const root = toValue({ ...table(), entries });
		return root.type === 'table' ? root.value : undefined;
	} catch (error) {
		if (error instanceof Invalid) return undefined;
		throw error;
	}
}

// ---- Display -----------------------------------------------------------

const pad = (value: number, width: number) =>
	String(value).padStart(width, '0');

export function formatDatetime(datetime: Datetime): string {
	let out = '';
	if (datetime.date)
		out += `${pad(datetime.date.year, 4)}-${pad(datetime.date.month, 2)}-${pad(datetime.date.day, 2)}`;
	if (datetime.time) {
		if (datetime.date) out += 'T';
		const time = datetime.time;
		out += `${pad(time.hour, 2)}:${pad(time.minute, 2)}`;
		const second =
			time.second ?? (time.nanosecond !== undefined ? 0 : undefined);
		if (second !== undefined) out += `:${pad(second, 2)}`;
		if (time.nanosecond !== undefined)
			out += `.${pad(time.nanosecond, 9).replace(/0+$/, '') || '0'}`;
	}
	if (datetime.offset === 'Z') out += 'Z';
	else if (datetime.offset !== undefined) {
		const sign = datetime.offset < 0 ? '-' : '+';
		const minutes = Math.abs(datetime.offset);
		out += `${sign}${pad(Math.trunc(minutes / 60), 2)}:${pad(minutes % 60, 2)}`;
	}
	return out;
}

/** Rust's `f64` `Display`: the shortest round-trip digits, never in exponent form. */
export function rustFloat(value: number): string {
	if (Number.isNaN(value)) return 'NaN';
	if (!Number.isFinite(value)) return value < 0 ? '-inf' : 'inf';
	const [mantissa, exponentText] = Math.abs(value)
		.toExponential()
		.split('e') as [string, string];
	const digits = mantissa.replace('.', '');
	const exponent = Number(exponentText);
	let out: string;
	if (exponent >= digits.length - 1)
		out = digits + '0'.repeat(exponent - digits.length + 1);
	else if (exponent >= 0)
		out = `${digits.slice(0, exponent + 1)}.${digits.slice(exponent + 1)}`;
	else out = `0.${'0'.repeat(-exponent - 1)}${digits}`;
	return (value < 0 || Object.is(value, -0) ? '-' : '') + out;
}

/** toml_writer's float: `nan` without a sign, `0.0`, and `.0` on a whole number. */
function formatFloat(value: number): string {
	if (Number.isNaN(value)) return 'nan';
	if (value === 0) return Object.is(value, -0) ? '-0.0' : '0.0';
	if (Number.isFinite(value) && value % 1 === 0) return `${rustFloat(value)}.0`;
	return rustFloat(value);
}

interface Metrics {
	singles: number;
	doubles: number;
	codes: boolean;
	escape: boolean;
	newline: boolean;
}

function metrics(text: string): Metrics {
	const out: Metrics = {
		singles: 0,
		doubles: 0,
		codes: false,
		escape: false,
		newline: false,
	};
	let singles = 0;
	let doubles = 0;
	for (const byte of new TextEncoder().encode(text)) {
		singles = byte === 0x27 ? singles + 1 : 0;
		doubles = byte === 0x22 ? doubles + 1 : 0;
		out.singles = Math.max(out.singles, singles);
		out.doubles = Math.max(out.doubles, doubles);
		if (byte === 0x5c) out.escape = true;
		else if (byte === 0x0a) out.newline = true;
		else if (byte !== 0x09 && (byte <= 0x1f || byte === 0x7f)) out.codes = true;
	}
	return out;
}

/** `write_toml_value` for the basic encodings, escaping as toml_writer does. */
function escaped(text: string, multiline: boolean): string {
	let out = '';
	let doubles = 0;
	for (const character of text) {
		const code = character.codePointAt(0) as number;
		if (code === 0x22) {
			doubles++;
			if (doubles > (multiline ? 2 : 0)) {
				out += '\\"';
				doubles = 0;
				continue;
			}
			out += character;
			continue;
		}
		doubles = 0;
		const named: Record<number, string> = {
			8: '\\b',
			9: '\\t',
			12: '\\f',
			13: '\\r',
			92: '\\\\',
		};
		if (named[code] !== undefined) out += named[code];
		else if (code === 0x0a) out += multiline ? '\n' : '\\n';
		else if (code <= 0x1f || code === 0x7f)
			out += `\\u${code.toString(16).toUpperCase().padStart(4, '0')}`;
		else out += character;
	}
	return out;
}

/** `TomlStringBuilder::as_default`: the plainest encoding the text allows. */
export function formatString(text: string): string {
	const m = metrics(text);
	if (!m.codes && !m.escape && m.doubles === 0 && !m.newline)
		return `"${escaped(text, false)}"`;
	if (!m.codes && m.singles === 0 && !m.newline) return `'${text}'`;
	const lead = m.newline ? '\n' : '';
	if (!m.codes && !m.escape && m.doubles <= 2)
		return `"""${lead}${escaped(text, true)}"""`;
	if (!m.codes && m.singles <= 2) return `'''${lead}${text}'''`;
	return m.newline
		? `"""${lead}${escaped(text, true)}"""`
		: `"${escaped(text, false)}"`;
}

/** `TomlKeyBuilder::as_default`. */
function formatKey(key: string): string {
	if (/^[A-Za-z0-9_-]+$/.test(key)) return key;
	const bytes = new TextEncoder().encode(key);
	const has = (test: (b: number) => boolean) => bytes.some(test);
	const codes = has((b) => b !== 0x09 && (b <= 0x1f || b === 0x7f));
	if (!codes && !has((b) => b === 0x5c) && !has((b) => b === 0x22))
		return `"${key}"`;
	if (!codes && !has((b) => b === 0x27)) return `'${key}'`;
	return `"${escaped(key, false)}"`;
}

/** `toml::Value`'s `Display`. */
export function formatValue(value: TomlValue): string {
	switch (value.type) {
		case 'string':
			return formatString(value.value);
		case 'integer':
			return value.value.toString();
		case 'float':
			return formatFloat(value.value);
		case 'boolean':
			return String(value.value);
		case 'datetime':
			return formatDatetime(value.value);
		case 'array':
			return `[${value.value.map(formatValue).join(', ')}]`;
		case 'table': {
			const parts = [...value.value.entries()].map(
				([key, child]) => ` ${formatKey(key)} = ${formatValue(child)}`,
			);
			return parts.length === 0 ? '{}' : `{${parts.join(',')} }`;
		}
	}
}
