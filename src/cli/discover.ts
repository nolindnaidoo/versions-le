/**
 * Finding the manifests under a root, as the crate's `discover.rs` does with
 * ripgrep's `ignore` crate.
 *
 * Hidden files are walked: `.github/workflows` is a manifest location. A
 * hidden directory is descended only under `--hidden`, `.github` always, and
 * `node_modules`, `.git` and `vendor` never. A root that is a file is read
 * whatever the exclusions say: you named it. A directory the walk cannot enter
 * ends the run, as an error from the crate's walk does.
 */
import { type Dirent, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, resolve, sep } from 'node:path';
import {
	basename,
	ECOSYSTEM_OF,
	type Ecosystem,
	type ManifestKind,
	manifestKind,
} from '../detect/heuristics';
import { asBytes, compileGlob } from './glob';
import {
	type DirectoryRules,
	isIgnored,
	type RepositoryRules,
	repositoryRoot,
	repositoryRules,
	rulesFor,
} from './ignoreRules';
import { decodeUtf8OrThrow, rustIoMessage, withoutBom } from './ioError';

const windows = sep === '\\';

export interface DiscoverOptions {
	readonly hidden: boolean;
	readonly respectIgnore: boolean;
	readonly exclude: readonly string[];
	readonly ecosystems: readonly Ecosystem[];
}

export interface Found {
	readonly path: string;
	/** Below the root, `/`-separated: what the exclusions and the label read. */
	readonly relative: string;
	readonly kind: ManifestKind;
}

/** The crate's `Excludes`: a pattern with a `/` matches the path, any other the name. */
function excludes(patterns: readonly string[]): (path: string) => boolean {
	const paths: RegExp[] = [];
	const names: RegExp[] = [];
	for (const pattern of patterns) {
		const glob = compileGlob(pattern, windows);
		if (glob === null) continue;
		(pattern.includes('/') ? paths : names).push(glob);
	}
	return (filepath) => {
		const path = filepath.replaceAll('\\', '/');
		const whole = asBytes(path);
		const name = asBytes(basename(path));
		return (
			paths.some((glob) => glob.test(whole)) ||
			names.some((glob) => glob.test(name))
		);
	};
}

export function discover(root: string, options: DiscoverOptions): Found[] {
	let isFile: boolean;
	try {
		isFile = statSync(root).isFile();
	} catch (error) {
		throw new Error(`${root}: ${rustIoMessage(error)}`);
	}
	if (isFile) {
		const kind = manifestKind(normalise(root));
		if (kind === undefined) {
			throw new Error(`${root}: not a manifest this tool reads`);
		}
		return [{ path: root, relative: normalise(root), kind }];
	}
	const absolute = resolve(root);
	const repoRoot = options.respectIgnore ? repositoryRoot(absolute) : null;
	const walk: Walk = {
		root,
		options,
		repository: repoRoot ? repositoryRules(repoRoot) : null,
		excluded: excludes(options.exclude),
		found: [],
	};
	visit(walk, root, [], absolute, parentRules(absolute, options));
	return sortAndDedupe(walk.found);
}

interface Walk {
	readonly root: string;
	readonly options: DiscoverOptions;
	readonly repository: RepositoryRules | null;
	readonly excluded: (path: string) => boolean;
	readonly found: Found[];
}

/** The crate's `parents(true)`: ignore files above the walk root count too. */
function parentRules(
	absolute: string,
	options: DiscoverOptions,
): readonly DirectoryRules[] {
	if (!options.respectIgnore) return [];
	const chain: DirectoryRules[] = [];
	let dir = dirname(absolute);
	while (dir !== dirname(dir)) {
		chain.unshift(rulesFor(dir, repositoryRoot(dir) !== null));
		dir = dirname(dir);
	}
	chain.unshift(rulesFor(dir, repositoryRoot(dir) !== null));
	return chain;
}

function visit(
	walk: Walk,
	shown: string,
	below: readonly string[],
	absolute: string,
	inherited: readonly DirectoryRules[],
): void {
	let entries: readonly Dirent[];
	try {
		entries = readdirSync(absolute, { withFileTypes: true });
	} catch (error) {
		// The `ignore` crate's error, under the root the crate names first.
		throw new Error(
			`${walk.root}: ${shown}: IO error for operation on ${shown}: ${rustIoMessage(error)}`,
		);
	}
	const chain = walk.options.respectIgnore
		? [...inherited, rulesFor(absolute, repositoryRoot(absolute) !== null)]
		: inherited;
	for (const entry of entries) {
		const entryShown = joinShown(shown, entry.name);
		const entryAbsolute = `${absolute}${sep}${entry.name}`;
		const isDir = entry.isDirectory();
		if (!isDir && !entry.isFile()) continue;
		if (
			walk.options.respectIgnore &&
			isIgnored(entryAbsolute, isDir, chain, walk.repository)
		)
			continue;
		const path = [...below, entry.name];
		if (isDir) {
			if (descend(entry.name, walk.options.hidden)) {
				visit(walk, entryShown, path, entryAbsolute, chain);
			}
			continue;
		}
		const relative = path.join('/');
		const kind = manifestKind(relative);
		if (kind === undefined || walk.excluded(relative)) continue;
		const { ecosystems } = walk.options;
		if (ecosystems.length > 0 && !ecosystems.includes(ECOSYSTEM_OF[kind])) {
			continue;
		}
		walk.found.push({ path: entryShown, relative, kind });
	}
}

/** The crate's `descend`: never into a vendored tree, into a hidden one on request. */
function descend(name: string, hidden: boolean): boolean {
	if (name === 'node_modules' || name === '.git' || name === 'vendor') {
		return false;
	}
	return hidden || name === '.github' || !name.startsWith('.');
}

/** `PathBuf::join`: no separator is added after one that is already there. */
function joinShown(parent: string, name: string): string {
	if (parent.endsWith('/') || parent.endsWith(sep)) return `${parent}${name}`;
	return `${parent}${sep}${name}`;
}

export function read(path: string): string {
	let bytes: Buffer;
	try {
		bytes = readFileSync(path);
	} catch (error) {
		throw new Error(`${path}: ${rustIoMessage(error)}`);
	}
	try {
		return withoutBom(decodeUtf8OrThrow(bytes));
	} catch {
		throw new Error(`${path}: not UTF-8 text`);
	}
}

/** The crate's `label`: below the root, or under it when there are several roots. */
export function label(found: Found, root: string, qualify: boolean): string {
	if (isFileQuietly(root)) return normalise(root);
	const normalRoot = normalise(root);
	if (!qualify || normalRoot === '.' || normalRoot === '')
		return found.relative;
	return `${normalRoot.replace(/\/+$/, '')}/${found.relative}`;
}

/** `Path::is_file`, which answers false rather than failing. */
function isFileQuietly(path: string): boolean {
	try {
		return statSync(path).isFile();
	} catch {
		return false;
	}
}

export function nameOf(path: string): string {
	return basename(normalise(path));
}

/**
 * The crate's `normalise`: a path's components joined with `/`, as
 * `Path::components` yields them, so repeated and trailing separators and an
 * interior `.` disappear, and a Windows drive is written upper-case.
 */
export function normalise(path: string): string {
	let rest = path;
	let out = '';
	if (windows && /^[A-Za-z]:/.test(rest)) {
		out = `${rest.charAt(0).toUpperCase()}:`;
		rest = rest.slice(2);
	}
	const separators = windows ? /[\\/]/ : /\//;
	if (separators.test(rest.charAt(0)) && !out.endsWith('/')) out += '/';
	const parts = rest
		.split(separators)
		.filter((part, index) => part !== '' && (part !== '.' || index === 0));
	for (const part of parts) {
		if (out !== '' && !out.endsWith('/')) out += '/';
		out += part;
	}
	return out;
}

type Component = readonly [rank: number, bytes: Buffer];

/** A path as Rust compares it: by component, a name by its bytes. */
function components(path: string): readonly Component[] {
	const separators = windows ? /[\\/]/ : /\//;
	const parts = path.split(separators);
	const found: Component[] = [];
	if (parts[0] === '' && parts.length > 1) found.push([1, Buffer.alloc(0)]);
	parts.forEach((part, index) => {
		if (!part) return;
		if (part === '.') {
			if (index === 0) found.push([2, Buffer.alloc(0)]);
			return;
		}
		found.push([part === '..' ? 3 : 4, Buffer.from(part)]);
	});
	return found;
}

function compareComponents(
	left: readonly Component[],
	right: readonly Component[],
): number {
	const shared = Math.min(left.length, right.length);
	for (let index = 0; index < shared; index++) {
		const [leftRank, leftBytes] = left[index] as Component;
		const [rightRank, rightBytes] = right[index] as Component;
		if (leftRank !== rightRank) return leftRank - rightRank;
		const order = Buffer.compare(leftBytes, rightBytes);
		if (order !== 0) return order;
	}
	return left.length - right.length;
}

/** `sort_by(path)` then `dedup`. */
function sortAndDedupe(found: readonly Found[]): Found[] {
	const keyed = found.map((item) => ({ item, key: components(item.path) }));
	keyed.sort((left, right) => compareComponents(left.key, right.key));
	return keyed
		.filter((entry, index) => {
			const previous = keyed[index - 1];
			return !previous || compareComponents(previous.key, entry.key) !== 0;
		})
		.map((entry) => entry.item);
}
