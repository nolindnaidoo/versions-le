#!/usr/bin/env node
/**
 * Drive the built MCP server through many tool calls over one stdio session
 * and sample its resident memory, so a leak per call shows as a line that
 * keeps climbing rather than one that levels off.
 *
 *   node scripts/check-mcp-memory.js [calls] [sampleEvery]
 *
 * One call is in flight at a time, as a client holds a session: writing
 * every request up front measures the pipe, not the server, because replies
 * the reader has not drained sit in the server's write queue and read as
 * resident memory (3,000 queued calls of this size held it near a gigabyte).
 *
 * Hand-run, like `benchmark`: a memory figure from a shared CI runner is
 * noise. It exits 1 when the last sample is more than a tenth above the
 * sample a third of the way in, which is past where the heap has settled on
 * every run so far. Needs `dist/mcp-server.js` (`bun run build:mcp:prod`) and
 * a POSIX `ps`.
 */

'use strict';

const { spawn, execFileSync } = require('node:child_process');
const { existsSync } = require('node:fs');
const path = require('node:path');

const SERVER = path.join(__dirname, '..', 'dist', 'mcp-server.js');
const calls = Number(process.argv[2] ?? 3000);
const sampleEvery = Number(process.argv[3] ?? 500);

if (!existsSync(SERVER)) {
	console.error(`${SERVER} is missing — run \`bun run build:mcp:prod\` first.`);
	process.exit(2);
}
if (!Number.isInteger(calls) || calls < sampleEvery * 3) {
	console.error('calls must be an integer at least three times sampleEvery.');
	process.exit(2);
}

// Two manifests with two hundred dependencies each, half of them drifted, so
// each call parses, compares and reports real work.
const dependencies = (offset) =>
	Object.fromEntries(
		Array.from({ length: 200 }, (_, i) => [`package-${i}`, `^${i % 2 === 0 ? 1 : 1 + offset}.${i}.0`]),
	);
const files = [
	{ path: 'api/package.json', content: JSON.stringify({ dependencies: dependencies(0) }) },
	{ path: 'web/package.json', content: JSON.stringify({ dependencies: dependencies(1) }) },
];

const server = spawn(process.execPath, [SERVER], {
	stdio: ['pipe', 'pipe', 'inherit'],
});

function residentKb() {
	return Number(
		execFileSync('ps', ['-o', 'rss=', '-p', String(server.pid)])
			.toString()
			.trim(),
	);
}

const samples = [];
let answered = 0;
let sent = 0;
let pending = '';

function sendNext() {
	sent += 1;
	server.stdin.write(
		`${JSON.stringify({
			jsonrpc: '2.0',
			id: sent,
			method: 'tools/call',
			params: { name: 'compare_versions', arguments: { files } },
		})}\n`,
	);
}

server.stdout.on('data', (chunk) => {
	pending += chunk;
	let newline = pending.indexOf('\n');
	while (newline !== -1) {
		pending = pending.slice(newline + 1);
		newline = pending.indexOf('\n');
		answered += 1;
		if (answered % sampleEvery === 0) samples.push([answered, residentKb()]);
		if (answered === calls) finish();
		else if (sent < calls) sendNext();
	}
});

sendNext();

const deadline = setTimeout(() => {
	console.error(`only ${answered} of ${calls} calls were answered in 120 s`);
	server.kill();
	process.exit(1);
}, 120_000);

function finish() {
	clearTimeout(deadline);
	server.stdin.end();
	for (const [call, kb] of samples) {
		console.log(`after ${String(call).padStart(6)} calls: ${(kb / 1024).toFixed(0).padStart(5)} MB`);
	}
	const settled = samples[Math.floor(samples.length / 3)];
	const last = samples[samples.length - 1];
	if (!settled || !last) {
		console.error('not enough samples to judge');
		process.exit(1);
	}
	const growth = last[1] / settled[1];
	if (growth > 1.1) {
		console.error(
			`FAIL: resident memory grew ${((growth - 1) * 100).toFixed(0)}% between call ${settled[0]} and call ${last[0]}`,
		);
		process.exit(1);
	}
	console.log(
		`OK: resident memory is flat from call ${settled[0]} to call ${last[0]} (${((growth - 1) * 100).toFixed(1)}% change)`,
	);
}
