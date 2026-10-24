# Plan

What we are building, what ships first, in what order, and what waits.

## The product

A managed push notification service for iOS and Android. A customer's backend calls one API to notify a user; we resolve that user's devices, deliver through Apple Push Notification service (APNs) and Firebase Cloud Messaging (FCM) directly, and report an honest result for every device. Customers bring their own Apple and Google credentials; we never need their database or their users' passwords.

## What ships now

The repo holds only the pieces in the first release. Each folder has a README that states its purpose and scope.

| Piece           | Path            | What it is                                                                                                                                                              |
| --------------- | --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Engine          | `apps/engine`   | The core: server, device and management APIs, and the delivery engine. Rust on AWS. Its architecture is in [`apps/engine/ARCHITECTURE.md`](apps/engine/ARCHITECTURE.md) |
| Platform        | `apps/platform` | The customer dashboard: projects, apps, credentials, API keys, test sends, delivery history                                                                             |
| Web             | `apps/web`      | Marketing site and blog                                                                                                                                                 |
| Docs            | `apps/docs`     | Developer documentation and the API reference                                                                                                                           |
| Shared packages | `packages/`     | Internal TypeScript shared by the websites, once two of them need the same code                                                                                         |
| Infrastructure  | `infra/`        | AWS for the engine, Cloudflare for the websites                                                                                                                         |

Until SDKs exist, customers call the HTTP API directly, and the docs show HTTP examples.

## Order of work

The engine comes first because everything else depends on it, and because it is where the product's quality is decided. Milestones follow the engine specification; each ends on a test, not a date.

| Milestone    | Scope                                                                                             | Exit criterion                                                                             | Status                        |
| ------------ | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ----------------------------- |
| M0           | Domain model, schema, the accept/claim/record transactions                                        | State-machine and database invariant tests pass                                            | Done (`push-core`, `push-db`) |
| Architecture | This review: AWS services, scaling, failure, cost, open decisions                                 | Jack signs off the open decisions                                                          | In review                     |
| M1           | APNs and FCM transports, fake provider servers, credential verification, real-device sends        | Repeatable sends to real iOS and Android devices with every provider response classified   | Next                          |
| M2           | `push-api` and `push-worker` end to end on Postgres and SQS, the recovery sweeps, fault injection | Killing any process at any step loses no accepted work and double-bills nothing            |                               |
| M3           | AWS infrastructure as code, tenant isolation, credential encryption, rate limits, audit           | Staging and production stacks built from code; failover and credential-rotation tests pass |                               |
| M4           | Developer beta: management API, dashboard, docs, test sends, templates                            | An outside developer can integrate, send and debug without help                            |                               |
| M5           | Benchmarks and published engineering write-ups                                                    | Capacity, cost per million and p99 latency measured and reproducible                       |                               |

The websites move in parallel where they do not depend on the engine: the marketing site and docs skeleton now; the dashboard once the management API exists (M4).

## Open decisions

The architecture review lists the decisions that need Jack's call, each with a recommendation: region, Aurora versus RDS, dashboard authentication, OpenTofu versus Terraform, disaster recovery stance, and moving the engine spec into the repo. See [section 13 of the architecture](apps/engine/ARCHITECTURE.md#13-open-decisions). The engine specification's own decisions D1 to D7 are also still open.

## Later

Not in the repo until their time comes. The earlier scaffolding for the SDKs, the mobile app and the SDK tooling is in git history at commit `0077801` if any of it is useful when the work starts.

**SDKs**
- Server SDKs: TypeScript, Python, Go and Rust, generated from the engine's OpenAPI spec with hand-written ergonomics on top. Swift and Go would need public mirror repos, because Swift Package Manager and `go get` install from git.
- Client SDKs: Swift (iOS), Kotlin (Android), React Native and Flutter, plus an Expo config plugin. They register devices, report the APNs environment and permission state, bind users with identity tokens, and report opens. A later mode lets the Android SDK draw notifications from data messages, which avoids FCM's collapse throttling and enables "received" events.
- Example apps for each SDK's quickstart.

**Mobile app**
- A companion app (Expo) for testing pushes without building your own app, and as the first real user of the client SDKs.

**Developer tooling**
- CLI: verify credentials, send a test, tail deliveries live.
- MCP server and agent skills, so coding agents can set up and debug pushes.
- Migration importers from OneSignal, Expo push and raw FCM/APNs token lists (the engine's import endpoint is in scope; the per-vendor tooling is not).

**Product features beyond v1**
- Broadcasts and segments, with asynchronous recipient expansion.
- Scheduled and timezone-aware sends.
- Outbound webhooks for delivery events.
- Live Activities and broadcast channels on iOS.
- Localisation of templates.
- In-app inbox (a notification feed).
- A/B testing and conversion attribution.

**Infrastructure beyond one region**
- Cells (independent stacks per group of tenants) when one database writer is no longer enough.
- A second region for EU data residency or disaster recovery.
- An analytics store for long-term delivery and interaction history.
