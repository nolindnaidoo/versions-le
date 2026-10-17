/**
 * The command line, parsed as the Rust CLI parses it: strictly, in order, and
 * refusing rather than defaulting.
 */
import { ECOSYSTEMS, type Ecosystem } from '../detect/heuristics';
import type { DiscoverOptions } from './discover';
import { FLAGS } from './usage';

export type FailOn = 'conflict' | 'any';

export interface Cli {
	readonly roots: readonly string[];
	readonly discover: DiscoverOptions;
	readonly failOn: FailOn;
	readonly strict: boolean;
}

export function parse(args: readonly string[]): Cli {
	const roots: string[] = [];
	const ecosystems: Ecosystem[] = [];
	const exclude: string[] = [];
	let hidden = false;
	let respectIgnore = true;
	let failOn: FailOn = 'conflict';
	let strict = false;

	for (let index = 0; index < args.length; index++) {
		const arg = args[index] as string;
		if (arg.startsWith('-') && !FLAGS.includes(arg)) {
			throw new Error(`${arg} is not an option. Try --help.`);
		}
		const value = () => {
			index += 1;
			return args[index];
		};
		if (arg === '--strict') strict = true;
		if (arg === '--hidden') hidden = true;
		if (arg === '--no-ignore') respectIgnore = false;
		if (arg === '--ecosystem') ecosystems.push(ecosystemOf(value()));
		if (arg === '--fail-on') failOn = failOnOf(value());
		if (arg === '--exclude') {
			const pattern = value();
			if (pattern === undefined) throw new Error('--exclude needs a pattern');
			exclude.push(pattern);
		}
		if (!arg.startsWith('-')) roots.push(arg);
	}

	return Object.freeze({
		roots: roots.length > 0 ? roots : ['.'],
		discover: { hidden, respectIgnore, exclude, ecosystems },
		failOn,
		strict,
	});
}

function ecosystemOf(value: string | undefined): Ecosystem {
	if (value === undefined) throw new Error('--ecosystem needs a name');
	const found = ECOSYSTEMS.find((name) => name === value);
	if (found === undefined) {
		throw new Error(
			`${value} is not an ecosystem. Try npm, cargo, python, go or ci.`,
		);
	}
	return found;
}

function failOnOf(value: string | undefined): FailOn {
	if (value === undefined) throw new Error('--fail-on needs conflict or any');
	if (value === 'conflict' || value === 'any') return value;
	throw new Error(`${value} is not conflict or any`);
}
