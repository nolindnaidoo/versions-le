import type { Constraint } from './grammar';
import * as grammar from './grammar';
import {
	type Ecosystem,
	looksLikeAVersion,
	type ManifestKind,
} from './heuristics';
import { type Node, parseJson } from './json';
import { isWhitespace, lines, trim } from './text';
import { formatValue, parseToml, type TomlValue } from './toml';

/**
 * The manifest readers — the crate's `parser.rs`. Each turns one manifest into
 * entries, refusals and parse errors, never a guess.
 */

export interface Site {
	readonly file: string;
	readonly key: string;
	readonly constraint: string;
	/** Only where a line reader honestly knows it. */
	readonly line?: number;
}

export type Kind =
	| 'runtime'
	| 'dev'
	| 'build'
	| 'peer'
	| 'optional'
	| 'engine'
	| 'msrv'
	| 'tool'
	| 'action';

export const isDevelopment = (kind: Kind) => kind === 'dev' || kind === 'build';

export interface Entry {
	readonly ecosystem: Ecosystem;
	readonly name: string;
	readonly kind: Kind;
	readonly site: Site;
	readonly constraint: Constraint;
	readonly floats: string | undefined;
}

export interface Refusal {
	readonly reason: string;
	readonly ecosystem: Ecosystem | null;
	readonly name: string;
	readonly message: string;
	readonly sites: Site[];
}

export interface Parsed {
	readonly entries: Entry[];
	readonly refusals: Refusal[];
	readonly errors: string[];
}

function site(
	file: string,
	key: string,
	constraint: string,
	line?: number,
): Site {
	return line === undefined
		? { file, key, constraint }
		: { file, key, constraint, line };
}

class Out implements Parsed {
	readonly entries: Entry[] = [];
	readonly refusals: Refusal[] = [];
	readonly errors: string[] = [];

	push(entry: Entry): void {
		if (entry.constraint.kind === 'unknown') {
			this.refusals.push({
				reason: 'unknown_grammar',
				ecosystem: entry.ecosystem,
				name: entry.name,
				message: `${entry.constraint.reason}; excluded from comparison`,
				sites: [entry.site],
			});
		}
		this.entries.push(entry);
	}

	ambiguous(ecosystem: Ecosystem, name: string, at: Site, why: string): void {
		this.refusals.push({
			reason: 'ambiguous_version_string',
			ecosystem,
			name,
			message: `${why}; excluded from comparison`,
			sites: [at],
		});
	}

	error(message: string): void {
		this.errors.push(message);
	}
}

export function parse(
	kind: ManifestKind,
	file: string,
	content: string,
): Parsed {
	const out = new Out();
	if (kind === 'package-json') packageJson(out, file, content);
	else if (kind === 'cargo-toml') cargoToml(out, file, content);
	else if (kind === 'pyproject-toml') pyprojectToml(out, file, content);
	else if (kind === 'go-mod') goMod(out, file, content);
	else workflow(out, file, content);
	return out;
}

// ---- package.json --------------------------------------------------------

const NPM_SECTIONS: ReadonlyArray<readonly [string, Kind]> = [
	['dependencies', 'runtime'],
	['devDependencies', 'dev'],
	['peerDependencies', 'peer'],
	['optionalDependencies', 'optional'],
];

const field = (node: Node, key: string): Node | undefined =>
	node.kind === 'object'
		? node.entries.find(([name]) => name === key)?.[1]
		: undefined;

function packageJson(out: Out, file: string, content: string): void {
	let root: Node;
	try {
		root = parseJson(content);
	} catch {
		// The crate reports every serde_json failure as the same sentence.
		out.error('not valid JSON');
		return;
	}
	for (const [section, kind] of NPM_SECTIONS) {
		const table = field(root, section);
		if (table?.kind !== 'object') continue;
		for (const [name, value] of table.entries) {
			if (value.kind !== 'text') {
				out.error(`${section}.${name} is not a version string`);
				continue;
			}
			out.push({
				ecosystem: 'npm',
				name,
				kind,
				site: site(file, `${section}.${name}`, value.text),
				constraint: grammar.npm(value.text),
				floats: npmFloats(value.text),
			});
		}
	}
	const engines = field(root, 'engines');
	if (engines?.kind === 'object') {
		for (const [name, value] of engines.entries) {
			if (value.kind !== 'text') {
				out.error(`engines.${name} is not a version string`);
				continue;
			}
			out.push({
				ecosystem: 'npm',
				name,
				kind: 'engine',
				site: site(file, `engines.${name}`, value.text),
				constraint: grammar.npm(value.text),
				floats: npmFloats(value.text),
			});
		}
	}
	const manager = field(root, 'packageManager');
	if (manager?.kind !== 'text') return;
	const at = manager.text.indexOf('@');
	if (at === -1) {
		out.error('packageManager is not <name>@<version>');
		return;
	}
	const name = manager.text.slice(0, at);
	const version = manager.text.slice(at + 1).split('+')[0] as string;
	out.push({
		ecosystem: 'npm',
		name,
		kind: 'tool',
		site: site(file, 'packageManager', version),
		constraint: grammar.npm(version),
		floats: npmFloats(version),
	});
}

function npmFloats(raw: string): string | undefined {
	const text = trim(raw);
	if (text === '' || ['*', 'x', 'X'].includes(text))
		return 'an unbounded range accepts any future major';
	if (text === 'latest' || text === 'next')
		return 'a dist tag resolves to whatever is newest at install time';
	return zeroCaret(text);
}

function zeroCaret(text: string): string | undefined {
	if (!text.startsWith('^')) return undefined;
	const major = text.slice(1).split(/[.-]/)[0];
	return major === '0'
		? 'a caret on a 0.x version, which promises no stability'
		: undefined;
}

// ---- Cargo.toml ----------------------------------------------------------

const CARGO_SECTIONS: ReadonlyArray<readonly [string, Kind]> = [
	['dependencies', 'runtime'],
	['dev-dependencies', 'dev'],
	['build-dependencies', 'build'],
];

const get = (
	value: TomlValue | undefined,
	key: string,
): TomlValue | undefined =>
	value?.type === 'table' ? value.value.get(key) : undefined;
const dig = (root: TomlValue, path: string): TomlValue | undefined =>
	path
		.split('.')
		.reduce<TomlValue | undefined>(
			(value, segment) => get(value, segment),
			root,
		);

function cargoToml(out: Out, file: string, content: string): void {
	const parsed = parseToml(content);
	if (parsed === undefined) {
		out.error('not valid TOML');
		return;
	}
	const root: TomlValue = { type: 'table', value: parsed };
	for (const [section, kind] of CARGO_SECTIONS) {
		cargoSection(out, file, get(root, section), section, kind);
		cargoSection(
			out,
			file,
			get(get(root, 'workspace'), section),
			`workspace.${section}`,
			kind,
		);
	}
	const targets = get(root, 'target');
	if (targets?.type === 'table') {
		for (const [platform, table] of targets.value) {
			for (const [section, kind] of CARGO_SECTIONS)
				cargoSection(
					out,
					file,
					get(table, section),
					`target.${platform}.${section}`,
					kind,
				);
		}
	}
	for (const key of [
		'package.rust-version',
		'workspace.package.rust-version',
	]) {
		const raw = dig(root, key);
		if (raw?.type !== 'string') continue;
		out.push({
			ecosystem: 'cargo',
			name: 'rust',
			kind: 'msrv',
			site: site(file, key, raw.value),
			constraint: grammar.minimum(raw.value),
			floats: undefined,
		});
	}
}

function cargoSection(
	out: Out,
	file: string,
	table: TomlValue | undefined,
	key: string,
	kind: Kind,
): void {
	if (table?.type !== 'table') return;
	for (const [alias, value] of table.value) {
		const [name, raw, constraint] = cargoDependency(alias, value);
		const floats = constraint.kind === 'range' ? cargoFloats(raw) : undefined;
		out.push({
			ecosystem: 'cargo',
			name,
			kind,
			site: site(file, `${key}.${alias}`, raw),
			constraint,
			floats,
		});
	}
}

function cargoDependency(
	alias: string,
	value: TomlValue,
): [string, string, Constraint] {
	if (value.type === 'string')
		return [alias, value.value, grammar.cargo(value.value)];
	if (value.type !== 'table') {
		return [
			alias,
			formatValue(value),
			{
				kind: 'malformed',
				reason: 'not a version string or a dependency table',
			},
		];
	}
	const renamed = value.value.get('package');
	const name = renamed?.type === 'string' ? renamed.value : alias;
	const version = value.value.get('version');
	if (version?.type === 'string')
		return [name, version.value, grammar.cargo(version.value)];
	return [name, '', { kind: 'unknown', reason: noVersionReason(value.value) }];
}

function noVersionReason(table: Map<string, TomlValue>): string {
	if (table.has('workspace'))
		return 'an inherited workspace dependency carries its version elsewhere';
	if (table.has('git')) return 'a git dependency resolves outside the registry';
	if (table.has('path'))
		return 'a path dependency with no version has no registry requirement';
	return 'a dependency table with no version';
}

function cargoFloats(raw: string): string | undefined {
	const text = trim(raw);
	if (text === '' || ['*', 'x', 'X'].includes(text))
		return 'an unbounded range accepts any future major';
	const zero = zeroCaret(text);
	if (zero !== undefined) return zero;
	return /^[0-9]/.test(text) && text.startsWith('0')
		? 'a caret on a 0.x version, which promises no stability'
		: undefined;
}

// ---- pyproject.toml ------------------------------------------------------

function pyprojectToml(out: Out, file: string, content: string): void {
	const parsed = parseToml(content);
	if (parsed === undefined) {
		out.error('not valid TOML');
		return;
	}
	const root: TomlValue = { type: 'table', value: parsed };
	const requires = dig(root, 'project.requires-python');
	if (requires?.type === 'string') {
		out.push({
			ecosystem: 'python',
			name: 'python',
			kind: 'engine',
			site: site(file, 'project.requires-python', requires.value),
			constraint: grammar.python(requires.value),
			floats: pythonFloats(requires.value),
		});
	}
	pep621List(
		out,
		file,
		dig(root, 'project.dependencies'),
		'project.dependencies',
		'runtime',
	);
	const groups = dig(root, 'project.optional-dependencies');
	if (groups?.type !== 'table') return;
	for (const [group, list] of groups.value)
		pep621List(
			out,
			file,
			list,
			`project.optional-dependencies.${group}`,
			'dev',
		);
}

function pep621List(
	out: Out,
	file: string,
	list: TomlValue | undefined,
	key: string,
	kind: Kind,
): void {
	if (list?.type !== 'array') return;
	for (const item of list.value) {
		if (item.type !== 'string') {
			out.error(`${key} holds something that is not a requirement`);
			continue;
		}
		const text = item.value;
		const [name, specifier, constraint] = pep508(text);
		if (name === '') {
			out.error(`${key}: "${text}" names no distribution`);
			continue;
		}
		const floats =
			constraint.kind === 'range' ? pythonFloats(specifier) : undefined;
		out.push({
			ecosystem: 'python',
			name,
			kind,
			site: site(file, `${key}.${text}`, specifier),
			constraint,
			floats,
		});
	}
}

function pep508(text: string): [string, string, Constraint] {
	const trimmed = trim(text);
	const name = /^[A-Za-z0-9._-]*/.exec(trimmed)?.[0] ?? '';
	const rest = trim(trimmed.slice(name.length));
	if (trimmed.includes(';'))
		return [name, '', { kind: 'unknown', reason: 'an environment marker' }];
	if (rest.startsWith('@'))
		return [name, '', { kind: 'unknown', reason: 'a direct URL reference' }];
	let after = rest;
	if (rest.startsWith('[')) {
		const close = rest.indexOf(']', 1);
		if (close === -1)
			return [
				name,
				rest,
				{
					kind: 'malformed',
					reason: 'an extras group with no closing bracket',
				},
			];
		after = trim(rest.slice(close + 1));
	}
	const specifier = trim(after.replace(/^\(+/, '').replace(/\)+$/, ''));
	return [name, specifier, grammar.python(specifier)];
}

const pythonFloats = (raw: string) =>
	trim(raw) === '' ? 'no version specifier at all' : undefined;

// ---- go.mod --------------------------------------------------------------

function goMod(out: Out, file: string, content: string): void {
	let inRequire = false;
	lines(content).forEach((rawLine, index) => {
		const line = trim(stripGoComment(rawLine));
		if (line === '' || rawLine.includes('// indirect')) return;
		const number = index + 1;
		if (line === ')') {
			inRequire = false;
			return;
		}
		if (line.startsWith('go ')) {
			const raw = trim(line.slice(3));
			out.push(
				goEntry(file, 'go', 'engine', 'go', grammar.minimum(raw), raw, number),
			);
			return;
		}
		if (line.startsWith('require (')) {
			inRequire = true;
			return;
		}
		let requirement: string;
		if (line.startsWith('require '))
			requirement = trim(line.slice('require '.length));
		else if (inRequire) requirement = line;
		else return;
		const split = Array.from(requirement).findIndex(isWhitespace);
		if (split === -1) {
			out.error(`${requirement} is a require with no version`);
			return;
		}
		const characters = Array.from(requirement);
		const module = characters.slice(0, split).join('');
		const version = trim(characters.slice(split + 1).join(''));
		out.push(
			goEntry(
				file,
				module,
				'runtime',
				'require',
				grammar.go(version),
				version,
				number,
			),
		);
	});
}

function goEntry(
	file: string,
	name: string,
	kind: Kind,
	key: string,
	constraint: Constraint,
	raw: string,
	line: number,
): Entry {
	return {
		ecosystem: 'go',
		name,
		kind,
		site: site(file, key, raw, line),
		constraint,
		floats: undefined,
	};
}

function stripGoComment(line: string): string {
	const at = line.indexOf('//');
	return at === -1 ? line : line.slice(0, at);
}

// ---- .github/workflows ---------------------------------------------------

function workflow(out: Out, file: string, content: string): void {
	lines(content).forEach((rawLine, index) => {
		let line = trim(rawLine);
		while (line.startsWith('- ')) line = line.slice(2);
		line = trim(line);
		if (line.startsWith('#')) return;
		const colon = line.indexOf(':');
		if (colon === -1) return;
		const key = trim(line.slice(0, colon));
		const value = unquote(line.slice(colon + 1));
		const number = index + 1;
		if (key === 'uses') {
			workflowUses(out, file, value, number);
			return;
		}
		const tool = workflowTool(key);
		if (tool === undefined) return;
		const at = site(file, key, value, number);
		if (!looksLikeAVersion(value)) {
			out.ambiguous('ci', tool, at, 'not evidently a version');
			return;
		}
		out.push({
			ecosystem: 'ci',
			name: tool,
			kind: tool === 'rust' ? 'msrv' : 'tool',
			site: at,
			constraint: grammar.ciTool(value),
			floats: ciFloats(value),
		});
	});
}

function workflowTool(key: string): string | undefined {
	if (key === 'toolchain') return 'rust';
	if (!key.endsWith('-version')) return undefined;
	const tool = key.slice(0, -'-version'.length);
	return tool !== '' && /^[A-Za-z0-9-]+$/.test(tool) ? tool : undefined;
}

function workflowUses(
	out: Out,
	file: string,
	value: string,
	line: number,
): void {
	const at = value.lastIndexOf('@');
	if (at === -1) return;
	const action = value.slice(0, at);
	const reference = value.slice(at + 1);
	const rustToolchain =
		action.endsWith('rust-toolchain') && !grammar.isCommitSha(reference);
	const where = site(
		file,
		rustToolchain ? 'toolchain' : 'uses',
		reference,
		line,
	);
	if (rustToolchain) {
		out.push({
			ecosystem: 'ci',
			name: 'rust',
			kind: 'msrv',
			site: where,
			constraint: grammar.ciTool(reference),
			floats: ciFloats(reference),
		});
		return;
	}
	out.push({
		ecosystem: 'ci',
		name: action,
		kind: 'action',
		site: where,
		constraint: grammar.actionRef(reference),
		floats: actionFloats(reference),
	});
}

function ciFloats(raw: string): string | undefined {
	const text = trim(raw);
	if (['*', 'x', 'X'].includes(text))
		return 'an unbounded range accepts any future major';
	if (grammar.MOVING_CHANNELS.includes(text) || text.startsWith('lts/')) {
		return 'an unpinned tool version installs whatever is newest that day';
	}
	return undefined;
}

function actionFloats(reference: string): string | undefined {
	if (
		grammar.actionRef(reference).kind === 'unknown' &&
		!/^[0-9A-Fa-f]*$/.test(reference)
	) {
		return 'a branch or tag reference moves under the workflow';
	}
	return undefined;
}

/** A trailing ` #` comment is dropped unless the value is quoted; quotes are trimmed off both ends. */
function unquote(input: string): string {
	let value = trim(input);
	if (!(value.startsWith('"') || value.startsWith("'"))) {
		const at = value.indexOf(' #');
		if (at !== -1) value = value.slice(0, at);
	}
	return trim(trim(value).replace(/^["']+|["']+$/g, ''));
}
