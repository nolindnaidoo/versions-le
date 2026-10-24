import { describe, expect, it } from 'vitest';
import { reportFor } from '../detect/report';
import { formatReport } from './format';

describe('the report', () => {
	it('says when there is nothing to compare', () => {
		expect(formatReport(reportFor([]))).toContain('No manifests found.');
	});

	it('lists the files it could not read when none could, rather than saying none was found', () => {
		const analysed = reportFor([]);
		const text = formatReport({
			...analysed,
			diagnostics: [
				{
					severity: 'error',
					code: 'unreadable',
					file: 'go.mod',
					message: 'not UTF-8 text',
				},
			],
			summary: { ...analysed.summary, errors: 1 },
		});
		expect(text).toContain('## Could not be read (1)');
		expect(text).toContain('- `go.mod`: not UTF-8 text');
		expect(text).not.toContain('No manifests found.');
		expect(text).not.toContain('No version problems found.');
	});

	it('says when manifests agree, and lists them', () => {
		const text = formatReport(
			reportFor([
				{ kind: 'go-mod', label: 'go.mod', content: 'module x\n\ngo 1.22\n' },
			]),
		);
		expect(text).toContain('No version problems found.');
		expect(text).toContain('- `go.mod` · go · 1');
	});

	it('shows a refusal that spans ecosystems with no ecosystem of its own', () => {
		const text = formatReport(
			reportFor([
				{
					kind: 'package-json',
					label: 'package.json',
					content: '{"dependencies": {"semver": "^7"}}',
				},
				{
					kind: 'cargo-toml',
					label: 'Cargo.toml',
					content: '[dependencies]\nsemver = "1"\n',
				},
			]),
		);
		expect(text).toContain(
			'- **cross_ecosystem** · — · `semver`: appears in cargo and npm',
		);
	});

	it('keeps a backtick in a name from closing its code span', () => {
		const text = formatReport(
			reportFor([
				{
					kind: 'package-json',
					label: 'package.json',
					content: '{"dependencies": {"a`b": "*"}}',
				},
			]),
		);
		expect(text).toContain("`a'b`");
	});
});
