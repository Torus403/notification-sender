# infra/cloudflare

Account-level Cloudflare config shared by `apps/web`, `apps/platform` and `apps/docs`.

## Scope

DNS for the company domain (including the `api` record pointing at the AWS load balancer, DNS-only and not proxied), zone settings and any bindings more than one app uses. Per-app resources stay in each app's `wrangler.jsonc`. Empty until it's needed.
