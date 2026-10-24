# @notification-sender/api-client

A typed TypeScript client for the engine's API, used by `apps/platform` to call the management API.

## Scope

The types in `src/schema.d.ts` are generated from `spec/openapi.json`; never edit them by hand. Regenerate after the spec changes:

```sh
pnpm --filter @notification-sender/api-client generate
```

`src/index.ts` adds a thin `openapi-fetch` wrapper. This is internal; the public server SDKs come later (see `PLAN.md`).
