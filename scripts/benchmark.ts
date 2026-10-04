/**
 * Measure real throughput. Run with `bun run benchmark`.
 *
 * Numbers are machine-specific, so the host is recorded alongside them and
 * they are never asserted in CI. Inputs are generated rather than checked in so
 * the sizes are explicit.
 */
import { cpus, totalmem } from 'node:os';
import { type Document, reportFor } from '../src/detect/report';

/** A monorepo of `count` crates and packages, every fourth one drifting. */
function monorepo(count: number): Document[] {
	const documents: Document[] = [];
	for (let i = 0; i < count; i++) {
		const drift = i % 4 === 0;
		documents.push({
			kind: 'cargo-toml',
			label: `crates/c${i}/Cargo.toml`,
			content: `[package]\nname = "c${i}"\nrust-version = "1.88"\n[dependencies]\nserde = "${drift ? '0.9' : '1.0.200'}"\nregex = "1"\ntokio = { version = "1.${i % 40}", features = ["full"] }\n[dev-dependencies]\ntempfile = "3"\n`,
		});
		documents.push({
			kind: 'package-json',
			label: `packages/p${i}/package.json`,
			content: JSON.stringify({ name: `p${i}`, dependencies: { react: drift ? '^17.0.0' : '^18.2.0', lodash: '^4.17.21', [`dep-${i % 50}`]: '~1.2.3' }, devDependencies: { typescript: '^5.4.0' } }),
		});
	}
	return documents;
}

const CASES: ReadonlyArray<{ label: string; build: () => Document[] }> = [
	{ label: '50 crates, 50 packages', build: () => monorepo(50) },
	{ label: '500 crates, 500 packages', build: () => monorepo(500) },
	{ label: '2,000 crates, 2,000 packages', build: () => monorepo(2000) },
];

const WARMUP = 2;
const RUNS = 7;
const median = (xs: readonly number[]) => {
	const s = [...xs].sort((a, b) => a - b);
	const mid = Math.floor(s.length / 2);
	return s.length % 2 ? (s[mid] as number) : ((s[mid - 1] as number) + (s[mid] as number)) / 2;
};

const results: Array<Record<string, unknown>> = [];
for (const c of CASES) {
	const documents = c.build();
	const content = documents.map((d) => d.content).join('\n');
	const bytes = Buffer.byteLength(content, 'utf8');
	const run = () => reportFor(documents).summary.entries;
	for (let i = 0; i < WARMUP; i++) run();
	const durations: number[] = [];
	let count = 0;
	for (let i = 0; i < RUNS; i++) {
		const t0 = performance.now();
		count = run();
		durations.push(performance.now() - t0);
	}
	const ms = median(durations);
	results.push({
		label: c.label,
		bytes,
		lines: content.split('\n').length,
		extracted: count,
		ms: Number(ms.toFixed(2)),
		perSecond: count > 0 ? Math.round(count / (ms / 1000)) : null,
		mbPerSecond: Number((bytes / 1_048_576 / (ms / 1000)).toFixed(1)),
	});
	console.log(`${c.label.padEnd(18)} ${(bytes / 1_048_576).toFixed(2)} MB  ${String(count).padStart(7)}  ${ms.toFixed(2)} ms`);
}
const cpu = cpus()[0]?.model ?? 'unknown CPU';
await Bun.write(
	'benchmark-results.json',
	`${JSON.stringify({ host: `${cpu}, ${Math.round(totalmem() / 1_073_741_824)} GB RAM, Node ${process.versions.node}`, runs: RUNS, results }, null, 2)}\n`,
);
console.log('\nwrote benchmark-results.json');
