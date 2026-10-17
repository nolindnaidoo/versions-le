/**
 * Requires the npm command line to behave exactly as the Rust one does.
 *
 * Both install a command named `versions-le`, so a person or a CI step cannot tell
 * which one they have. This is a different contract from
 * `check-extraction-parity.ts`, which holds the extension's engine equal to
 * the crate's: here the whole terminal surface is compared, the walk, the
 * findings and refusals, the labels, the exit codes, stdout and stderr
 * byte for byte, and every reply `versions-le mcp` writes, byte for byte too.
 *
 * Every case runs both binaries over one generated tree with stdout piped,
 * which is how a script sees them. `--version` is compared too: every surface
 * carries one version, and two binaries of one name disagreeing about it is
 * the first thing a person comparing them would see.
 *
 * Run: bun run build:cli && cargo build --locked --manifest-path crate/Cargo.toml
 *      bun scripts/check-cli-parity.ts
 */
import { spawnSync } from 'node:child_process';
import {
	chmodSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	rmdirSync,
	symlinkSync,
	unlinkSync,
	writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const ROOT = join(import.meta.dir, '..');
const RUST =
	process.env.VERSIONS_LE_BIN ??
	join(
		ROOT,
		'crate',
		'target',
		'debug',
		`versions-le${process.platform === 'win32' ? '.exe' : ''}`,
	);
const NODE_CLI = join(ROOT, 'cli', 'cli.js');
/**
 * Node, by name, not `process.execPath`: this script runs under Bun, and the
 * package is run by Node wherever `npx` runs it. Bun's errors are not Node's.
 */
const NODE = process.env.NODE_BIN ?? 'node';

interface Case {
	readonly args: readonly string[];
	readonly stdin?: string | Buffer;
}

interface Outcome {
	readonly status: number | null;
	readonly stdout: string;
	readonly stderr: string;
}

for (const [label, path] of [
	['the Rust binary', RUST],
	['cli/cli.js', NODE_CLI],
] as const) {
	if (!existsSync(path)) {
		console.error(`FAIL: ${label} is missing at ${path}. Build it first.`);
		process.exit(1);
	}
}

const tree = mkdtempSync(join(tmpdir(), 'versions-le-cli-parity-'));
const created: string[] = [];
const dirs: string[] = [tree];

function dir(relative: string): void {
	mkdirSync(join(tree, relative));
	dirs.push(join(tree, relative));
}

function file(relative: string, content: string | Buffer): void {
	writeFileSync(join(tree, relative), content);
	created.push(join(tree, relative));
}

function link(relative: string, target: string): void {
	// Windows grants symlinks to administrators and developer mode only, and
	// both binaries then see the same tree without the link.
	try {
		symlinkSync(target, join(tree, relative));
	} catch {
		return;
	}
	created.push(join(tree, relative));
}

const canLock = process.platform !== 'win32' && process.getuid?.() !== 0;

const npm = (deps: Record<string, string>, dev: Record<string, string> = {}) =>
	`${JSON.stringify({ name: 'x', dependencies: deps, devDependencies: dev }, null, 2)}\n`;

function build(): void {
	dir('repo');
	dir('repo/.git');
	file('repo/.gitignore', 'ignored/\n');
	file('repo/.ignore', 'dot-ignored/\n');
	file('repo/package.json', npm({ react: '^18.2.0', lodash: '4.17.21', 'left-pad': 'git+https://example.invalid/x' }));
	dir('repo/web');
	file('repo/web/package.json', npm({ react: '^17.0.0', lodash: '~4.17.0' }, { typescript: '5.x', vite: 'latest' }));
	dir('repo/api');
	file('repo/api/package.json', npm({ react: '>=16 <19', 'ünïcödé': '1.0.0' }, { typescript: '^5.4.0' }));
	file('repo/api/Cargo.toml', '[package]\nname = "api"\n\n[dependencies]\nserde = "1.0"\nregex = "1"\ntokio = { version = "1.38", features = ["full"] }\nlocal = { path = "../local" }\n');
	dir('repo/core');
	file('repo/core/Cargo.toml', '[dependencies]\nserde = "=0.9.0"\nregex = "2"\n\n[dev-dependencies]\ntokio = "1"\nsemver = "1.0"\n');
	file('repo/core/pyproject.toml', '[project]\nname = "core"\ndependencies = ["requests>=2.28", "numpy~=1.26", "semver==3.0.2"]\n');
	file('repo/pyproject.toml', '[project]\nname = "root"\ndependencies = ["requests<2.0"]\n');
	file('repo/go.mod', 'module example.invalid/x\n\ngo 1.22\n\nrequire (\n\tgithub.com/pkg/errors v0.9.1\n\tgolang.org/x/text v0.14.0\n)\n');
	dir('repo/svc');
	file('repo/svc/go.mod', 'module example.invalid/svc\n\nrequire github.com/pkg/errors v0.8.0\n');
	dir('repo/.github');
	dir('repo/.github/workflows');
	file('repo/.github/workflows/ci.yml', 'jobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - uses: actions/setup-node@v4\n        with:\n          node-version: 20\n');
	file('repo/.github/workflows/release.yaml', 'jobs:\n  ship:\n    steps:\n      - uses: actions/checkout@v3\n      - uses: actions/setup-node@0123456789abcdef0123456789abcdef01234567\n        with:\n          node-version: 18\n');
	file('repo/.github/dependabot.yml', 'version: 2\n');
	dir('repo/broken');
	file('repo/broken/package.json', '{"dependencies": {"react": "^18"\n');
	file('repo/broken/Cargo.toml', '[dependencies\nserde = "1"\n');
	dir('repo/bom');
	file('repo/bom/package.json', `\uFEFF${npm({ react: '18.2.0' })}`);
	dir('repo/latin1');
	file('repo/latin1/package.json', Buffer.from([0x7b, 0x22, 0xe9, 0x22, 0x3a, 0x31, 0x7d, 0x0a]));
	dir('repo/node_modules');
	dir('repo/node_modules/dep');
	file('repo/node_modules/dep/package.json', npm({ react: '0.1.0' }));
	dir('repo/vendor');
	file('repo/vendor/go.mod', 'module v\n\nrequire github.com/pkg/errors v0.1.0\n');
	dir('repo/.hidden');
	file('repo/.hidden/package.json', npm({ react: '0.2.0' }));
	dir('repo/ignored');
	file('repo/ignored/package.json', npm({ react: '0.3.0' }));
	dir('repo/dot-ignored');
	file('repo/dot-ignored/package.json', npm({ react: '0.4.0' }));
	dir('repo/docs');
	file('repo/docs/package.json.bak', npm({ react: '0.5.0' }));
	file('repo/docs/notes.md', 'react 0.6.0\n');
	dir('repo/ünï');
	file('repo/ünï/package.json', npm({ lodash: '3.0.0' }));
	link('repo/linked.json', 'package.json');
	link('repo/linked-dir', 'web');
	dir('loose');
	file('loose/package.json', npm({ react: '15.0.0' }));
	// A name in two ecosystems is refused as the answer, not as a limit,
	// so --strict passes it.
	dir('cross');
	file('cross/package.json', npm({ semver: '^7.0.0' }));
	file('cross/Cargo.toml', '[dependencies]\nsemver = "1.0"\n');
	dir('none');
	file('none/README.md', 'nothing\n');
	dir('clean');
	file('clean/package.json', npm({ react: '^18.2.0' }));
	dir('clean/b');
	file('clean/b/package.json', npm({ react: '^18.3.0' }));
	if (canLock) {
		dir('closed');
		file('closed/package.json', npm({ react: '1.0.0' }));
		dir('closed/shut');
		file('closed/shut/package.json', npm({ react: '2.0.0' }));
		chmodSync(join(tree, 'closed', 'shut'), 0o000);
		dir('locked');
		file('locked/package.json', npm({ react: '1.0.0' }));
		chmodSync(join(tree, 'locked', 'package.json'), 0o000);
	}
}

function cases(): readonly Case[] {
	const repo = join(tree, 'repo');
	const at = (name: string) => join(repo, name);
	const corpus = join(ROOT, 'crate', 'fixtures', 'documents');
	return [
		{ args: ['--help'] },
		{ args: ['-h'] },
		{ args: ['--version'] },
		{ args: ['-V'] },
		{ args: [repo, '--help'] },
		{ args: [] },
		{ args: [repo] },
		{ args: [`${repo}/`] },
		{ args: ['repo'] },
		{ args: ['./repo/'] },
		{ args: ['repo/./web'] },
		{ args: [repo, '--hidden'] },
		{ args: [repo, '--no-ignore'] },
		{ args: [repo, '--hidden', '--no-ignore'] },
		{ args: [repo, '--strict'] },
		{ args: [repo, '--fail-on', 'any'] },
		{ args: [repo, '--fail-on', 'conflict'] },
		{ args: [repo, '--fail-on', 'some'] },
		{ args: [repo, '--fail-on'] },
		{ args: [repo, '--ecosystem', 'npm'] },
		{ args: [repo, '--ecosystem', 'cargo', '--ecosystem', 'python'] },
		{ args: [repo, '--ecosystem', 'go', '--ecosystem', 'ci'] },
		{ args: [repo, '--ecosystem', 'NPM'] },
		{ args: [repo, '--ecosystem', 'rust'] },
		{ args: [repo, '--ecosystem'] },
		{ args: [repo, '--exclude', 'web/*'] },
		{ args: [repo, '--exclude', 'package.json'] },
		{ args: [repo, '--exclude', '**/Cargo.toml', '--exclude', '*.yml'] },
		{ args: [repo, '--exclude', '{api,core}/**'] },
		{ args: [repo, '--exclude', '.github/**'] },
		{ args: [repo, '--exclude', 'ün?/*'] },
		{ args: [repo, '--exclude', '[!a]*/package.json'] },
		{ args: [repo, '--exclude', '[a'] },
		{ args: [repo, '--exclude', 'a}'] },
		{ args: [repo, '--exclude', '*\\'] },
		{ args: [repo, '--exclude', 'p?ckage.*'] },
		{ args: [repo, '--exclude'] },
		{ args: [repo, '--nope'] },
		{ args: [repo, '-'] },
		{ args: [at('package.json')] },
		{ args: [at('package.json'), '--exclude', 'package.json'] },
		{ args: [at('package.json'), '--ecosystem', 'cargo'] },
		{ args: [at('api/Cargo.toml'), at('core/Cargo.toml')] },
		{ args: [at('core/pyproject.toml'), at('pyproject.toml')] },
		{ args: [at('go.mod'), at('svc/go.mod')] },
		{ args: [at('.github/workflows/ci.yml'), at('.github/workflows/release.yaml')] },
		{ args: [at('.github')] },
		{ args: [at('.github/dependabot.yml')] },
		{ args: [at('docs/notes.md')] },
		{ args: [at('broken')] },
		{ args: [at('broken'), '--strict'] },
		{ args: [at('bom'), at('latin1')] },
		{ args: [at('latin1'), '--strict'] },
		{ args: [at('ünï')] },
		{ args: [at('linked.json')] },
		{ args: [at('linked-dir')] },
		{ args: [at('node_modules')] },
		{ args: [at('.hidden')] },
		{ args: ['repo', 'loose'] },
		{ args: ['./repo', './loose/'] },
		{ args: [repo, repo] },
		{ args: ['.', 'loose'] },
		{ args: [join(tree, 'none')] },
		{ args: [join(tree, 'cross')] },
		{ args: [join(tree, 'cross'), '--strict'] },
		{ args: ['./repo//package.json'] },
		{ args: ['repo/./api/../api/Cargo.toml'] },
		{ args: [join(tree, 'clean')] },
		{ args: [join(tree, 'clean'), '--fail-on', 'any'] },
		...(canLock
			? [
					{ args: [join(tree, 'closed')] },
					{ args: [join(tree, 'locked')] },
					{ args: [join(tree, 'locked'), '--strict'] },
				]
			: []),
		{ args: [join(tree, 'missing')] },
		{ args: [repo, join(tree, 'missing')] },
		{ args: [join(tree, 'mïssing')] },
		{ args: [corpus] },
		{ args: [corpus, '--hidden', '--no-ignore', '--strict'] },
	];
}

function run(command: readonly string[], testCase: Case): Outcome {
	const [program, ...rest] = command as [string, ...string[]];
	const result = spawnSync(program, [...rest, ...testCase.args], {
		input: testCase.stdin ?? '',
		cwd: tree,
		maxBuffer: 256 * 1024 * 1024,
	});
	return {
		status: result.status,
		stdout: result.stdout.toString('utf8'),
		stderr: result.stderr.toString('utf8'),
	};
}

function compareCli(): string[] {
	const failures: string[] = [];
	for (const testCase of cases()) {
		const rust = run([RUST], testCase);
		const node = run([NODE, NODE_CLI], testCase);
		for (const field of ['status', 'stdout', 'stderr'] as const) {
			if (rust[field] === node[field]) continue;
			failures.push(
				`versions-le ${testCase.args.join(' ')}\n  ${field} differs\n` +
					describeDifference(String(rust[field]), String(node[field])),
			);
		}
	}
	return failures;
}

function mcpLines(): readonly string[] {
	const repo = join(tree, 'repo');
	let id = 0;
	const request = (method: string, params?: unknown) =>
		JSON.stringify({
			jsonrpc: '2.0',
			id: ++id,
			method,
			...(params === undefined ? {} : { params }),
		});
	const call = (name: string, args?: unknown) =>
		request('tools/call', args === undefined ? { name } : { name, arguments: args });
	const compare = (args: unknown) => call('compare_versions', args);
	const check = (args: unknown) => call('versions_le_check', args);
	const files = [
		{ path: 'a/package.json', content: npm({ react: '^18.2.0' }) },
		{ path: 'b/package.json', content: npm({ react: '^17.0.0' }) },
		{ path: 'c/Cargo.toml', content: '[dependencies]\nserde = "1"\n' },
	];
	return [
		request('initialize', { protocolVersion: '2024-11-05' }),
		JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }),
		request('tools/list'),
		request('ping'),
		request('resources/list'),
		request('tools/call'),
		request('tools/call', { arguments: {} }),
		request('tools/call', null),
		call('no_such_tool', {}),
		JSON.stringify({ jsonrpc: '2.0', id: 'tëxt-id', method: 'ping' }),
		JSON.stringify({ jsonrpc: '2.0', id: null, method: 'ping' }),
		JSON.stringify({ id: 77, method: 'ping' }),
		JSON.stringify({ jsonrpc: '2.0', id: 78 }),
		'not json',
		'',
		'   ',
		'[1, 2]',
		`${request('ping')}\r`,
		compare({ files }),
		compare({ files, maxResults: 1 }),
		compare({ files: [] }),
		compare({ files: [{ path: 'README.md', content: 'x' }] }),
		compare({ files, extra: 1 }),
		compare(null),
		compare([1]),
		call('compare_versions'),
		check({ path: repo }),
		check({ path: repo, ecosystems: ['npm', 'ci'], exclude: ['web/*'] }),
		check({ path: repo, hidden: true, ignored: true }),
		check({ path: repo, hidden: null, exclude: null }),
		check({ path: repo, hidden: 'yes' }),
		check({ path: repo, ecosystems: ['rust'] }),
		check({ path: repo, ecosystems: 'npm' }),
		check({ path: repo, exclude: [1] }),
		check({ path: repo, zebra: 1, alpha: 2 }),
		check({ path: join(repo, 'package.json') }),
		check({ path: join(repo, 'broken') }),
		check({ path: 3 }),
		check({}),
		check(null),
		check('x'),
		check({ path: join(tree, 'missing') }),
		...(canLock ? [check({ path: join(tree, 'closed') }), check({ path: join(tree, 'locked') })] : []),
	];
}

function serve(command: readonly string[], input: string | Buffer): Outcome {
	const [program, ...rest] = command as [string, ...string[]];
	const result = spawnSync(program, [...rest, 'mcp'], { input, cwd: tree });
	return {
		status: result.status,
		stdout: result.stdout.toString('utf8'),
		stderr: result.stderr.toString('utf8'),
	};
}

function compareMcp(): string[] {
	const failures: string[] = [];
	const lines = mcpLines();
	const sessions: readonly (readonly [string, string | Buffer])[] = [
		['the whole session', `${lines.join('\n')}\n`],
		['a last line with no newline', lines[3] as string],
		[
			'a line that is not UTF-8',
			Buffer.concat([
				Buffer.from(`${lines[3]}\n`),
				Buffer.from([0x7b, 0xff, 0x7d, 0x0a]),
				Buffer.from(`${lines[3]}\n`),
			]),
		],
	];
	for (const [label, input] of sessions) {
		const rust = serve([RUST], input);
		const node = serve([NODE, NODE_CLI], input);
		for (const field of ['status', 'stdout', 'stderr'] as const) {
			if (rust[field] === node[field]) continue;
			failures.push(
				`versions-le mcp, ${label}\n  ${field} differs\n` +
					describeDifference(String(rust[field]), String(node[field])),
			);
		}
	}
	return failures;
}

/** The first differing line, which is the one worth reading in a long output. */
function describeDifference(rust: string, node: string): string {
	const left = rust.split('\n');
	const right = node.split('\n');
	const index = left.findIndex((line, at) => line !== right[at]);
	const at = index === -1 ? left.length : index;
	const clip = (line: string | undefined) =>
		line === undefined || line.length <= 2000 ? line : `${line.slice(0, 2000)}…`;
	return `  line ${at + 1}\n  rust: ${clip(left[at])}\n  node: ${clip(right[at])}`;
}

function cleanUp(): void {
	const unlock: readonly (readonly [string, number])[] = [
		[join(tree, 'closed', 'shut'), 0o700],
		[join(tree, 'locked', 'package.json'), 0o600],
	];
	for (const [path, mode] of unlock) if (existsSync(path)) chmodSync(path, mode);
	for (const path of created) unlinkSync(path);
	for (const path of [...dirs].reverse()) rmdirSync(path);
}

build();
let failures: string[];
try {
	failures = [...compareCli(), ...compareMcp()];
} finally {
	cleanUp();
}

if (failures.length > 0) {
	console.error(failures.join('\n\n'));
	console.error(`\nFAIL: ${failures.length} difference(s) between the npm and Rust CLIs`);
	process.exit(1);
}
console.log(
	`OK: the npm CLI matches the Rust CLI on ${cases().length} commands and ${mcpLines().length} MCP lines`,
);
