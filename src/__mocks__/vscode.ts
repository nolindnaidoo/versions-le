/**
 * Mock VS Code API for unit tests (aliased via vitest.config.ts).
 * Stateful pieces (config store, message log, command registry, opened
 * documents) expose `_reset()`/`_set()` helpers prefixed with
 * underscore — test-only API.
 */

export interface WorkspaceFolder {
	readonly uri: Uri;
	readonly name: string;
	readonly index: number;
}

// ---------------------------------------------------------------- Uri

export class Uri {
	scheme: string;
	authority: string;
	path: string;
	query: string;
	fragment: string;

	constructor(
		scheme: string,
		authority: string,
		path: string,
		query: string,
		fragment: string,
	) {
		this.scheme = scheme;
		this.authority = authority;
		this.path = path;
		this.query = query;
		this.fragment = fragment;
	}

	get fsPath(): string {
		return this.path;
	}

	toString(_skipEncoding?: boolean): string {
		return `${this.scheme}://${this.authority}${this.path}`;
	}

	static file(path: string): Uri {
		return new Uri('file', '', path, '', '');
	}

	with(change: { path?: string }): Uri {
		return new Uri(this.scheme, this.authority, change.path ?? this.path, this.query, this.fragment);
	}

	/** POSIX joining with `..` resolved, which is all the audit command asks of it. */
	static joinPath(base: Uri, ...segments: string[]): Uri {
		const parts = base.path.split('/').filter(Boolean);
		for (const segment of segments.flatMap((s) => s.split('/'))) {
			if (segment === '..') parts.pop();
			else if (segment && segment !== '.') parts.push(segment);
		}
		return base.with({ path: `/${parts.join('/')}` });
	}

	static parse(value: string): Uri {
		const match = value.match(/^(\w+):\/\/([^/]*)(.*)$/);
		if (match?.[1] && match[2] !== undefined && match[3] !== undefined) {
			return new Uri(match[1], match[2], match[3], '', '');
		}
		return new Uri('file', '', value, '', '');
	}
}

// ---------------------------------------------- positions and ranges

export class Position {
	constructor(
		public readonly line: number,
		public readonly character: number,
	) {}
}

export class Range {
	constructor(
		public readonly start: Position,
		public readonly end: Position,
	) {}
}

export class Selection extends Range {}

// ---------------------------------------------------------- documents

export interface MockDocumentInit {
	readonly content: string;
	readonly languageId?: string;
	readonly fileName?: string;
}

export function _createDocument(init: MockDocumentInit) {
	let content = init.content;
	const doc = {
		get lineCount() {
			return content.split('\n').length;
		},
		getText: () => content,
		_setText: (next: string) => {
			content = next;
		},
		languageId: init.languageId ?? 'plaintext',
		fileName: init.fileName ?? '/mock/document.txt',
		uri: Uri.file(init.fileName ?? '/mock/document.txt'),
		positionAt: (offset: number) => {
			const lines = content.split('\n');
			let remaining = Math.max(0, Math.min(offset, content.length));
			for (let line = 0; line < lines.length; line++) {
				const length = (lines[line] ?? '').length;
				if (remaining <= length) return new Position(line, remaining);
				remaining -= length + 1;
			}
			return new Position(
				lines.length - 1,
				(lines[lines.length - 1] ?? '').length,
			);
		},
		lineAt: (line: number) => {
			const lines = content.split('\n');
			return {
				text: lines[line] ?? '',
				range: new Range(
					new Position(line, 0),
					new Position(line, (lines[line] ?? '').length),
				),
			};
		},
	};
	return doc;
}

export type MockDocument = ReturnType<typeof _createDocument>;

// ------------------------------------------------------------ editors

export interface RecordedEdit {
	readonly kind: 'replace' | 'insert';
	readonly text: string;
}

export function _createEditor(document: MockDocument) {
	const edits: RecordedEdit[] = [];
	return {
		document,
		_edits: edits,
		edit: async (
			callback: (builder: {
				replace(range: Range, text: string): void;
				insert(position: Position, text: string): void;
			}) => void,
		) => {
			callback({
				replace: (_range: Range, text: string) => {
					edits.push({ kind: 'replace', text });
					document._setText(text);
				},
				insert: (_position: Position, text: string) => {
					edits.push({ kind: 'insert', text });
					document._setText(document.getText() + text);
				},
			});
			return true;
		},
	};
}

export type MockEditor = ReturnType<typeof _createEditor>;

// ------------------------------------------------------ configuration

const configStore = new Map<string, unknown>();
const configUpdates: Array<{ key: string; value: unknown; target: unknown }> =
	[];

export function _setConfig(key: string, value: unknown): void {
	configStore.set(key, value);
}

export function _getConfigUpdates(): ReadonlyArray<{
	key: string;
	value: unknown;
	target: unknown;
}> {
	return configUpdates;
}

export const ConfigurationTarget = {
	Global: 1,
	Workspace: 2,
	WorkspaceFolder: 3,
};

type ConfigListener = (event: {
	affectsConfiguration: (section: string) => boolean;
}) => void;
const configListeners: ConfigListener[] = [];

export function _fireConfigChange(section: string): void {
	for (const listener of configListeners) {
		listener({
			affectsConfiguration: (candidate: string) =>
				section === candidate || section.startsWith(`${candidate}.`),
		});
	}
}

// --------------------------------------------------------- workspace

const openedDocuments: MockDocument[] = [];

export function _openedDocuments(): readonly MockDocument[] {
	return openedDocuments;
}

/**
 * An in-memory filesystem: absolute path to the file's bytes. A directory is
 * any path some file sits under, and a path nothing sits at is not there.
 */
const workspaceFiles = new Map<string, Uint8Array>();

export function _setWorkspaceFiles(files: Readonly<Record<string, string | Uint8Array>>): void {
	workspaceFiles.clear();
	for (const [path, content] of Object.entries(files)) {
		workspaceFiles.set(path, typeof content === 'string' ? new TextEncoder().encode(content) : content);
	}
}

function isDirectory(path: string): boolean {
	const prefix = path === '/' ? '/' : `${path}/`;
	return [...workspaceFiles.keys()].some((file) => file.startsWith(prefix));
}

/** A glob as VS Code reads one: `**` crosses separators, `*` does not, `{a,b}` is a choice. */
function globToRegExp(glob: string): RegExp {
	let out = '';
	for (let i = 0; i < glob.length; i++) {
		const c = glob.charAt(i);
		if (c === '*' && glob.charAt(i + 1) === '*') {
			out += glob.charAt(i + 2) === '/' ? '(?:.*/)?' : '.*';
			i += glob.charAt(i + 2) === '/' ? 2 : 1;
		} else if (c === '*') out += '[^/]*';
		else if (c === '?') out += '[^/]';
		else if (c === '{') out += '(?:';
		else if (c === '}') out += ')';
		else if (c === ',') out += '|';
		else out += c.replace(/[.+^$()|[\]\\]/g, '\\$&');
	}
	return new RegExp(`^${out}$`);
}

export class RelativePattern {
	readonly baseUri: Uri;
	constructor(
		base: Uri | { uri: Uri },
		readonly pattern: string,
	) {
		this.baseUri = base instanceof Uri ? base : base.uri;
	}
}

export const workspace = {
	workspaceFolders: undefined as WorkspaceFolder[] | undefined,
	/** Files in the in-memory filesystem under the pattern's base, relative paths matched against the globs. */
	findFiles: async (include: RelativePattern, exclude?: string) => {
		const base = include.baseUri.path.replace(/\/$/, '');
		const includes = globToRegExp(include.pattern);
		const excludes = exclude ? globToRegExp(exclude) : undefined;
		return [...workspaceFiles.keys()]
			.filter((path) => path.startsWith(`${base}/`))
			.filter((path) => {
				const relative = path.slice(base.length + 1);
				return includes.test(relative) && !excludes?.test(relative);
			})
			.map((path) => Uri.file(path));
	},
	getWorkspaceFolder: (_uri: Uri) => undefined as WorkspaceFolder | undefined,
	asRelativePath: (target: Uri | string, _includeFolder?: boolean) => (typeof target === 'string' ? target : target.path),
	fs: {
		readFile: async (uri: Uri) => {
			const bytes = workspaceFiles.get(uri.path);
			if (bytes === undefined) throw new Error(`no such file: ${uri.path}`);
			return bytes;
		},
		writeFile: async (_uri: Uri, _content: Uint8Array) => {},
		stat: async (uri: Uri) => {
			const bytes = workspaceFiles.get(uri.path);
			if (bytes !== undefined) return { type: FileType.File, ctime: 0, mtime: 0, size: bytes.length };
			if (isDirectory(uri.path)) return { type: FileType.Directory, ctime: 0, mtime: 0, size: 0 };
			throw new Error(`no such file or directory: ${uri.path}`);
		},
		readDirectory: async (uri: Uri) => {
			const prefix = uri.path === '/' ? '/' : `${uri.path}/`;
			const entries = new Map<string, number>();
			for (const file of workspaceFiles.keys()) {
				if (!file.startsWith(prefix)) continue;
				const [name, ...rest] = file.slice(prefix.length).split('/');
				entries.set(name as string, rest.length === 0 ? FileType.File : FileType.Directory);
			}
			return [...entries];
		},
	},
	getConfiguration: (section?: string) => ({
		get: <T>(key: string, defaultValue?: T): T | undefined => {
			const full = section ? `${section}.${key}` : key;
			return configStore.has(full)
				? (configStore.get(full) as T)
				: defaultValue;
		},
		update: async (key: string, value: unknown, target?: unknown) => {
			const full = section ? `${section}.${key}` : key;
			configStore.set(full, value);
			configUpdates.push({ key: full, value, target });
		},
	}),
	onDidChangeConfiguration: (listener: ConfigListener) => {
		configListeners.push(listener);
		return {
			dispose: () => {
				const index = configListeners.indexOf(listener);
				if (index >= 0) configListeners.splice(index, 1);
			},
		};
	},
	openTextDocument: async (options?: {
		content?: string;
		language?: string;
	}) => {
		const doc = _createDocument({
			content: options?.content ?? '',
			languageId: options?.language ?? 'plaintext',
		});
		openedDocuments.push(doc);
		return doc;
	},
};

// ------------------------------------------------------------ window

export interface ShownMessage {
	readonly kind: 'info' | 'warning' | 'error';
	readonly message: string;
	readonly items: readonly unknown[];
}

const shownMessages: ShownMessage[] = [];
const shownEditors: MockEditor[] = [];
const shownDocumentOptions: Array<{ viewColumn?: number }> = [];
let activeTextEditor: MockEditor | undefined;
let quickPickResponder: ((items: unknown[]) => unknown) | undefined;
let inputBoxResponder: (() => string | undefined) | undefined;
let warningResponder: ((items: unknown[]) => unknown) | undefined;

export function _shownMessages(): readonly ShownMessage[] {
	return shownMessages;
}

export function _shownDocumentOptions(): readonly { viewColumn?: number }[] {
	return shownDocumentOptions;
}

export function _shownEditors(): readonly MockEditor[] {
	return shownEditors;
}

export function _setActiveEditor(document: MockDocument | undefined): void {
	activeTextEditor = document ? _createEditor(document) : undefined;
}

export function _activeEditor(): MockEditor | undefined {
	return activeTextEditor;
}

export function _respondToQuickPick(
	responder: ((items: unknown[]) => unknown) | undefined,
): void {
	quickPickResponder = responder;
}

/** Values an input-box validator refused, in order. */
const inputBoxRejections: Array<{ value: string; message: string }> = [];

export function _inputBoxRejections(): readonly {
	value: string;
	message: string;
}[] {
	return inputBoxRejections;
}

export function _respondToInputBox(
	responder: (() => string | undefined) | undefined,
): void {
	inputBoxResponder = responder;
}

export function _respondToWarning(
	responder: ((items: unknown[]) => unknown) | undefined,
): void {
	warningResponder = responder;
}

export const StatusBarAlignment = { Left: 1, Right: 2 };
export const ViewColumn = { Active: -1, Beside: -2, One: 1, Two: 2 };
export const ProgressLocation = { Notification: 15, Window: 10 };

const cancellationToken = {
	isCancellationRequested: false,
	onCancellationRequested: (_listener: () => void) => ({ dispose: () => {} }),
};

export const window = {
	get activeTextEditor() {
		return activeTextEditor;
	},
	showInformationMessage: async (message: string, ...items: unknown[]) => {
		shownMessages.push({ kind: 'info', message, items });
		return undefined;
	},
	showWarningMessage: async (message: string, ...items: unknown[]) => {
		shownMessages.push({ kind: 'warning', message, items });
		return warningResponder?.(items);
	},
	showErrorMessage: async (message: string, ...items: unknown[]) => {
		shownMessages.push({ kind: 'error', message, items });
		return undefined;
	},
	showQuickPick: async (items: unknown[], _options?: unknown) =>
		quickPickResponder ? quickPickResponder(items) : undefined,
	showInputBox: async (options?: unknown) => {
		const value = inputBoxResponder ? inputBoxResponder() : undefined;

		// VS Code runs validateInput against what the user types and refuses to
		// accept a value the validator rejects — the box stays open until the
		// input is valid or the user escapes. Ignoring validateInput leaves the
		// validators uncovered AND lets a test hand a command a value the real UI
		// would never deliver.
		const validate = (options as { validateInput?: (v: string) => unknown })
			?.validateInput;
		if (typeof validate === 'function' && typeof value === 'string') {
			const message = validate(value);
			if (message !== undefined && message !== null && message !== '') {
				inputBoxRejections.push({ value, message: String(message) });
				return undefined;
			}
		}
		return value;
	},
	showTextDocument: async (document: unknown, options?: unknown) => {
		const editor = _createEditor(document as MockDocument);
		shownEditors.push(editor);
		// Recorded so tests can assert placement (openResultsSideBySide sets
		// viewColumn); the editor object itself carries no such field.
		shownDocumentOptions.push(
			(options as { viewColumn?: number } | undefined) ?? {},
		);
		return editor;
	},
	withProgress: async <T>(
		_options: unknown,
		task: (
			progress: { report: (value: unknown) => void },
			token: typeof cancellationToken,
		) => Promise<T>,
	): Promise<T> => task({ report: () => {} }, cancellationToken),
	createOutputChannel: (_name: string) => {
		const linesOut: string[] = [];
		outputChannels.push(linesOut);
		return {
			appendLine: (line: string) => linesOut.push(line),
			dispose: () => {},
			_lines: linesOut,
		};
	},
	createStatusBarItem: (_alignment?: unknown, _priority?: number) => ({
		text: '',
		tooltip: '',
		command: undefined as unknown,
		visible: false,
		show(): void {
			(this as { visible: boolean }).visible = true;
		},
		hide(): void {
			(this as { visible: boolean }).visible = false;
		},
		dispose: () => {},
	}),
};

const outputChannels: string[][] = [];

export function _outputChannelLines(): readonly string[] {
	return outputChannels.flat();
}

// ---------------------------------------------------------- commands

const registeredCommands = new Map<string, (...args: unknown[]) => unknown>();

export function _registeredCommands(): ReadonlyMap<
	string,
	(...args: unknown[]) => unknown
> {
	return registeredCommands;
}

export const commands = {
	registerCommand: (id: string, handler: (...args: unknown[]) => unknown) => {
		registeredCommands.set(id, handler);
		return {
			dispose: () => {
				registeredCommands.delete(id);
			},
		};
	},
	executeCommand: async (id: string, ...args: unknown[]) => {
		const handler = registeredCommands.get(id);
		if (handler) return handler(...args);
		executedBuiltins.push({ id, args });
		return undefined;
	},
};

export const executedBuiltins: Array<{ id: string; args: unknown[] }> = [];

// --------------------------------------------------------------- env

const clipboard = { value: '' };

export const env = {
	clipboard: {
		writeText: async (text: string) => {
			clipboard.value = text;
		},
		readText: async () => clipboard.value,
	},
	openExternal: async (_uri: Uri) => true,
};

export function _clipboardText(): string {
	return clipboard.value;
}

// ------------------------------------------------- extension context

export function _createExtensionContext() {
	const globalStateStore = new Map<string, unknown>();
	return {
		subscriptions: [] as Array<{ dispose(): void }>,
		globalState: {
			get: <T>(key: string, defaultValue?: T): T | undefined =>
				globalStateStore.has(key)
					? (globalStateStore.get(key) as T)
					: defaultValue,
			update: async (key: string, value: unknown) => {
				globalStateStore.set(key, value);
			},
		},
	};
}

export type MockExtensionContext = ReturnType<typeof _createExtensionContext>;

// -------------------------------------------------------------- misc

export const FileType = {
	Unknown: 0,
	File: 1,
	Directory: 2,
	SymbolicLink: 64,
};

/** Reset all mutable mock state between tests. */
export function _resetMockState(): void {
	inputBoxRejections.length = 0;
	configStore.clear();
	configUpdates.length = 0;
	configListeners.length = 0;
	shownMessages.length = 0;
	shownEditors.length = 0;
	shownDocumentOptions.length = 0;
	openedDocuments.length = 0;
	outputChannels.length = 0;
	executedBuiltins.length = 0;
	registeredCommands.clear();
	activeTextEditor = undefined;
	quickPickResponder = undefined;
	inputBoxResponder = undefined;
	warningResponder = undefined;
	clipboard.value = '';
	workspaceFiles.clear();
	workspace.workspaceFolders = undefined;
}

export const l10n = {
	t(message: string, ...args: unknown[]): string {
		if (args.length === 1 && typeof args[0] === 'object' && args[0] !== null) {
			const named = args[0] as Record<string, unknown>;
			return message.replace(/\{(\w+)\}/g, (whole, key) =>
				key in named ? String(named[key]) : whole,
			);
		}
		return message.replace(/\{(\d+)\}/g, (whole, index) => {
			const value = args[Number(index)];
			return value === undefined ? whole : String(value);
		});
	},
};
