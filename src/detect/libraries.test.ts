import { describe, expect, it } from 'vitest';
import { parseJson } from './json';
import {
	compareVersions,
	parsePrerelease,
	parseVersion,
	parseVersionReq,
	type Version,
} from './semver';
import { formatValue, parseToml, rustFloat } from './toml';

/**
 * The three transcribed crates, at the cases worth pinning. Every expectation
 * was read back from the real crate — toml 1.1.6, semver 1.0.28, serde_json
 * 1.0.151 — not written from the source.
 */
const display = (text: string) => {
	const parsed = parseToml(text);
	return parsed === undefined
		? undefined
		: formatValue({ type: 'table', value: parsed });
};

describe('toml 1.1.6', () => {
	it('lets a dotted key extend a table a header made, as the crate does and the spec suite does not', () => {
		expect(display('[a.b.c]\n[a]\nb.k = 1')).toBe(
			'{ a = { b = { c = {}, k = 1 } } }',
		);
	});

	it('walks a dotted key into the last table of an array of tables', () => {
		expect(display('[[a.c]]\n[a]\nc.d.e = 1')).toBe(
			'{ a = { c = [{ d = { e = 1 } }] } }',
		);
	});

	it('refuses a header over a table dotted keys made, an i64 overflow, and a float that overflows', () => {
		expect(display('a.b = 1\n[a]')).toBeUndefined();
		expect(display('v = 0x8000000000000000')).toBeUndefined();
		expect(display('w = 1e400')).toBeUndefined();
	});

	it('accepts a time without seconds and a space before it (TOML 1.1)', () => {
		expect(display('d = 1979-05-27 07:32')).toBe('{ d = 1979-05-27T07:32 }');
	});

	it('writes values as toml_writer does: plain floats, escaped tabs, sorted inline keys, the plainest quoting', () => {
		expect(
			display('x = [1.5, "t\\tb", 1e21, nan, -0.0, { b = 2, a = "q\\"" }]'),
		).toBe(
			`{ x = [1.5, "t\\tb", 1000000000000000000000.0, nan, -0.0, { a = 'q"', b = 2 }] }`,
		);
	});

	it("prints an f64 as Rust's Display does, never in exponent form", () => {
		expect(rustFloat(5e-7)).toBe('0.0000005');
		expect(rustFloat(0.1 + 0.2)).toBe('0.30000000000000004');
		expect(rustFloat(1e21)).toBe('1000000000000000000000');
	});
});

describe('semver 1.0.28', () => {
	it('reads a requirement, a lone wildcard, and refuses a 33rd comparator', () => {
		expect(parseVersionReq('>=1.2, <2')?.map((c) => c.op)).toEqual([
			'greaterEq',
			'less',
		]);
		expect(parseVersionReq(' * ')).toEqual([]);
		expect(
			parseVersionReq(Array.from({ length: 33 }, () => '1').join(',')),
		).toBeUndefined();
		expect(parseVersionReq('1.*.3')).toBeUndefined();
	});

	it('refuses a leading zero in a numeric prerelease identifier, and nowhere in build metadata', () => {
		expect(parsePrerelease('rc.01')).toBeUndefined();
		expect(parseVersion('1.0.0+001')?.build).toBe('001');
	});

	it('orders prereleases below their release, numbers below words, and builds by value then length', () => {
		const v = (text: string) => parseVersion(text) as Version;
		expect(compareVersions(v('1.0.0-rc.1'), v('1.0.0'))).toBe(-1);
		expect(compareVersions(v('1.0.0-1'), v('1.0.0-a'))).toBe(-1);
		expect(compareVersions(v('1.0.0-alpha'), v('1.0.0-alpha.1'))).toBe(-1);
		expect(compareVersions(v('1.0.0+01'), v('1.0.0+1'))).toBe(1);
	});
});

describe('serde_json 1.0.151 into Value', () => {
	it('keeps the last of a repeated key and sorts keys by byte', () => {
		const node = parseJson(
			'{"b": "x", "a": "1", "b": "y", "é": "e", "Z": "z"}',
		);
		expect(node.kind === 'object' && node.entries.map(([key]) => key)).toEqual([
			'Z',
			'a',
			'b',
			'é',
		]);
		expect(
			node.kind === 'object' && node.entries.find(([key]) => key === 'b')?.[1],
		).toEqual({ kind: 'text', text: 'y' });
	});

	it('refuses what JSON.parse would accept: a number past f64 and nesting past 128 inside an array', () => {
		expect(() => parseJson('{"a": 1e400}')).toThrow('number out of range');
		expect(() => parseJson(`${'['.repeat(128)}${']'.repeat(128)}`)).toThrow(
			'recursion limit exceeded',
		);
	});
});
