export interface NotificationSenderOptions {
	apiKey: string;
	baseUrl: string;
}

/** Entry point for the server SDK. Endpoint methods are generated from spec/openapi.json as the API lands. */
export class NotificationSender {
	readonly baseUrl: string;
	readonly #apiKey: string;

	constructor(options: NotificationSenderOptions) {
		if (!options.apiKey) throw new Error("apiKey is required");
		this.#apiKey = options.apiKey;
		this.baseUrl = options.baseUrl.replace(/\/+$/, "");
	}

	get headers(): Record<string, string> {
		return { Authorization: `Bearer ${this.#apiKey}`, "Content-Type": "application/json" };
	}
}
