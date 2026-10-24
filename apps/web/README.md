# web

Marketing site and blog, built with [EmDash](https://github.com/emdash-cms/emdash) (an Astro CMS) from its Cloudflare marketing template. Deploys to Cloudflare Workers with D1 for content and R2 for media.

```sh
pnpm --filter @notification-sender/web dev      # local dev with the seeded content
pnpm --filter @notification-sender/web deploy   # build and wrangler deploy
```

Before the first deploy, create the D1 database `notification-sender-web` and the R2 bucket `notification-sender-web-media` (or rename them in `wrangler.jsonc`). The seed in `seed/seed.json` is the template's sample content. `AGENTS.md` and `.agents/skills` are EmDash's guides for coding agents.
