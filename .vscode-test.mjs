import { defineConfig } from '@vscode/test-cli';

// No GPU process. Under xvfb on a CI runner there is no GPU to start, and
// the window has sat unresponsive from launch until the test run gave up
// on it: "CodeWindow: detected unresponsive", sixteen seconds in, before a
// single test had run. Software rendering has nothing to wait for.
const launchArgs = ['--disable-extensions', '--disable-gpu'];

const mocha = {
	ui: 'bdd',
	timeout: 30_000,
};

export default defineConfig([
	{
		label: 'integration',
		files: 'out-test/integration/**/*.test.js',
		version: 'stable',
		launchArgs,
		mocha,
	},
	{
		// Installing a language pack is not enough for `--locale=de`: the host
		// applies a display language from `languagepacks.json` in its user data
		// directory, and writes that file on the first start that finds the
		// pack installed. This launch installs the pack and starts the host
		// once, so the `german host` launch below is the one that reads German.
		label: 'german language pack',
		files: 'out-test/locale-setup/**/*.test.js',
		version: 'stable',
		installExtensions: ['MS-CEINTL.vscode-language-pack-de'],
		launchArgs: ['--disable-gpu', '--locale=de'],
		mocha,
	},
	{
		// The same host in German, so a test can read what the runtime
		// localization mechanism actually loaded: a report the extension wrote.
		// Parity of the catalogue files says nothing about that, and this is
		// the only place it is shown.
		//
		// A display language needs its language pack, installed and registered
		// by the launch above; `--disable-extensions` would switch it off
		// again, so this config does not pass it.
		label: 'german host',
		files: 'out-test/locale/**/*.test.js',
		version: 'stable',
		installExtensions: ['MS-CEINTL.vscode-language-pack-de'],
		launchArgs: ['--disable-gpu', '--locale=de'],
		mocha,
	},
]);
