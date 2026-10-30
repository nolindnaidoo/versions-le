import { Readable } from 'node:stream';
import { describe, expect, it } from 'vitest';
import { parse } from './args';
import { normalise } from './discover';
import { asBytes, compileGlob } from './glob';
import { serve } from './mcp';
import { FLAGS, USAGE } from './usage';

const matches = (glob: string, path: string) =>
	compileGlob(glob, false)?.test(asBytes(path)) ?? 'refused';

describe('usage', () => {
	it('names every flag the parser accepts, and no other', () => {
		const named = new Set(
			USAGE.split(/\s+/)
				.filter((word) => word.startsWith('--'))
				.map((word) => word.replace(/[,.:;)]+$/, ''))
				.filter((word) => word !== '--version' && word !== '--help'),
		);
		expect([...named].sort()).toEqual([...FLAGS].sort());
	});
});

describe('parse', () => {
	it('reads the current directory when no root is named', () => {
		expect(parse([]).roots).toEqual(['.']);
	});

	it('refuses what the crate refuses, in its words', () => {
		expect(() => parse(['--ecosystem', 'rust'])).toThrow(
			'rust is not an ecosystem. Try npm, cargo, python, go or ci.',
		);
		expect(() => parse(['--fail-on', 'some'])).toThrow(
			'some is not conflict or any',
		);
		expect(() => parse(['--exclude'])).toThrow('--exclude needs a pattern');
	});
});

// Every expectation below is globset 0.4.20's own answer, from a reference
// binary run over the same globs and paths.
describe('compileGlob', () => {
	it('keeps * and ? inside one component', () => {
		expect(matches('*.json', 'a/b.json')).toBe(false);
		expect(matches('a/*', 'a/b/c')).toBe(false);
		expect(matches('?', '/')).toBe(false);
	});

	it('reads ** only where a separator bounds it', () => {
		expect(matches('**/package.json', 'package.json')).toBe(true);
		expect(matches('a/**', 'a')).toBe(false);
		expect(matches('a/**/b', 'a/b')).toBe(true);
		expect(matches('a**', 'a/x')).toBe(false);
	});

	it('matches bytes, not characters', () => {
		expect(matches('?', 'é')).toBe(false);
		expect(matches('??', 'é')).toBe(true);
		expect(matches('*', '\r')).toBe(true);
	});

	it('refuses the globs globset refuses', () => {
		for (const glob of ['[a', 'a}', '{a', '[z-a]', 'a\\']) {
			expect(matches(glob, 'a')).toBe('refused');
		}
	});
});

describe('normalise', () => {
	it('drops what Path::components drops', () => {
		expect(normalise('./repo//a/./b/')).toBe('./repo/a/b');
		expect(normalise('/abs/../x')).toBe('/abs/../x');
	});
});

describe('mcp', () => {
	it('refuses arguments that are not an object, as the crate does', async () => {
		const written: string[] = [];
		const request = {
			jsonrpc: '2.0',
			id: 1,
			method: 'tools/call',
			params: { name: 'versions_le_check', arguments: 5 },
		};
		await serve(
			Readable.from([Buffer.from(`${JSON.stringify(request)}\n`)]),
			(reply) => written.push(reply),
			'9.9.9',
		);
		const reply = JSON.parse(written[0] ?? '{}');
		expect(reply.result.isError).toBe(true);
		expect(reply.result.content[0].text).toBe(
			'this tool takes an object of arguments',
		);
	});

	it('reads null arguments as none, as every server in the family does', async () => {
		// `null` is how a client spells "not supplied"; it was refused as not
		// being an object, and the crate's check-cli-parity holds both
		// command lines to one answer.
		const reply = async (args: unknown) => {
			const written: string[] = [];
			const request = {
				jsonrpc: '2.0',
				id: 1,
				method: 'tools/call',
				params: { name: 'compare_versions', arguments: args },
			};
			await serve(
				Readable.from([Buffer.from(`${JSON.stringify(request)}\n`)]),
				(line) => {
					written.push(line);
					return undefined;
				},
				'9.9.9',
			);
			return JSON.parse(written[0] ?? '{}');
		};
		expect(await reply(null)).toEqual(await reply({}));
		expect((await reply(null)).result.content[0].text).not.toBe(
			'this tool takes an object of arguments',
		);
	});
});
