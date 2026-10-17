/**
 * The terminal surface, as the Rust CLI's `cli.rs` runs it.
 *
 * stdout is protocol: one JSON report whenever something is reading it, and
 * nothing when it is a terminal.
 */
import { parse } from './args';
import { exitCode, scan, summary } from './scan';
import { USAGE } from './usage';

export interface RunIo {
	readonly stdout: (text: string) => void;
	readonly stderr: (text: string) => void;
	readonly stdoutIsTerminal: boolean;
	readonly version: string;
}

export async function run(args: readonly string[], io: RunIo): Promise<number> {
	const first = args[0];
	if (first === '--help' || first === '-h') {
		io.stdout(`${USAGE}\n`);
		return 0;
	}
	if (first === '--version' || first === '-V') {
		io.stdout(`versions-le ${io.version}\n`);
		return 0;
	}
	try {
		const cli = parse(args);
		const report = scan(cli.roots, cli.discover);
		if (!io.stdoutIsTerminal) io.stdout(`${JSON.stringify(report)}\n`);
		io.stderr(summary(report));
		return exitCode(report, cli.failOn, cli.strict);
	} catch (error) {
		io.stderr(
			`versions-le: ${error instanceof Error ? error.message : String(error)}\n`,
		);
		return 2;
	}
}
