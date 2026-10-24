# notification-sender

A managed push notification service for iOS and Android: one API for your backend, direct delivery through APNs and FCM, and honest per-device results.

This is the company monorepo. Each app builds and deploys on its own. [`PLAN.md`](PLAN.md) says what ships first, in what order, and what is deliberately left for later.

## Layout

| Path            | What it is                                                                       | Deploys to         |
| --------------- | -------------------------------------------------------------------------------- | ------------------ |
| `apps/engine`   | The core API and push delivery engine. Rust, one Cargo workspace.                | AWS                |
| `apps/platform` | Customer dashboard. TanStack Start.                                              | Cloudflare Workers |
| `apps/web`      | Marketing site and blog. EmDash (Astro).                                         | Cloudflare         |
| `apps/docs`     | Developer docs. Astro Starlight.                                                 | Cloudflare         |
| `packages/*`    | Shared TypeScript: `config`, `design-tokens`, `ui`, `api-client`. Internal only. |                    |
| `spec/`         | `openapi.json`, the API contract the dashboard client and API reference use.     |                    |
| `infra/`        | Infrastructure as code for AWS and Cloudflare.                                   |                    |

Every folder's README states its purpose and scope. The engine's design is in [`apps/engine/ARCHITECTURE.md`](apps/engine/ARCHITECTURE.md).

## Working in the repo

TypeScript uses pnpm workspaces and Turborepo; Rust uses Cargo in `apps/engine`.

```sh
pnpm install
pnpm dev          # every app's dev server
pnpm build        # build everything (cached by Turborepo)
pnpm typecheck
pnpm lint         # Biome
```

CI runs only what a change touches: `engine.yml` for `apps/engine`, and `js.yml` for the TypeScript workspace (Turborepo `--affected`).
