# notification-sender

A managed push notification service for iOS and Android: one API for your backend, direct delivery through APNs and FCM, and honest per-device results.

This is the company monorepo. Each app builds and deploys on its own.

## Layout

| Path | What it is | Deploys to |
| --- | --- | --- |
| `apps/engine` | Push delivery engine. Rust, one Cargo workspace. | AWS |
| `apps/web` | Marketing site and EmDash blog. Astro. | Cloudflare |
| `apps/platform` | Customer app. TanStack Start. | Cloudflare Workers |
| `apps/docs` | Developer docs. | Cloudflare |
| `apps/mobile` | Mobile app. Expo. | App Store, Google Play |

SDKs will live in `sdks/` and shared TypeScript in `packages/` once the first ones exist.
