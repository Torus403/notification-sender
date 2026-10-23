# notification-sender

A managed push notification service for iOS and Android: one API for your backend, direct delivery through APNs and FCM, and honest per-device results.

This is the company monorepo. Each app builds and deploys on its own.

## Layout

| Path            | What it is                                                                       | Deploys to                      |
| --------------- | -------------------------------------------------------------------------------- | ------------------------------- |
| `apps/engine`   | Push delivery engine. Rust, one Cargo workspace.                                 | AWS                             |
| `apps/web`      | Marketing site and blog. EmDash (Astro).                                         | Cloudflare                      |
| `apps/platform` | Customer app. TanStack Start.                                                    | Cloudflare Workers              |
| `apps/docs`     | Developer docs. Astro Starlight.                                                 | Cloudflare                      |
| `apps/mobile`   | Mobile app. Expo.                                                                | App Store, Google Play          |
| `packages/*`    | Shared TypeScript: `config`, `design-tokens`, `ui`, `api-client`. Internal only. |                                 |
| `sdks/server/*` | TypeScript, Python, Go and Rust SDKs for sending notifications.                  | npm, PyPI, Go mirror, crates.io |
| `sdks/client/*` | Swift, Kotlin, React Native and Flutter SDKs for mobile apps.                    | SPM mirror, Maven, npm, pub.dev |
| `spec/`         | `openapi.json`, the API contract everything else is generated from.              |                                 |
| `infra/`        | Infrastructure as code for AWS and Cloudflare.                                   |                                 |
| `tools/`        | Repo scripts (SDK mirroring, codegen).                                           |                                 |
| `examples/`     | Sample apps for the SDK quickstarts.                                             |                                 |

## Working in the repo

TypeScript uses pnpm workspaces and Turborepo; Rust uses Cargo in `apps/engine`.

```sh
pnpm install
pnpm dev          # every app's dev server
pnpm build        # build everything (cached by Turborepo)
pnpm typecheck
pnpm lint         # Biome
```

CI runs only what a change touches: `engine.yml` for `apps/engine`, `js.yml` for the TypeScript workspace (Turborepo `--affected`), and `sdks.yml` per SDK. `mirror-sdks.yml` pushes the Swift and Go SDKs to their public repos on a release tag.
