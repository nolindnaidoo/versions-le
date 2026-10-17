/**
 * The `versions-le` npm binary: the Rust CLI's surface over the extension's
 * detection engine. `versions-le mcp` serves the crate's two tools over stdio.
 */
import { version } from '../mcp/info';
import { serve } from './mcp';
import { crateOutput } from './output';
import { run } from './run';

function main(args: readonly string[]): void {
	const output = crateOutput('versions-le');
	if (args[0] === 'mcp') {
		// The crate's server stops at the first reply it cannot write.
		process.stdout.once('error', () => output.exit(2, null));
		serve(process.stdin, output.stdout, version).then((code) =>
			output.exit(code, null),
		);
		return;
	}
	run(args, {
		stdout: output.stdout,
		stderr: output.stderr,
		stdoutIsTerminal: process.stdout.isTTY === true,
		version,
	}).then((code) => output.exit(code, 'could not write the report'));
}

main(process.argv.slice(2));
