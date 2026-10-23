# platform

The customer app: API keys, apps, logs and billing. TanStack Start, deployed to Cloudflare Workers.

```sh
pnpm --filter @notification-sender/platform dev      # http://localhost:3000
pnpm --filter @notification-sender/platform deploy   # build and wrangler deploy
```

Routes live in `src/routes` (file-based). Shared UI comes from `@notification-sender/ui`, brand tokens from `@notification-sender/design-tokens`, and API calls go through `@notification-sender/api-client`.
