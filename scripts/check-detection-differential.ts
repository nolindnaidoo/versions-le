/**
 * `compare_versions` is offered by BOTH servers — the npm one in
 * `src/mcp/tools.ts` and the Rust one in `crate/src/mcp/compare.rs`. One tool
 * name, one schema, two implementations, so the contract is identical output.
 *
 * `crate/fixtures/mcp-compare-versions.json` pins the cases somebody thought
 * of. This generates them: sets of manifests in all five kinds, with
 * constraints in every grammar — modelled, unmodelled, malformed and moving —
 * shared names across files and ecosystems, MSRV pins against CI toolchains,
 * broken JSON and TOML, and malformed arguments. serde_json, toml and semver
 * are transcribed in this repo, so this is where a difference in what any of
 * them accepts would show.
 *
 * Run: bun scripts/check-detection-differential.ts
 *   VERSIONS_LE_DIFFERENTIAL_SEED=<n>  reproduce a specific failure
 *   VERSIONS_LE_DIFFERENTIAL_CASES=<n> how many calls (default 1500)
 *   VERSIONS_LE_BIN=<path>             the Rust binary (default the release build)
 */
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { TOOLS } from '../src/mcp/tools';

const ROOT = join(import.meta.dir, '..');
const BINARY = process.env.VERSIONS_LE_BIN ?? join(ROOT, 'crate', 'target', 'release', 'versions-le');
const SEED = Number(process.env.VERSIONS_LE_DIFFERENTIAL_SEED ?? 20261004);
const CASES = Number(process.env.VERSIONS_LE_DIFFERENTIAL_CASES ?? 1500);

function seeded(seed: number): () => number {
	let state = seed >>> 0;
	return () => {
		state = (state + 0x6d2b79f5) >>> 0;
		let t = state;
		t = Math.imul(t ^ (t >>> 15), t | 1);
		t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
}

const NAMES = ['serde', 'regex', 'left-pad', 'semver', 'react', 'tokio', 'bun', 'node', 'requests', 'golang.org/x/net', 'café', '😀pkg', 'a', 'Zeta'];
const NPM = ['^1.2.3', '~0.4.1', '>=1 <2', '>= 20', '1.2.3 - 2.0.0', '1.x', '*', '', 'latest', 'next', 'workspace:*', 'file:../x', 'git+https://x/y.git', 'user/repo', '^0.2.3', '>2 <1', '<1.80 || >=1.90', '1.2.3-beta.1', '>=', '^ 1.0.0', '1.2.3.4', 'v2', '=1.0.0', '>1.2', '<=1.2', '^0.0.3', 'not a version', '1.0.0+build', '>=1.0.0-rc.1 <2'];
const CARGO = ['1', '1.0.200', '^0.4', '~1.2.3', '>=1.0, <2', '=0.3.1', '*', '0.4', '1.*', '>1, <1', '', ' ', 'abc', '1.2.3-alpha', '>=1.2.3-pre.01', 'x', '^1, ^2'];
const PEP = ['>=3.10', '>=1.0,<2.0', '==1.2', '~=1.4', '!=1.5', '==1.*', '>2,<1', '>=2.0.0rc1', '1.0', '', '>=1.0.post1', '==1!2.0', ',', '<3.12'];
const GO = ['v1.2.3', 'v0.0.0-20230101000000-abcdef123456', '1.2.3', 'vv1.0.0', 'v1.2', 'v1.2.3+incompatible', 'garbage'];

function cargoValue(random: () => number, pick: <T>(l: readonly T[]) => T): string {
	const r = random();
	const v = pick(CARGO);
	if (r < 0.55) return JSON.stringify(v);
	if (r < 0.7) return `{ version = ${JSON.stringify(v)}${random() < 0.4 ? ', features = ["x"]' : ''}${random() < 0.2 ? `, package = ${JSON.stringify(pick(NAMES))}` : ''} }`;
	if (r < 0.78) return pick(['{ workspace = true }', '{ path = "../x" }', '{ git = "https://x" }', '{ path = "../x", version = "1.2" }', '{ optional = true }']);
	return pick(['1', '1.5', 'true', '[1, "a"]', '1979-05-27', '{ a = { b = 1 } }', 'nan', '"a\tb"', '[{ x = "y" }]', '-0.0', '1e30', '0.1']);
}

function manifest(random: () => number, pick: <T>(l: readonly T[]) => T): { path: string; content: string } {
	const dir = pick(['', 'api/', 'web/', 'crates/x/', 'a/b/']);
	const kind = pick(['npm', 'cargo', 'cargo', 'python', 'go', 'ci', 'ci']);
	const count = 1 + Math.floor(random() * 5);
	if (kind === 'npm') {
		const sections = ['dependencies', 'devDependencies', 'peerDependencies', 'optionalDependencies', 'engines'];
		const body: Record<string, Record<string, unknown>> = {};
		for (let i = 0; i < count; i++) {
			const section = pick(sections);
			body[section] = { ...(body[section] ?? {}), [pick(NAMES)]: random() < 0.07 ? pick([1, null, ['^1'], { v: 1 }]) : pick(NPM) };
		}
		let content = JSON.stringify({ name: 'x', ...body, ...(random() < 0.3 ? { packageManager: pick(['bun@1.1.0', 'pnpm@9.0.0+sha512.abc', 'yarn', 'npm@latest']) } : {}) }, null, 2);
		if (random() < 0.08) content = pick([content.slice(0, -2), `${content},`, content.replace('{', '{"a":1,"a":2,'), '{"dependencies": {"x": "1"}} trailing', '{"a": 1e400}', '{"a": "\u0000"}', '[1, 2]', '"str"']);
		return { path: `${dir}package.json`, content };
	}
	if (kind === 'cargo') {
		const lines = random() < 0.6 ? ['[package]', 'name = "x"', `rust-version = ${JSON.stringify(pick(['1.88', '1.70', '1.85.0', '1.x', '*', 'abc']))}`] : [];
		const sections = ['dependencies', 'dev-dependencies', 'build-dependencies', 'workspace.dependencies', "target.'cfg(windows)'.dependencies"];
		const used = new Set<string>();
		for (let i = 0; i < count; i++) {
			const section = pick(sections);
			if (used.has(section)) continue;
			used.add(section);
			lines.push(`[${section}]`);
			const names = new Set<string>();
			for (let j = 0; j < 1 + Math.floor(random() * 3); j++) {
				const name = pick(NAMES).replace(/[^A-Za-z0-9_-]/g, '');
				if (name === '' || names.has(name)) continue;
				names.add(name);
				lines.push(`${name} = ${cargoValue(random, pick)}`);
			}
		}
		let content = `${lines.join('\n')}\n`;
		if (random() < 0.08) content = pick([`${content}[dependencies]\n`, `${content}x = `, `${content}a = 1\na = 2\n`, 'not toml at all', `${content}[package]\n`]);
		return { path: `${dir}Cargo.toml`, content };
	}
	if (kind === 'python') {
		const deps = Array.from({ length: count }, () => {
			const r = random();
			if (r < 0.1) return pick(['requests[security', 'urllib3; python_version < "3.11"', 'pkg @ https://x/y.whl', '>=1.0', 'name (>=1.0)']);
			return `${pick(NAMES).replace(/[^A-Za-z0-9._-]/g, '')}${pick(PEP)}`;
		});
		let content = `[project]\nname = "x"\nrequires-python = ${JSON.stringify(pick(PEP))}\ndependencies = [\n${deps.map((d) => `  ${JSON.stringify(d)},`).join('\n')}\n]\n`;
		if (random() < 0.3) content += `[project.optional-dependencies]\ndev = [${JSON.stringify(`pytest${pick(PEP)}`)}, ${random() < 0.2 ? '1' : '"black"'}]\n`;
		return { path: `${dir}pyproject.toml`, content };
	}
	if (kind === 'go') {
		const lines = ['module example.com/x', '', `go ${pick(['1.22', '1.21.0', '1.x', 'abc'])}`, '', 'require ('];
		for (let i = 0; i < count; i++) lines.push(`\t${pick(NAMES)} ${pick(GO)}${random() < 0.15 ? ' // indirect' : ''}`);
		lines.push(')', `require example.com/y ${pick(GO)}`, random() < 0.1 ? 'require lonely' : '');
		return { path: `${dir}go.mod`, content: lines.join(random() < 0.1 ? '\r\n' : '\n') };
	}
	const lines = ['name: ci', 'jobs:', '  test:', '    steps:'];
	for (let i = 0; i < count; i++) {
		const r = random();
		if (r < 0.4) lines.push(`      - uses: ${pick(['actions/checkout', 'actions/setup-node', 'dtolnay/rust-toolchain', 'docker://alpine', './local'])}@${pick(['v4', 'v4.1.0', 'main', '1.88', 'stable', '8e5e7e5ab8b370d6c329ec480221332ada57f0ab', 'abc1234', 'v4 # pinned'])}`);
		else lines.push(`        ${pick(['node-version', 'bun-version', 'python-version', 'toolchain', 'go-version'])}: ${pick(['20', '"18.x"', "'lts/*'", '${{ matrix.node }}', '1.85', 'latest', '[18, 20]', '>=3.10', 'v1.2', 'stable', '1.88.0 # msrv'])}`);
	}
	return { path: `${pick(['', 'sub/'])}.github/workflows/${pick(['ci.yml', 'release.yaml'])}`, content: lines.join('\n') };
}

interface Generated {
	readonly name: string;
	readonly args: Record<string, unknown>;
}

function generate(count: number, seed: number): Generated[] {
	const random = seeded(seed);
	const pick = <T>(list: readonly T[]): T => list[Math.floor(random() * list.length)] as T;
	const out: Generated[] = [];
	for (let index = 0; index < count; index++) {
		const files = Array.from({ length: 1 + Math.floor(random() * 5) }, () => manifest(random, pick));
		let args: Record<string, unknown> = { files };
		const r = random();
		if (r < 0.08) args.maxResults = 1 + Math.floor(random() * 3);
		else if (r < 0.1) args = pick([{}, { files: [] }, { files, extra: 1 }, { files: [{ path: 'x/settings.json', content: '{}' }] }, { files: [{ path: 'package.json' }] }, { files: ['package.json'] }, { files: [{ path: 'package.json', content: '{}', z: 1 }] }, { files, maxResults: 0 }, { files, maxResults: 99999 }]);
		out.push({ name: `${index}`, args });
	}
	return out;
}

function canonical(value: unknown): string {
	if (value === null || typeof value !== 'object') return JSON.stringify(value);
	if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
	const entries = Object.entries(value as Record<string, unknown>)
		.filter(([, item]) => item !== undefined)
		.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
	return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${canonical(v)}`).join(',')}}`;
}

async function fromNpm(documents: readonly Generated[]): Promise<string[]> {
	const tool = TOOLS.find((candidate) => candidate.name === 'compare_versions');
	if (!tool) throw new Error('the npm server no longer offers compare_versions');
	const answers: string[] = [];
	for (const document of documents) {
		try {
			answers.push(canonical(JSON.parse(JSON.stringify(await tool.handler(document.args)))));
		} catch (error) {
			answers.push(`error: ${error instanceof Error ? error.message : String(error)}`);
		}
	}
	return answers;
}

async function fromCrate(documents: readonly Generated[]): Promise<string[]> {
	if (!existsSync(BINARY)) throw new Error(`no binary at ${BINARY} — build it first: cd crate && cargo build --release`);
	const child = Bun.spawn([BINARY, 'mcp'], { stdin: 'pipe', stdout: 'pipe', stderr: 'pipe' });
	const draining = new Response(child.stdout).text();
	child.stdin.write(
		`${documents
			.map((document, id) =>
				JSON.stringify({ jsonrpc: '2.0', id, method: 'tools/call', params: { name: 'compare_versions', arguments: document.args } }),
			)
			.join('\n')}\n`,
	);
	child.stdin.end();
	const stdout = await draining;
	await child.exited;
	const answers: string[] = new Array(documents.length);
	for (const line of stdout.split('\n')) {
		if (line.trim().length === 0) continue;
		const response = JSON.parse(line) as {
			id: number;
			result?: { structuredContent?: unknown; isError?: boolean; content?: { text: string }[] };
			error?: unknown;
		};
		if (response.error !== undefined) throw new Error(`the crate server refused document ${response.id}: ${JSON.stringify(response.error)}`);
		answers[response.id] = response.result?.isError
			? `error: ${response.result.content?.[0]?.text}`
			: canonical(response.result?.structuredContent);
	}
	const missing = answers.findIndex((answer) => answer === undefined);
	if (missing !== -1) throw new Error(`the crate server never answered document ${missing}: ${await new Response(child.stderr).text()}`);
	return answers;
}

const documents = generate(CASES, SEED);
console.log(`differential: ${documents.length} generated documents, seed ${SEED}, binary ${BINARY.replace(ROOT, '.')}`);
const [npm, crate] = await Promise.all([fromNpm(documents), fromCrate(documents)]);
const failures: string[] = [];
let findings = 0;
let refusals = 0;
let errors = 0;
for (const [index, document] of documents.entries()) {
	const ours = npm[index] as string;
	if (!ours.startsWith('error:')) {
		const answer = JSON.parse(ours);
		findings += answer.data.summary.findings;
		refusals += answer.data.summary.refusals;
		errors += answer.diagnostics.length;
	}
	if (ours !== crate[index]) {
		failures.push(
			`the two compare_versions servers disagree on "${document.name}"\n  arguments: ${JSON.stringify(document.args).slice(0, 700)}\n  npm:   ${ours.slice(0, 900)}\n  crate: ${(crate[index] as string).slice(0, 900)}`,
		);
	}
}
console.log(`  ${findings} findings, ${refusals} refusals, ${errors} manifest diagnostics, ${npm.filter((a) => a.startsWith('error:')).length} refused calls`);
if (failures.length > 0) {
	console.error(`\nDIFFERENTIAL FAILED — ${failures.length} problem(s):\n`);
	for (const failure of failures.slice(0, 8)) console.error(`${failure}\n`);
	process.exit(1);
}
console.log('OK: both compare_versions servers gave identical answers on every document.');
