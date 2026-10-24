# spec

`openapi.json` is the API contract. `push-api` in `apps/engine` will generate it, and `packages/api-client`, the SDKs and the docs API reference are built from it. CI will fail when the committed file is out of date.

Until `push-api` exists this is an empty placeholder.
