import * as assert from 'node:assert';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import * as vscode from 'vscode';

const EXTENSION_ID = 'nolindnaidoo.versions-le';

/** A tree on disk, so the command reads real files the way the Explorer hands them over. */
function tree(files: Record<string, string>): vscode.Uri {
	const root = mkdtempSync(join(tmpdir(), 'versions-le-it-'));
	for (const [path, content] of Object.entries(files)) {
		mkdirSync(join(root, path, '..'), { recursive: true });
		writeFileSync(join(root, path), content);
	}
	return vscode.Uri.file(root);
}

async function compare(root: vscode.Uri): Promise<string> {
	await vscode.commands.executeCommand('versions-le.compare', root);
	const report = vscode.workspace.textDocuments
		.filter((doc) => doc.languageId === 'markdown' && doc.getText().includes('Versions-LE report'))
		.at(-1);
	assert.ok(report, 'no report document found');
	return report.getText();
}

describe('Versions-LE integration', function () {
	this.timeout(30_000);

	it('activates', async () => {
		const extension = vscode.extensions.getExtension(EXTENSION_ID);
		assert.ok(extension, `extension ${EXTENSION_ID} not found`);
		await extension.activate();
		assert.strictEqual(extension.isActive, true);
	});

	it('registers every declared command', async () => {
		const extension = vscode.extensions.getExtension(EXTENSION_ID);
		await extension?.activate();
		const commands = await vscode.commands.getCommands(true);
		for (const id of ['versions-le.compare', 'versions-le.openSettings', 'versions-le.help']) {
			assert.ok(commands.includes(id), `missing command: ${id}`);
		}
	});

	it('finds a disjoint constraint across two crates, labelled by their paths', async () => {
		const text = await compare(
			tree({ 'api/Cargo.toml': '[dependencies]\nregex = "1"\n', 'web/Cargo.toml': '[dependencies]\nregex = "2"\n' }),
		);
		assert.ok(text.includes('**disjoint-constraint** · cargo · `regex`'), text);
		assert.ok(text.includes('`api/Cargo.toml` · `dependencies.regex` = `1`'), text);
	});

	it('reads a workflow under .github/workflows and checks its toolchain against rust-version', async () => {
		const text = await compare(
			tree({
				'Cargo.toml': '[package]\nrust-version = "1.88"\n',
				'.github/workflows/ci.yml': 'jobs:\n  t:\n    steps:\n      - uses: dtolnay/rust-toolchain@1.85\n',
			}),
		);
		assert.ok(text.includes('CI builds on 1.85.0, below the declared minimum 1.88.0'), text);
	});

	it('honours an exclude pattern holding a comma or a brace group', async function () {
		this.timeout(60_000);
		const root = tree({
			'keep/Cargo.toml': '[dependencies]\nx = "1"\n',
			'b/Cargo.toml': '[dependencies]\nx = "1"\n',
			'a,b/Cargo.toml': '[dependencies]\nx = "2"\n',
			'skip/Cargo.toml': '[dependencies]\nx = "3"\n',
			'also/Cargo.toml': '[dependencies]\nx = "4"\n',
		});
		const config = vscode.workspace.getConfiguration('versions-le');
		await config.update('exclude', ['**/a,b/**', '**/{skip,also}/**'], vscode.ConfigurationTarget.Global);
		try {
			const text = await compare(root);
			assert.ok(text.includes('- `keep/Cargo.toml`'), text);
			assert.ok(text.includes('- `b/Cargo.toml`'), text);
			assert.ok(!text.includes('a,b/'), text);
			assert.ok(!text.includes('skip/'), text);
			assert.ok(!text.includes('also/'), text);
		} finally {
			await config.update('exclude', undefined, vscode.ConfigurationTarget.Global);
		}
	});

	it('offers its MCP server to agent mode', async () => {
		// The registration itself is only observable in a real host, which
		// scripts/e2e-vsix.js covers against the installed VSIX.
		const extension = vscode.extensions.getExtension(EXTENSION_ID);
		await extension?.activate();
		assert.strictEqual(
			typeof vscode.lm.registerMcpServerDefinitionProvider,
			'function',
			'this VS Code build predates the MCP provider API',
		);
		const providers = extension?.packageJSON.contributes.mcpServerDefinitionProviders as {
			id: string;
			label: string;
		}[];
		assert.deepStrictEqual(
			providers.map((p) => p.id),
			['versions-le'],
		);
	});
});
