import { describe, expect, it } from 'vitest';
import { frames } from './frames';

async function read(...chunks: (string | number[])[]): Promise<unknown[]> {
	const input = (async function* () {
		for (const chunk of chunks) {
			yield typeof chunk === 'string'
				? Buffer.from(chunk)
				: Uint8Array.from(chunk);
		}
	})();
	const out: unknown[] = [];
	for await (const frame of frames(input)) out.push(frame);
	return out;
}

const ping = (id: unknown) => ({
	request: { jsonrpc: '2.0', id, method: 'ping' },
});
const invalid = { refusal: 'Invalid JSON' };
const notARequest = { refusal: 'Not a JSON-RPC 2.0 request' };

describe('frames', () => {
	it('answers a line that is not UTF-8 with a parse error and reads on', async () => {
		expect(
			await read(
				[0xff, 0xfe, 10],
				'{"jsonrpc":"2.0","id":1,"method":"ping"}\n',
			),
		).toEqual([invalid, ping(1)]);
	});

	it('reads an invalid byte inside a string as a replacement character', async () => {
		const [frame] = await read([
			...Buffer.from('{"jsonrpc":"2.0","id":2,"method":"ping","x":"caf'),
			0xe9,
			...Buffer.from('"}\n'),
		]);
		expect(frame).toEqual({
			request: { jsonrpc: '2.0', id: 2, method: 'ping', x: 'caf�' },
		});
	});

	it('refuses a line that is not JSON, a BOM-led one included', async () => {
		expect(
			await read('nope\n﻿{"jsonrpc":"2.0","id":1,"method":"ping"}\n'),
		).toEqual([invalid, invalid]);
	});

	it('refuses JSON that is not a JSON-RPC 2.0 request', async () => {
		expect(
			await read(
				'{"id":1}\n{"id":1,"method":"ping"}\n{"jsonrpc":"1.0","id":1,"method":"ping"}\n{"jsonrpc":"2.0","id":1,"method":7}\n[1,2,3]\n5\n',
			),
		).toEqual([
			notARequest,
			notARequest,
			notARequest,
			notARequest,
			notARequest,
			notARequest,
		]);
	});

	it('reassembles a frame split across chunks, and drops one trailing CR', async () => {
		expect(
			await read('{"jsonrpc":"2.0","id"', ':1,"method":"ping"}\r\n'),
		).toEqual([ping(1)]);
	});

	it('drops a last line with no newline: a partial frame at close was never a request', async () => {
		expect(
			await read(
				'{"jsonrpc":"2.0","id":1,"method":"ping"}\n{"jsonrpc":"2.0","id":2,"method":"ping"}',
			),
		).toEqual([ping(1)]);
	});

	it("skips a line Rust's trim leaves empty, and keeps a null id for the server to read as a notification", async () => {
		expect(
			await read('\u0085 \t\n{"jsonrpc":"2.0","id":null,"method":"ping"}\n'),
		).toEqual([ping(null)]);
	});
});
