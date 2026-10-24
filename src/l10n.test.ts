import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * The runtime strings and the bundle, held equal.
 *
 * `l10n.t()` looks a string up by its exact text, and a string that is not in
 * the bundle is shown in English in every locale with nothing thrown. The
 * reverse is dead weight, or the old wording of a string that was edited and
 * so went untranslated. The integration suite holds the twelve catalogues to
 * each other; nothing held them to the code that calls them.
 *
 * This file sits beside `src/`'s other suites rather than in `src/i18n/`,
 * because `copy:i18n` copies that whole folder to the package root at
 * prepublish and `clean:i18n` takes back only the catalogues.
 */

const root = join(__dirname, '..');

/** Every first argument to `l10n.t(`, with the files it appears in. */
function stringsPassedToL10n(): Map<string, string[]> {
	const found = new Map<string, string[]>();
	for (const file of sourceFiles(join(root, 'src'))) {
		const text = readFileSync(file, 'utf8');
		for (const match of text.matchAll(LITERAL_CALL)) {
			const key = (match[2] ?? '').replace(/\\(["'])/g, '$1');
			found.set(key, [...(found.get(key) ?? []), file]);
		}
	}
	return found;
}

/** `l10n.t(` followed by a string literal, across line breaks. */
const LITERAL_CALL = /l10n\.t\(\s*(["'])((?:(?!\1)[^\\]|\\.)*)\1/g;

/** `l10n.t(` followed by anything that is not a string literal. */
const OTHER_CALL = /l10n\.t\(\s*[^"'\s)]/;

function sourceFiles(dir: string): string[] {
	return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) {
			return entry.name === '__mocks__' ? [] : sourceFiles(path);
		}
		return path.endsWith('.ts') && !path.endsWith('.test.ts') ? [path] : [];
	});
}

function bundleKeys(file: string): string[] {
	return Object.keys(
		JSON.parse(readFileSync(join(root, 'l10n', file), 'utf8')) as Record<
			string,
			string
		>,
	).sort();
}

describe('runtime strings and the bundle agree', () => {
	const inSource = stringsPassedToL10n();
	const inBundle = bundleKeys('bundle.l10n.json');

	it('reads a real number of calls, so the comparison is not vacuous', () => {
		expect(inSource.size).toBeGreaterThan(20);
	});

	it('every string passed to l10n.t() is in the bundle', () => {
		const missing = [...inSource.keys()].filter(
			(key) => !inBundle.includes(key),
		);
		expect(missing).toEqual([]);
	});

	it('every key in the bundle is passed to l10n.t() somewhere', () => {
		const dead = inBundle.filter((key) => !inSource.has(key));
		expect(dead).toEqual([]);
	});

	it('every call hands l10n.t() a string literal, so the key is the text', () => {
		// A template literal or a variable is looked up as its evaluated value,
		// which is never in the bundle. Values go in as `{0}` arguments.
		const offenders = sourceFiles(join(root, 'src')).filter((file) =>
			OTHER_CALL.test(readFileSync(file, 'utf8')),
		);
		expect(offenders).toEqual([]);
	});

	it('never calls l10n.t() at module scope', () => {
		// At module scope the call runs when the module is required, before the
		// host has loaded the bundle, and the string stays English for the life
		// of the process. Every call site is inside a function body, which this
		// reads as the call being indented; a call at column zero is the defect.
		const offenders = sourceFiles(join(root, 'src')).flatMap((file) =>
			readFileSync(file, 'utf8')
				.split('\n')
				.flatMap((line, index) =>
					/^(?:export\s+)?(?:const|let|var)\b.*\bl10n\.t\(/.test(line)
						? [`${file}:${index + 1}`]
						: [],
				),
		);
		expect(offenders).toEqual([]);
	});

	it('hands no literal to the notifier, the status bar, a prompt or a dialog', () => {
		// A string the user sees goes through `l10n.t()`. A literal or a
		// template handed straight to a notification, a flash, a quick pick,
		// an input box or a modal button is English in every locale. The value
		// a call passes on, such as a parse error, is a variable and is not
		// what this reads.
		const DIRECT =
			/(?:\.(?:info|warn|error|flash)|show(?:Information|Warning|Error)Message|showQuickPick|showInputBox)\(\s*(['"`])/g;
		const BUTTON = /\{\s*modal:\s*true\s*\}\s*,\s*(['"`])/g;
		// src/detect/ builds diagnostics in the crate's words, which are data the
		// report quotes, identical to the CLI's; src/cli/ is the crate's surface,
		// held to it byte for byte. Neither is text this extension says.
		const offenders = sourceFiles(join(root, 'src'))
			.filter(
				(file) =>
					!file.includes(join('src', 'detect')) &&
					!file.includes(join('src', 'cli')),
			)
			.flatMap((file) => {
				const text = readFileSync(file, 'utf8');
				return [DIRECT, BUTTON].flatMap((pattern) =>
					[...text.matchAll(pattern)].map(
						(match) =>
							`${file}:${text.slice(0, match.index).split('\n').length}`,
					),
				);
			});
		expect(offenders).toEqual([]);
	});

	it('every locale carries exactly the English keys', () => {
		const locales = readdirSync(join(root, 'l10n')).filter(
			(file) => file !== 'bundle.l10n.json',
		);
		expect(locales).toHaveLength(12);
		for (const file of locales) {
			expect(bundleKeys(file), file).toEqual(inBundle);
		}
	});
});
