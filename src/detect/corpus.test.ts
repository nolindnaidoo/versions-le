import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { analyse } from './compare';
import { ECOSYSTEM_OF, manifestKind } from './heuristics';
import { type Entry, parse, type Refusal } from './parser';
import { compareStrings } from './semver';

/**
 * The crate's `fixtures/detection.json`, run through the port: the same three
 * sections `crate/src/detect/corpus.rs` checks, over the documents that file's
 * own table names.
 */
const CRATE = join(__dirname, '..', '..', 'crate');
const table = new Map<string, string>();
for (const match of readFileSync(
	join(CRATE, 'src', 'detect', 'corpus.rs'),
	'utf8',
).matchAll(
	/"([^"]+)"\s*=>\s*\{?\s*include_str!\("\.\.\/\.\.\/fixtures\/documents\/([^"]+)"\)/g,
)) {
	table.set(
		match[1] as string,
		readFileSync(
			join(CRATE, 'fixtures', 'documents', match[2] as string),
			'utf8',
		),
	);
}
const corpus = JSON.parse(
	readFileSync(join(CRATE, 'fixtures', 'detection.json'), 'utf8'),
);
const read = (path: string) =>
	parse(manifestKind(path) as never, path, table.get(path) as string);
const KIND: Record<string, string> = {
	runtime: 'Runtime',
	dev: 'Dev',
	build: 'Build',
	peer: 'Peer',
	optional: 'Optional',
	engine: 'Engine',
	msrv: 'Msrv',
	tool: 'Tool',
	action: 'Action',
};
const entryLine = (e: Entry) =>
	`${e.ecosystem} ${KIND[e.kind]} ${e.site.key} ${e.name}=${e.site.constraint}`;
const refusalLine = (r: Refusal) => `${r.reason} ${r.name}`;

describe('the shared corpus', () => {
	it('reads every document the crate table names', () => {
		expect(table.size).toBeGreaterThanOrEqual(20);
	});

	it('classifies every path the same', () => {
		for (const c of corpus.classification) {
			const kind = manifestKind(c.path);
			expect(kind === undefined ? null : ECOSYSTEM_OF[kind], c.path).toBe(
				c.ecosystem,
			);
		}
	});

	for (const c of corpus.extraction) {
		it(`extracts ${c.file} the same`, () => {
			const parsed = read(c.file);
			expect(parsed.entries.map(entryLine)).toEqual(c.entries);
			expect(parsed.refusals.map(refusalLine)).toEqual(c.refusals);
		});
	}

	for (const c of corpus.analysis) {
		it(`analyses ${c.name} the same`, () => {
			const parsed = c.files.map(read);
			const { findings, refusals } = analyse(
				parsed.flatMap((p: { entries: Entry[] }) => p.entries),
			);
			expect(
				findings.map((f) => `${f.severity} ${f.code} ${f.ecosystem} ${f.name}`),
			).toEqual(c.findings);
			expect(
				[
					...parsed.flatMap((p: { refusals: Refusal[] }) => p.refusals),
					...refusals,
				]
					.map(refusalLine)
					.sort(compareStrings),
			).toEqual(c.refusals);
		});
	}
});
