import { describe, expect, it } from 'vitest';
import { errorMessage, sanitizeErrorMessage } from './errors';

describe('sanitizeErrorMessage', () => {
	it('redacts user directories', () => {
		expect(sanitizeErrorMessage('ENOENT /Users/alice/project/data.json')).toBe(
			'ENOENT /Users/***/project/data.json',
		);
		expect(sanitizeErrorMessage('read /home/bob/x failed')).toBe(
			'read /home/***/x failed',
		);
		expect(sanitizeErrorMessage('open C:\\Users\\carol\\f.csv')).toBe(
			'open C:\\Users\\***\\f.csv',
		);
	});

	it('redacts credential-shaped fragments', () => {
		expect(sanitizeErrorMessage('auth password: hunter2 rejected')).toBe(
			'auth password=*** rejected',
		);
		expect(sanitizeErrorMessage('token=abc123')).toBe('token=***');
		expect(sanitizeErrorMessage('api key: xyz')).toBe('api key=***');
	});

	it('leaves ordinary messages alone', () => {
		expect(sanitizeErrorMessage('Failed to parse JSON: bad input')).toBe(
			'Failed to parse JSON: bad input',
		);
	});
});

describe('errorMessage', () => {
	it('takes the message from a real Error', () => {
		expect(errorMessage(new Error('parse failed'))).toBe('parse failed');
	});

	it('stringifies a thrown non-Error instead of yielding undefined', () => {
		// The format parsers used `(error as Error).message`, which the compiler
		// accepts and which produces "Failed to parse JSON: undefined" the moment
		// a parser throws anything that is not an Error.
		expect(errorMessage('boom')).toBe('boom');
		expect(errorMessage({ code: 'EBADF' })).toBe('[object Object]');
		expect(errorMessage(undefined)).toBe('undefined');
	});

	it('handles a subclassed Error', () => {
		class ParseError extends Error {}
		expect(errorMessage(new ParseError('bad token'))).toBe('bad token');
	});
});
