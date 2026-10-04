import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { TOOLS } from '../mcp/tools';
import { formatValue, parseToml } from './toml';

/**
 * Two tables of answers read back from the crate, one row per case: what toml
 * accepts and how it displays it, and what the crate's own server reports for
 * one manifest. They pin the paths a corpus written for the CLI does not visit
 * — every string form, every number base, every grammar's refusals.
 */
const load = (name: string) =>
	JSON.parse(readFileSync(join(__dirname, '__fixtures__', name), 'utf8'));

describe('toml cases, from the crate', () => {
	for (const { document, display } of load('toml-cases.json').cases as Array<{
		document: string;
		display: string | null;
	}>) {
		it(JSON.stringify(document), () => {
			const parsed = parseToml(document);
			expect(
				parsed === undefined
					? null
					: formatValue({ type: 'table', value: parsed }),
			).toBe(display);
		});
	}
});

interface ManifestCase {
	readonly path: string;
	readonly content: string;
	readonly diagnostics: string[];
	readonly findings: string[];
	readonly refusals: string[];
	readonly entries: number;
}

describe('manifest cases, from the crate', () => {
	const tool = TOOLS[0] as (typeof TOOLS)[number];
	for (const c of load('manifest-cases.json').cases as ManifestCase[]) {
		it(`${c.path}: ${JSON.stringify(c.content).slice(0, 70)}`, async () => {
			const answer = (await tool.handler({
				files: [{ path: c.path, content: c.content }],
			})) as {
				diagnostics: Array<{ message: string }>;
				data: {
					findings: Array<{ code: string; name: string; message: string }>;
					refusals: Array<{ reason: string; name: string; message: string }>;
					summary: { entries: number };
				};
			};
			expect(answer.diagnostics.map((d) => d.message)).toEqual(c.diagnostics);
			expect(
				answer.data.findings.map((f) => `${f.code} ${f.name}: ${f.message}`),
			).toEqual(c.findings);
			expect(
				answer.data.refusals.map((r) => `${r.reason} ${r.name}: ${r.message}`),
			).toEqual(c.refusals);
			expect(answer.data.summary.entries).toBe(c.entries);
		});
	}
});
