/**
 * Redact user directories and credential-shaped fragments from messages
 * before they reach notifications or logs.
 */
export function sanitizeErrorMessage(message: string): string {
	return message
		.replace(/\/Users\/[^/]+\//g, '/Users/***/')
		.replace(/\/home\/[^/]+\//g, '/home/***/')
		.replace(/C:\\Users\\[^\\]+\\/g, 'C:\\Users\\***\\')
		.replace(/password[=:]\s*[^\s]+/gi, 'password=***')
		.replace(/token[=:]\s*[^\s]+/gi, 'token=***')
		.replace(/key[=:]\s*[^\s]+/gi, 'key=***');
}

/**
 * The message from an unknown thrown value.
 *
 * The format parsers each used `(error as Error).message`, which is a lie the
 * compiler cannot catch: a parser that throws a string or a plain object
 * produces `undefined` there, and the user sees "Failed to parse JSON:
 * undefined". `extract.ts` already guarded with `instanceof Error`; this makes
 * that the single convention.
 */
export function errorMessage(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}
