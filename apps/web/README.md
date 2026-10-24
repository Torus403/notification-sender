# web

The marketing site and blog, built with [EmDash](https://github.com/emdash-cms/emdash) (an Astro CMS) from its Cloudflare marketing template. Deploys to Cloudflare Workers with D1 for content and R2 for media.

## Purpose

Explain the product to developers and convince them to try it: what it does, how it compares, what it costs, and the engineering behind it.

## Scope

In scope: home, pricing, comparison and migration pages (from OneSignal, Expo push, raw FCM), the engineering blog, legal pages and the changelog. Content is edited in the CMS.

Out of scope: developer documentation (`apps/docs`) and anything behind sign-in (`apps/platform`).

## Working on it

```sh
pnpm --filter @notification-sender/web dev      # local dev with the seeded content
pnpm --filter @notification-sender/web deploy   # build and wrangler deploy
```

Before the first deploy, create the D1 database `notification-sender-web` and the R2 bucket `notification-sender-web-media` (or rename them in `wrangler.jsonc`). The seed in `seed/seed.json` is the template's sample content. `AGENTS.md` and `.agents/skills` are EmDash's guides for coding agents.
