# docs

Developer docs: guides, SDK quickstarts and, later, the API reference rendered from `spec/openapi.json`. Astro Starlight, deployed to Cloudflare Workers as static assets.

```sh
pnpm --filter @notification-sender/docs dev
pnpm --filter @notification-sender/docs deploy
```

Pages are Markdown/MDX in `src/content/docs`. The two example pages come from the Starlight template.
