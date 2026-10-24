# spec

`openapi.json` is the API contract.

## Scope

`push-api` in `apps/engine` will generate it from its route definitions, and CI will fail when the committed file is out of date. `packages/api-client` and the API reference in `apps/docs` are built from it, and later the SDKs.

The file describes the public HTTP API only. The engine's internal design is in `apps/engine/ARCHITECTURE.md` and the engine specification. Until `push-api` exists this is an empty placeholder.
