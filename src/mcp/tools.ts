import { manifestKind, normalisePath } from '../detect/heuristics';
import { type Document, reportFor } from '../detect/report';
import { compareStrings } from '../detect/semver';
import {
	DEFAULT_MAX_RESULTS,
	type Diagnostic,
	MAX_MAX_RESULTS,
	readMaxResults,
} from './envelope';
import type { ToolDefinition } from './transport';

/**
 * The tool this server exposes: `compare_versions`, which the crate's server
 * offers too. One name, one schema, two implementations — the definition below
 * is the crate's, word for word, and `crate/fixtures/mcp-compare-versions.json`
 * pins the answers on both sides.
 */

const DESCRIPTION =
	'Compare the version constraints in a set of manifests and report where the same dependency is constrained inconsistently — including where two constraints cannot both be satisfied. Takes file contents directly and reads no filesystem. Comparison never crosses an ecosystem: an npm `semver` and a Cargo `semver` are unrelated packages that share a word. A constraint in a grammar the tool does not model is reported in `refusals` and excluded from comparison rather than guessed at.';

const INVALID_FILES =
	'files is required and must be a non-empty array of { path, content }';

const isObject = (value: unknown): value is Record<string, unknown> =>
	typeof value === 'object' && value !== null && !Array.isArray(value);

/** `additionalProperties: false`, enforced: the first unknown key in sorted order is named. */
function only(
	value: unknown,
	allowed: readonly string[],
	what: string,
): Record<string, unknown> {
	if (!isObject(value)) throw new Error(`${what} takes an object of arguments`);
	const unknown = Object.keys(value)
		.sort(compareStrings)
		.find((key) => !allowed.includes(key));
	if (unknown !== undefined)
		throw new Error(
			`${unknown} is not an argument ${what} takes. It takes ${allowed.join(', ')}.`,
		);
	return value;
}

function readFiles(args: Record<string, unknown>): Document[] {
	const items = args.files;
	if (!Array.isArray(items) || items.length === 0)
		throw new Error(INVALID_FILES);
	return items.map((item: unknown) => {
		const file = only(item, ['path', 'content'], 'a file');
		if (typeof file.path !== 'string' || typeof file.content !== 'string')
			throw new Error(INVALID_FILES);
		const kind = manifestKind(normalisePath(file.path));
		if (kind === undefined) {
			throw new Error(
				`${file.path} is not a manifest this tool reads. It reads package.json, Cargo.toml, pyproject.toml, go.mod, and .yml or .yaml under .github/workflows/.`,
			);
		}
		return { kind, label: file.path, content: file.content };
	});
}

function compareVersions(raw: Record<string, unknown>): Promise<unknown> {
	const args = only(raw, ['files', 'maxResults'], 'this tool');
	const documents = readFiles(args);
	const maxResults = readMaxResults(args);
	const report = reportFor(documents);

	// The flag matters more than the cap: this tool's whole job is saying what disagrees.
	const truncated = report.findings.length > maxResults;
	const findings = report.findings.slice(0, maxResults);
	const diagnostics: Diagnostic[] = report.diagnostics.map((diagnostic) => ({
		severity: diagnostic.severity,
		code: diagnostic.code,
		message: `${diagnostic.file}: ${diagnostic.message}`,
	}));

	return Promise.resolve({
		ok: !diagnostics.some((diagnostic) => diagnostic.severity === 'error'),
		data: {
			schema: report.schema,
			manifests: report.manifests,
			findings,
			refusals: report.refusals,
			summary: report.summary,
		},
		diagnostics,
		meta: { tool: 'compare_versions', count: findings.length, truncated },
	});
}

export const TOOLS: readonly ToolDefinition[] = Object.freeze([
	Object.freeze({
		name: 'compare_versions',
		description: DESCRIPTION,
		inputSchema: {
			type: 'object',
			properties: {
				files: {
					type: 'array',
					minItems: 1,
					description: 'The manifests to compare.',
					items: {
						type: 'object',
						properties: {
							path: {
								type: 'string',
								description:
									'Path or filename, e.g. "crates/api/Cargo.toml". It decides which grammar the content is read with, and labels every finding. Recognised: package.json, Cargo.toml, pyproject.toml, go.mod, and any .yml or .yaml under .github/workflows/.',
							},
							content: { type: 'string', description: 'The file contents.' },
						},
						required: ['path', 'content'],
						additionalProperties: false,
					},
				},
				maxResults: {
					type: 'integer',
					minimum: 1,
					maximum: MAX_MAX_RESULTS,
					default: DEFAULT_MAX_RESULTS,
					description: `Cap on returned findings (default ${DEFAULT_MAX_RESULTS}). meta.truncated reports whether any were dropped.`,
				},
			},
			required: ['files'],
			additionalProperties: false,
		},
		handler: compareVersions,
	}),
]);
