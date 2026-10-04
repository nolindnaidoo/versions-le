import { MOVING_CHANNELS } from './grammar';
import { trim } from './text';

/**
 * The crate's `heuristics.rs`: which manifest a path is, and whether a string
 * found outside a typed key is evidently a version.
 */

/** The five grammars, in the order the crate's `Ecosystem` sorts. */
export const ECOSYSTEMS = Object.freeze([
	'cargo',
	'ci',
	'go',
	'npm',
	'python',
] as const);
export type Ecosystem = (typeof ECOSYSTEMS)[number];
export const ecosystemOrder = (ecosystem: Ecosystem) =>
	ECOSYSTEMS.indexOf(ecosystem);

export type ManifestKind =
	| 'package-json'
	| 'cargo-toml'
	| 'pyproject-toml'
	| 'go-mod'
	| 'workflow';

export const ECOSYSTEM_OF: Readonly<Record<ManifestKind, Ecosystem>> =
	Object.freeze({
		'package-json': 'npm',
		'cargo-toml': 'cargo',
		'pyproject-toml': 'python',
		'go-mod': 'go',
		workflow: 'ci',
	});

export function basename(path: string): string {
	return path.split(/[/\\]/).at(-1) as string;
}

/** The workflow test is on the directory, not the extension. */
export function manifestKind(filepath: string): ManifestKind | undefined {
	const path = filepath.replace(/\\/g, '/');
	const inside =
		path.startsWith('.github/workflows/') ||
		path.includes('/.github/workflows/');
	if (inside && (path.endsWith('.yml') || path.endsWith('.yaml')))
		return 'workflow';
	switch (basename(path)) {
		case 'package.json':
			return 'package-json';
		case 'Cargo.toml':
			return 'cargo-toml';
		case 'pyproject.toml':
			return 'pyproject-toml';
		case 'go.mod':
			return 'go-mod';
		default:
			return undefined;
	}
}

export function looksLikeAVersion(input: string): boolean {
	const text = trim(input);
	if (
		text === '' ||
		text.includes('${{') ||
		text.includes(',') ||
		text.startsWith('[')
	)
		return false;
	if (text.startsWith('v') || text.startsWith('V'))
		return /^[0-9]/.test(text.slice(1));
	if (/^[0-9^~><=*]/.test(text)) return true;
	return MOVING_CHANNELS.includes(text) || text.startsWith('lts/');
}

/**
 * `discover::normalise` on the platform the crate's server runs on: the
 * components of a path joined by `/`, a leading `.` kept and an inner one
 * dropped, repeated and trailing separators collapsed — `std::path`'s own reading.
 */
export function normalisePath(path: string): string {
	const absolute = path.startsWith('/');
	const parts = path
		.split('/')
		.filter((part, index) => part !== '' && (part !== '.' || index === 0));
	return (absolute ? '/' : '') + parts.join('/');
}
