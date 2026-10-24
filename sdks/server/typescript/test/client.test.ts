import assert from "node:assert/strict";
import { test } from "node:test";
import { NotificationSender } from "../src/index.ts";

test("requires an api key", () => {
	assert.throws(() => new NotificationSender({ apiKey: "", baseUrl: "https://api.example.com" }));
});

test("trims a trailing slash from the base url", () => {
	const client = new NotificationSender({ apiKey: "key", baseUrl: "https://api.example.com/" });
	assert.equal(client.baseUrl, "https://api.example.com");
});
