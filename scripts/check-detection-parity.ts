/**
 * Fails when the extension's detection drifts from the shared corpus.
 *
 * - fixtures/detection.json must reproduce under the port: every path's
 *   classification, every document's entries and refusals, and every
 *   analysis's findings and refusals.
 * - fixtures/mcp-compare-versions.json must reproduce under the npm server's
 *   tool, and — when the release binary is built — under the crate's server.
 *
 * The documents are addressed by path through the crate's own table in
 * `crate/src/detect/corpus.rs`, read here rather than restated.
 *
 * Run: bun scripts/check-detection-parity.ts   (VERSIONS_LE_BIN=<path> for the binary)
 */
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { analyse } from '../src/detect/compare';
import { ECOSYSTEM_OF, manifestKind } from '../src/detect/heuristics';
import { type Entry, parse, type Refusal } from '../src/detect/parser';
import { compareStrings } from '../src/detect/semver';
import { TOOLS } from '../src/mcp/tools';

const ROOT = join(import.meta.dir, '..');
const CRATE = join(ROOT, 'crate');
const BINARY = process.env.VERSIONS_LE_BIN ?? join(CRATE, 'target', 'release', 'versions-le');

const table = new Map<string, string>();
const source = readFileSync(join(CRATE, 'src', 'detect', 'corpus.rs'), 'utf8');
for (const match of source.matchAll(/"([^"]+)"\s*=>\s*\{?\s*include_str!\("\.\.\/\.\.\/fixtures\/documents\/([^"]+)"\)/g)) {
	table.set(match[1] as string, readFileSync(join(CRATE, 'fixtures', 'documents', match[2] as string), 'utf8'));
}
const document = (path: string) => {
	const content = table.get(path);
	if (content === undefined) throw new Error(`the corpus table has no document for ${path}`);
	return content;
};
const read = (path: string) => {
	const kind = manifestKind(path);
	if (kind === undefined) throw new Error(`${path} is not a manifest`);
	return parse(kind, path, document(path));
};
const KIND_DEBUG: Record<string, string> = { runtime: 'Runtime', dev: 'Dev', build: 'Build', peer: 'Peer', optional: 'Optional', engine: 'Engine', msrv: 'Msrv', tool: 'Tool', action: 'Action' };
const entryLine = (e: Entry) => `${e.ecosystem} ${KIND_DEBUG[e.kind]} ${e.site.key} ${e.name}=${e.site.constraint}`;
const refusalLine = (r: Refusal) => `${r.reason} ${r.name}`;

const failures: string[] = [];
const expectEqual = (label: string, actual: unknown, expected: unknown) => {
	if (!isDeepStrictEqual(JSON.parse(JSON.stringify(actual ?? null)), expected ?? null)) {
		failures.push(`${label}\n  got:      ${JSON.stringify(actual).slice(0, 500)}\n  expected: ${JSON.stringify(expected).slice(0, 500)}`);
	}
};

const corpus = JSON.parse(readFileSync(join(CRATE, 'fixtures', 'detection.json'), 'utf8'));
for (const c of corpus.classification) {
	const kind = manifestKind(c.path);
	expectEqual(`classification: ${c.path}`, kind === undefined ? null : ECOSYSTEM_OF[kind], c.ecosystem);
}
for (const c of corpus.extraction) {
	const parsed = read(c.file);
	expectEqual(`extraction entries: ${c.file}`, parsed.entries.map(entryLine), c.entries);
	expectEqual(`extraction refusals: ${c.file}`, parsed.refusals.map(refusalLine), c.refusals);
}
for (const c of corpus.analysis) {
	const parsed = c.files.map(read);
	const { findings, refusals } = analyse(parsed.flatMap((p: { entries: Entry[] }) => p.entries));
	expectEqual(`analysis findings: ${c.name}`, findings.map((f) => `${f.severity} ${f.code} ${f.ecosystem} ${f.name}`), c.findings);
	const lines = [...parsed.flatMap((p: { refusals: Refusal[] }) => p.refusals), ...refusals].map(refusalLine).sort(compareStrings);
	expectEqual(`analysis refusals: ${c.name}`, lines, c.refusals);
}

const mcp = JSON.parse(readFileSync(join(CRATE, 'fixtures', 'mcp-compare-versions.json'), 'utf8'));
const argumentsOf = (c: { files?: string[]; arguments: Record<string, unknown> }) => ({
	...(c.files ? { files: c.files.map((path) => ({ path, content: document(path) })) } : {}),
	...c.arguments,
});
const tool = TOOLS[0] as (typeof TOOLS)[number];
for (const c of mcp) {
	try {
		const answer = JSON.parse(JSON.stringify(await tool.handler(argumentsOf(c))));
		if (c.expectedError !== undefined) failures.push(`npm server answered "${c.name}", which the corpus refuses`);
		for (const [key, value] of Object.entries(c.expected ?? {})) expectEqual(`npm server: ${c.name} (${key})`, answer[key], value);
	} catch (error) {
		expectEqual(`npm server refusal: ${c.name}`, (error as Error).message, c.expectedError);
	}
}

let crateChecked = false;
if (existsSync(BINARY)) {
	crateChecked = true;
	const child = Bun.spawn([BINARY, 'mcp'], { stdin: 'pipe', stdout: 'pipe' });
	const out = new Response(child.stdout).text();
	child.stdin.write(
		`${mcp.map((c: never, id: number) => JSON.stringify({ jsonrpc: '2.0', id, method: 'tools/call', params: { name: 'compare_versions', arguments: argumentsOf(c) } })).join('\n')}\n`,
	);
	child.stdin.end();
	for (const line of (await out).trim().split('\n')) {
		const response = JSON.parse(line);
		const c = mcp[response.id];
		if (response.result.isError) {
			expectEqual(`crate server refusal: ${c.name}`, response.result.content[0].text, c.expectedError);
			continue;
		}
		for (const [key, value] of Object.entries(c.expected ?? {})) {
			expectEqual(`crate server: ${c.name} (${key})`, response.result.structuredContent[key], value);
		}
	}
}

if (failures.length > 0) {
	console.error(`PARITY FAILED — ${failures.length} difference(s):\n`);
	for (const f of failures) console.error(`${f}\n`);
	process.exit(1);
}
console.log(
	`OK: every detection.json case reproduces under the port (${table.size} documents), and every mcp-compare-versions.json case under the npm server${crateChecked ? ' and the crate server' : ' (no binary built, so the crate server was not asked)'}.`,
);
