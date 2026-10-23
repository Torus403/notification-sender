import createClient from "openapi-fetch";
import type { paths } from "./schema";

export type { paths } from "./schema";

export function createApiClient(options: { baseUrl: string; apiKey?: string }) {
	return createClient<paths>({
		baseUrl: options.baseUrl,
		headers: options.apiKey ? { Authorization: `Bearer ${options.apiKey}` } : undefined,
	});
}
