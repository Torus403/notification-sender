# packages

Internal TypeScript shared by `apps/platform`, `apps/web` and `apps/docs`. Nothing here is published.

| Package         | Purpose                                                                            |
| --------------- | ---------------------------------------------------------------------------------- |
| `config`        | Shared `tsconfig` presets. Lint and format settings live in the root `biome.json`. |
| `design-tokens` | Brand colours, radius and spacing as CSS variables and as a TypeScript object.     |
| `ui`            | React components shared by the dashboard and the websites.                         |
| `api-client`    | A typed client for the engine's API, generated from `spec/openapi.json`.           |

A package belongs here only when at least two apps use it. Code used by one app stays in that app.
