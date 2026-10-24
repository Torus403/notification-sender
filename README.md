# notification-sender

A managed push notification service for iOS and Android: one API for your backend, direct delivery through APNs and FCM, and honest per-device results.

This is the company monorepo. Each app builds and deploys on its own. [`PLAN.md`](PLAN.md) says what ships first, in what order, and what is deliberately left for later.

## Layout

| Path            | What it is                                                        | Deploys to         |
| --------------- | ----------------------------------------------------------------- | ------------------ |
| `apps/engine`   | The core API and push delivery engine. Rust, one Cargo workspace. | AWS                |
| `apps/platform` | Customer dashboard (planned; README only).                        | Cloudflare Workers |
| `apps/web`      | Marketing site and blog (planned; README only).                   | Cloudflare         |
| `apps/docs`     | Developer docs (planned; README only).                            | Cloudflare         |
| `packages/*`    | Shared TypeScript (none yet). Internal only.                      |                    |
| `infra/`        | Infrastructure as code for AWS and Cloudflare.                    |                    |

Every folder's README states its purpose and scope. The engine's design is in [`apps/engine/ARCHITECTURE.md`](apps/engine/ARCHITECTURE.md).

## Working in the repo

Each app owns its build, format and lint tooling; nothing language-specific lives at the root. The engine uses Cargo from `apps/engine`. TypeScript apps are pnpm workspaces, each with its own Biome config.

```sh
pnpm install
pnpm -r <script>  # run a script in every TypeScript app, e.g. pnpm lint, pnpm build
```

CI is one workflow per app, triggered by path filters on that app's folder.
