#!/usr/bin/env node
/**
 * npm command-line package gate.
 *
 * Packs `cli/`, installs the tarball into a clean throwaway project, and runs
 * the *installed* `versions-le` over a real file. That is what `npx versions-le`
 * does, minus the registry, so it catches what a source-level test cannot: a
 * missing `files` entry, a bin path that does not resolve, a lost shebang, or
 * a bundle that still reaches for a dev dependency.
 *
 * Runs before publishing, because a broken version can never be reused. The
 * throwaway project is left in the OS temp directory rather than deleted.
 *
 *   node scripts/build-cli.js --prod && node scripts/check-cli-package.js
 */
const cp = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const root = JSON.parse(fs.readFileSync('package.json', 'utf8'));
const manifest = JSON.parse(fs.readFileSync('cli/package.json', 'utf8'));
const workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-check-'));

function fail(message, detail) {
	console.error(`\n  ✗ ${message}`);
	if (detail) console.error(`    ${detail}`);
	process.exit(1);
}

const npm = (args, cwd) => cp.execFileSync('npm', args, { cwd, encoding: 'utf8', stdio: 'pipe' });

if (manifest.version !== root.version) {
	fail(`cli/package.json is ${manifest.version}, package.json is ${root.version}`);
}

// A README telling people to `npx` a name this repo does not publish would
// fetch nothing, or someone else's package, so every `npx` must name one.
const published = [manifest.name, JSON.parse(fs.readFileSync('mcp/package.json', 'utf8')).name];
for (const readme of ['README.md', 'cli/README.md', 'crate/README.md', 'mcp/README.md']) {
	for (const [, named] of fs.readFileSync(readme, 'utf8').matchAll(/npx (?:-\S+ )*([\w@/.-]+)/g)) {
		if (!published.includes(named)) {
			fail(`${readme} says \`npx ${named}\``, `the published packages are ${published.join(' and ')}`);
		}
	}
}

const packed = JSON.parse(
	npm(['pack', path.resolve('cli'), '--json', '--pack-destination', workspace]),
)[0];
const shipped = packed.files.map((file) => file.path).sort();
const expected = ['LICENSE', 'README.md', 'cli.js', 'package.json'];
if (JSON.stringify(shipped) !== JSON.stringify(expected)) {
	fail('the tarball does not carry exactly the expected files', `got: ${shipped.join(', ')}`);
}

const project = path.join(workspace, 'project');
fs.mkdirSync(project);
fs.writeFileSync(
	path.join(project, 'package.json'),
	JSON.stringify({ name: 'cli-check', version: '1.0.0', private: true }),
);
npm(['install', path.join(workspace, packed.filename), '--no-audit', '--no-fund'], project);

const bin = path.join(project, 'node_modules', '.bin', 'versions-le');
if (!fs.existsSync(bin)) fail('npm did not install a `versions-le` binary', 'check the bin field');

const run = (args) => cp.spawnSync(bin, args, { cwd: project, encoding: 'utf8' });

const version = run(['--version']);
if (version.stdout.trim() !== `versions-le ${root.version}`) {
	fail('--version does not report the release', `got: ${JSON.stringify(version.stdout)}`);
}

for (const [dir, react] of [['a', '^18.2.0'], ['b', '^17.0.0']]) {
	fs.mkdirSync(path.join(project, dir));
	fs.writeFileSync(path.join(project, dir, 'package.json'), JSON.stringify({ dependencies: { react } }));
}
const scan = run(['a', 'b']);
let report;
try {
	report = JSON.parse(scan.stdout);
} catch {
	fail('the installed binary did not write a JSON report', scan.stderr || scan.stdout);
}
const found = report.findings.map((f) => `${f.code}:${f.name}`).join(' ');
if (scan.status !== 1 || found !== 'disjoint-constraint:react') {
	fail('the installed binary answered wrongly', `exit ${scan.status}, ${found}`);
}

const mcp = cp.spawnSync(bin, ['mcp'], {
	cwd: project,
	encoding: 'utf8',
	input: `${JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/list' })}\n`,
});
const tools = JSON.parse(mcp.stdout.split('\n')[0] || '{}').result?.tools ?? [];
const names = tools.map((tool) => tool.name).join(', ');
if (names !== 'compare_versions, versions_le_check') {
	fail('`versions-le mcp` does not serve both tools', `got: ${names}`);
}

console.log(`  ✓ ${manifest.name}@${manifest.version}: packed, installed, scanned, served`);
