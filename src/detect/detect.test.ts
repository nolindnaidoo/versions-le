import { describe, expect, it } from 'vitest';
import { cargo, npm, python } from './grammar';
import { manifestKind, normalisePath } from './heuristics';
import { reportFor } from './report';

const cargoFiles = (a: string, b: string) => [
	{
		kind: 'cargo-toml' as const,
		label: 'api/Cargo.toml',
		content: `[dependencies]\nregex = ${a}\n`,
	},
	{
		kind: 'cargo-toml' as const,
		label: 'web/Cargo.toml',
		content: `[dependencies]\nregex = ${b}\n`,
	},
];

describe('the grammars', () => {
	it('reads a spaced npm comparator as one, as node-semver does', () => {
		expect(npm('>= 20')).toEqual(npm('>=20'));
	});

	it('names what it will not model instead of approximating it', () => {
		expect(npm('workspace:*')).toMatchObject({ kind: 'unknown' });
		expect(python('~=1.4')).toMatchObject({ kind: 'unknown' });
		expect(cargo('not a version')).toMatchObject({ kind: 'malformed' });
	});
});

describe('the report', () => {
	it('escalates to disjoint-constraint when no version satisfies both', () => {
		const report = reportFor(cargoFiles('"1"', '"2"'));
		expect(report.findings.map((f) => f.code)).toContain('disjoint-constraint');
	});

	it('does not call two spellings of one requirement a conflict', () => {
		const report = reportFor(cargoFiles('"1.2"', '"^1.2.0"'));
		expect(
			report.findings.filter((f) => f.code.includes('constraint')),
		).toEqual([]);
	});

	it('reports a manifest that does not parse, and compares nothing in it', () => {
		const report = reportFor([
			{ kind: 'cargo-toml', label: 'Cargo.toml', content: 'a = ' },
		]);
		expect(report.diagnostics).toEqual([
			{
				severity: 'error',
				code: 'parse-error',
				file: 'Cargo.toml',
				message: 'not valid TOML',
			},
		]);
		expect(report.summary.entries).toBe(0);
	});
});

describe('paths', () => {
	it('reads a workflow by its directory, and normalises a path as std::path does', () => {
		expect(manifestKind('.github/workflows/ci.yml')).toBe('workflow');
		expect(manifestKind('k8s/deploy.yml')).toBeUndefined();
		expect(normalisePath('./a//b/./c/')).toBe('./a/b/c');
		expect(normalisePath('/x/./y')).toBe('/x/y');
	});
});
