import * as assert from 'node:assert';
import * as vscode from 'vscode';

/**
 * The German language pack is installed, and this launch registers it.
 *
 * Installing a pack is not enough for `--locale=de`: the host applies a
 * display language from `languagepacks.json` in its user data directory, and
 * writes that file on the first start that finds the pack installed. So a pack
 * takes effect from the launch after the one that installed it. The `german
 * language pack` config in `.vscode-test.mjs` installs the pack and runs this
 * test, and the `german host` config after it is that next launch. The host
 * may still be in English here; what is held is the precondition the German
 * tests depend on, so a failed install is named here rather than showing up
 * there as the wrong language.
 */
describe('German language pack', () => {
	it('is installed into the test profile', () => {
		const pack = vscode.extensions.getExtension('MS-CEINTL.vscode-language-pack-de');
		assert.ok(pack, 'MS-CEINTL.vscode-language-pack-de is not installed');
	});
});
