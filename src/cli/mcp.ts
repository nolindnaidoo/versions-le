/**
 * `versions-le mcp`: the crate's MCP server (`crate/src/mcp/`), not the
 * extension's. It offers `versions_le_check` beside the shared
 * `compare_versions`, answers in the crate's words, and writes every reply
 * with its keys sorted, as a `serde_json::Value` without `preserve_order` does.
 */

import { ECOSYSTEMS, type Ecosystem } from '../detect/heuristics';
import { envelope, sortedKeys } from '../mcp/envelope';
import { only, TOOLS } from '../mcp/tools';
import { frames, type Request } from './frames';
import { scan } from './scan';

const PROTOCOL_VERSION = '2025-06-18';
const PARSE_ERROR = -32700;
const INVALID_PARAMS = -32602;
const METHOD_NOT_FOUND = -32601;

type Handled =
	| { readonly result: unknown }
	| { readonly error: { readonly code: number; readonly message: string } };

const SHARED = sharedTool('compare_versions');

const CHECK_ARGUMENTS = ['path', 'ecosystems', 'exclude', 'hidden', 'ignored'];

const CHECK_DEFINITION = {
	name: 'versions_le_check',
	description:
		'Find the manifests in a directory and report where the same dependency is constrained inconsistently. Reads the filesystem; never writes to it and never resolves a dependency or reaches the network. Comparison never crosses an ecosystem, and a constraint in a grammar the tool does not model is reported in `refusals` rather than guessed at.',
	inputSchema: {
		type: 'object',
		properties: {
			path: { type: 'string', description: 'the directory to analyse' },
			ecosystems: {
				type: 'array',
				items: {
					type: 'string',
					enum: ['npm', 'cargo', 'python', 'go', 'ci'],
				},
				description: 'Limit the walk to these ecosystems. Empty means all.',
			},
			exclude: {
				type: 'array',
				items: { type: 'string' },
				description: 'Glob patterns for manifests to skip.',
			},
			hidden: {
				type: 'boolean',
				default: false,
				description:
					'Descend hidden directories too. .github is always walked.',
			},
			ignored: {
				type: 'boolean',
				default: false,
				description: 'Walk files excluded by .gitignore too.',
			},
		},
		required: ['path'],
		additionalProperties: false,
	},
};

/** The shared tool is the extension's, definition and handler both. */
function sharedTool(name: string) {
	const tool = TOOLS.find((candidate) => candidate.name === name);
	if (!tool) throw new Error(`the extension offers no tool named ${name}`);
	return tool;
}

/**
 * Serves until stdin closes, one request at a time, in order, over the
 * crate's framing in `frames.ts`. A frame the framing refuses is answered
 * with a parse error, as the crate answers it.
 */
export async function serve(
	input: AsyncIterable<Uint8Array>,
	write: (line: string) => void,
	version: string,
): Promise<number> {
	for await (const frame of frames(input)) {
		if ('refusal' in frame) {
			write(
				`${reply({ id: null, error: { code: PARSE_ERROR, message: frame.refusal } })}\n`,
			);
			continue;
		}
		const response = await handle(frame.request, version);
		if (response !== undefined) write(`${response}\n`);
	}
	return 0;
}

/** One reply as serde_json writes it: keys in byte order at every level. */
function reply(body: Record<string, unknown>): string {
	return JSON.stringify(sortedKeys({ jsonrpc: '2.0', ...body }));
}

function field(value: unknown, name: string): unknown {
	if (typeof value !== 'object' || value === null || Array.isArray(value)) {
		return undefined;
	}
	return Object.hasOwn(value, name)
		? (value as Record<string, unknown>)[name]
		: undefined;
}

async function handle(
	request: Request,
	version: string,
): Promise<string | undefined> {
	// A notification carries no id, or a null one, and gets no reply.
	if (request.id === undefined || request.id === null) return undefined;
	const handled = await dispatch(request.method, request.params, version);
	return reply({ id: request.id, ...handled });
}

async function dispatch(
	method: string,
	params: unknown,
	version: string,
): Promise<Handled> {
	if (method === 'initialize') {
		return {
			result: {
				protocolVersion: PROTOCOL_VERSION,
				capabilities: { tools: {} },
				serverInfo: { name: 'versions-le', version },
			},
		};
	}
	if (method === 'tools/list') {
		return {
			result: {
				tools: [
					{
						name: SHARED.name,
						description: SHARED.description,
						inputSchema: SHARED.inputSchema,
					},
					CHECK_DEFINITION,
				],
			},
		};
	}
	if (method === 'tools/call') return callTool(params);
	if (method === 'ping') return { result: {} };
	return {
		error: { code: METHOD_NOT_FOUND, message: `Unknown method: ${method}` },
	};
}

/**
 * Protocol failures are JSON-RPC errors. A tool that fails on its arguments
 * returns a result carrying `isError`, so a model reads the reason rather than
 * concluding the server is broken.
 */
async function callTool(params: unknown): Promise<Handled> {
	const name = field(params, 'name');
	if (typeof name !== 'string') {
		return {
			error: {
				code: INVALID_PARAMS,
				message: 'tools/call requires a string `name`',
			},
		};
	}
	// Absent or null arguments read as an empty object, since a null is how a
	// client spells "not supplied"; anything else present reaches the tool as
	// sent, and both tools refuse what is not an object, as the crate's do.
	const raw = field(params, 'arguments');
	const args = (raw ?? {}) as Record<string, unknown>;
	if (name === 'compare_versions') {
		return { result: await outcome(() => SHARED.handler(args)) };
	}
	if (name === 'versions_le_check') {
		return { result: await outcome(async () => check(args)) };
	}
	return {
		error: { code: INVALID_PARAMS, message: `Unknown tool: ${name}` },
	};
}

async function outcome(tool: () => Promise<unknown>): Promise<unknown> {
	let envelope: unknown;
	try {
		envelope = sortedKeys(await tool());
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		return { content: [{ type: 'text', text: message }], isError: true };
	}
	return {
		content: [{ type: 'text', text: JSON.stringify(envelope, null, 2) }],
		structuredContent: envelope,
		isError: false,
	};
}

/** The crate's `check_tool`: the arguments in its order, then one scan. */
function check(raw: Record<string, unknown>): unknown {
	const args = only(raw, CHECK_ARGUMENTS, 'this tool');
	if (typeof args.path !== 'string') {
		throw new Error('no directory was supplied to analyse');
	}
	const ecosystems = list(args, 'ecosystems').map((name): Ecosystem => {
		const found = ECOSYSTEMS.find((candidate) => candidate === name);
		if (found === undefined) {
			throw new Error(`${name} is not one of npm, cargo, python, go or ci`);
		}
		return found;
	});
	const options = {
		hidden: flag(args, 'hidden'),
		respectIgnore: !flag(args, 'ignored'),
		exclude: list(args, 'exclude'),
		ecosystems,
	};
	const report = scan([args.path], options);
	const diagnostics = report.diagnostics.map((diagnostic) => ({
		severity: diagnostic.severity,
		code: diagnostic.code,
		message: `${diagnostic.file}: ${diagnostic.message}`,
	}));
	return envelope(
		'versions_le_check',
		report,
		report.findings.length,
		diagnostics,
		false,
	);
}

/** serde_json's `get`, where an explicit `null` counts as not supplied. */
function supplied(args: Record<string, unknown>, name: string): unknown {
	return Object.hasOwn(args, name) && args[name] !== null
		? args[name]
		: undefined;
}

function flag(args: Record<string, unknown>, name: string): boolean {
	const value = supplied(args, name);
	if (value === undefined) return false;
	if (typeof value !== 'boolean')
		throw new Error(`${name} takes true or false`);
	return value;
}

function list(args: Record<string, unknown>, name: string): string[] {
	const value = supplied(args, name);
	if (value === undefined) return [];
	if (
		!Array.isArray(value) ||
		!value.every((item) => typeof item === 'string')
	) {
		throw new Error(`${name} takes a list of strings`);
	}
	return value;
}
