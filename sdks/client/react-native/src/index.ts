export interface NotificationSenderConfig {
	publishableKey: string;
	baseUrl: string;
}

/**
 * Entry point for the React Native client SDK. Device registration and open tracking land here
 * once the matching endpoints exist in spec/openapi.json.
 */
export function configure(config: NotificationSenderConfig): Readonly<NotificationSenderConfig> {
	if (!config.publishableKey) throw new Error("publishableKey is required");
	return Object.freeze({ ...config, baseUrl: config.baseUrl.replace(/\/+$/, "") });
}
