import { beforeEach, describe, expect, it } from 'vitest';
import {
	_outputChannelLines,
	_resetMockState,
	_setConfig,
} from '../__mocks__/vscode';
import { createTelemetry } from './telemetry';

beforeEach(() => {
	_resetMockState();
});

describe('telemetry', () => {
	it('writes nothing while telemetryEnabled is off (the default)', () => {
		const telemetry = createTelemetry();
		telemetry.event('extension-activated');
		expect(_outputChannelLines()).toHaveLength(0);
		telemetry.dispose();
	});

	it('logs events to the output channel when enabled', () => {
		_setConfig('versions-le.telemetryEnabled', true);
		const telemetry = createTelemetry();
		telemetry.event('command', { name: 'detect' });
		const lines = _outputChannelLines();
		expect(lines).toHaveLength(1);
		expect(lines[0]).toContain('command');
		expect(lines[0]).toContain('detect');
		telemetry.dispose();
	});
});
