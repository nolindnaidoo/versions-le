/**
 * The crate MCP server's framing, which `versions-le mcp` reproduces and
 * `check:cli-pipes` holds: newline-delimited JSON, a line read with
 * replacement characters where it is not UTF-8, as `from_utf8_lossy` reads
 * it, and trimmed as Rust's `str::trim` trims. A blank line is skipped. A
 * line that is not JSON, or not a JSON-RPC 2.0 request, is a parse error the
 * server answers, in the words the extension's server uses, so a client that
 * sent one learns what happened; dropping it cost the client every request
 * it then waited on. A partial frame at close was never a request. A byte
 * order mark is not whitespace to either parser, so a frame led by one is
 * not JSON.
 */
export interface Request {
	readonly jsonrpc: '2.0';
	readonly method: string;
	readonly id?: unknown;
	readonly params?: unknown;
}

export type Frame =
	| { readonly request: Request }
	| { readonly refusal: 'Invalid JSON' | 'Not a JSON-RPC 2.0 request' };

export async function* frames(
	input: AsyncIterable<Uint8Array>,
): AsyncGenerator<Frame> {
	for await (const line of lines(input)) {
		const text = rustTrim(line);
		if (text === '') continue;
		let parsed: unknown;
		try {
			parsed = JSON.parse(text);
		} catch {
			yield { refusal: 'Invalid JSON' };
			continue;
		}
		if (!isRequest(parsed)) {
			yield { refusal: 'Not a JSON-RPC 2.0 request' };
			continue;
		}
		yield { request: parsed };
	}
}

/** Complete lines, split on `\n`; whatever follows the last newline at close is dropped. */
async function* lines(
	input: AsyncIterable<Uint8Array>,
): AsyncGenerator<string> {
	const decoder = new TextDecoder('utf-8', { ignoreBOM: true });
	let pending = Buffer.alloc(0);
	for await (const chunk of input) {
		pending = Buffer.concat([pending, chunk]);
		let newline = pending.indexOf(10);
		while (newline !== -1) {
			yield decoder.decode(pending.subarray(0, newline));
			pending = pending.subarray(newline + 1);
			newline = pending.indexOf(10);
		}
	}
}

function isRequest(value: unknown): value is Request {
	if (typeof value !== 'object' || value === null || Array.isArray(value)) {
		return false;
	}
	const candidate = value as Record<string, unknown>;
	return candidate.jsonrpc === '2.0' && typeof candidate.method === 'string';
}

/**
 * Rust's `str::trim`: `char::is_whitespace` at both ends. JavaScript's `trim`
 * also strips a byte order mark, which the crate reads as not JSON.
 */
function rustTrim(line: string): string {
	const space =
		'[\\t\\n\\v\\f\\r \\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000]';
	return line.replace(new RegExp(`^${space}+|${space}+$`, 'g'), '');
}
