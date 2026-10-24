# Engine architecture

Status: proposal for review, 24 October 2026. Nothing here is built on AWS yet.

This document decides how the push engine runs: which AWS services it uses, how work moves between them, where state lives, how it scales and fails, and what it costs. The delivery protocol itself (state machines, transaction boundaries, the SQS message contract, error tables, invariants) is specified in the [Push Delivery Engine Specification v0.1](https://claude.ai/artifact/9xNDseoAYnaWxst78tQsqV), referred to below as "the spec". Where this document changes the spec, section 12 lists the change.

Earlier design passes fixed Rust and AWS and named ECS Fargate, RDS, SQS, KMS and Terraform without checking them against current documentation or costing them. This pass does that. Each choice below gives the options considered, the recommendation, and what evidence would change it. Facts about Apple, Google and AWS were checked against their documentation in October 2026; sources are in section 14.

## Contents

1. [Decision summary](#1-decision-summary)
2. [What the engine owns](#2-what-the-engine-owns)
3. [Constraints that shape the design](#3-constraints-that-shape-the-design)
4. [System overview](#4-system-overview)
5. [Compute](#5-compute)
6. [Database](#6-database)
7. [Queue](#7-queue)
8. [APNs and FCM transports](#8-apns-and-fcm-transports)
9. [Network, ingress, keys and accounts](#9-network-ingress-keys-and-accounts)
10. [Observability](#10-observability)
11. [Capacity, scaling, failure and cost](#11-capacity-scaling-failure-and-cost)
12. [Changes to the spec and the current code](#12-changes-to-the-spec-and-the-current-code)
13. [Open decisions](#13-open-decisions)
14. [Sources](#14-sources)

## 1. Decision summary

"Decided" means Jack has chosen it. "Recommended" means this document proposes it and it needs sign-off. "Open" means it needs Jack's call before the work it blocks.

| Area | Choice | Status |
| --- | --- | --- |
| Language | Rust (Tokio, Axum, hyper/h2, rustls, SQLx) | Decided |
| Cloud | AWS, one region, at least two availability zones | Decided |
| Region | `us-east-1`, confirmed by a latency test to APNs and FCM | Open (A1) |
| Compute | ECS on Fargate, ARM64. Two services, `push-api` and `push-worker`, from one image | Recommended |
| Database | Aurora PostgreSQL, provisioned Graviton writer plus one reader in another AZ | Open (A2) |
| Queue | One SQS Standard queue with fair queues; Postgres stays the source of truth | Recommended |
| Provider transports | Our own HTTP/2 clients on hyper/h2 and rustls; no third-party push service | Recommended |
| Ingress | Application Load Balancer with AWS WAF; DNS at Cloudflare, not proxied | Recommended |
| Egress | Private subnets behind one Regional NAT Gateway | Recommended |
| Secrets | KMS envelope encryption (two keys, as in the spec); IAM auth to the database | Recommended |
| Observability | OpenTelemetry in the code; CloudWatch for metrics and alarms; trace backend later | Recommended |
| Infrastructure as code | OpenTofu (Terraform-compatible HCL), state in S3 with native locking | Open (A4) |
| AWS accounts | AWS Organizations: separate production, staging and management accounts | Recommended |
| Dashboard auth | Managed auth provider with organisations; the engine verifies its tokens | Open (A3) |
| Not used in v1 | Redis, Kafka, Kubernetes, DynamoDB, API Gateway, Lambda, multi-region | Recommended |

## 2. What the engine owns

The engine is the product. It exposes three APIs on one host (`api.<domain>`):

- **Server API** (secret key): a customer's backend sends a notification and reads its outcome.
- **Device API** (publishable key, then an installation token): a customer's app registers its device, binds it to a user and reports interactions.
- **Management API** (dashboard session): `apps/platform` configures projects, apps, credentials and keys, sends tests and reads history.

Behind them, the engine owns intake, the device registry, user binding, dispatch to APNs and FCM, retries, recovery, delivery history and metering. It does not own dashboard sign-in, billing plans, Stripe, the websites, or anything on the device.

Two deployable processes share one domain crate and one schema:

| Process | Does | Never does |
| --- | --- | --- |
| `push-api` | Authenticates requests, validates, accepts sends (idempotency, recipient resolution, commit), serves device and management endpoints, publishes post-commit queue hints | Talks to APNs or FCM, holds provider credentials in plaintext |
| `push-worker` | Claims deliveries, submits to APNs and FCM, records outcomes, retries, runs the recovery sweeps, verifies credentials | Serves public traffic |

The split exists for security and scaling, not code organisation: only the worker can decrypt provider credentials, and the two scale on different signals (request rate versus queue backlog).

## 3. Constraints that shape the design

### Workload

A send is: accept and persist intent, resolve one user to a handful of devices, submit one HTTP/2 request per device to Apple or Google, record the result. The work is I/O-bound. Traffic is bursty: most tenants are quiet, and one can submit hundreds of thousands of sends in a minute. Monthly volume converts to average rates as follows.

| Monthly deliveries | Average per second | Plausible peak (10× average) |
| --- | --- | --- |
| 10 million | 4 | 40 |
| 100 million | 39 | 390 |
| 1 billion | 386 | 3,860 |
| 10 billion | 3,858 | 38,580 |

Peaks are what matter. The accept path must absorb a burst synchronously; the dispatch path may fall behind, as long as work waits durably and other tenants keep moving.

### Guarantees

The spec (section 1) publishes seven guarantees. The ones that constrain infrastructure: a 202 means the notification and every delivery are committed to multi-AZ storage (G1); nothing is sent after its expiry (G4); a delivery is billed at most once (G5); ambiguous sends are retried at most once (G7). Every infrastructure choice below is checked against these.

### Provider facts

These are from Apple's and Google's current documentation (section 14). Several are newer than the earlier design passes.

**APNs**

- HTTP/2 over TLS 1.2 or later to `api.push.apple.com` or `api.sandbox.push.apple.com`, port 443 or 2197.
- One connection can carry tokens from only one developer team. On the first push, APNs binds the connection to that team and its bundle ids. Apple says to open separate connection pools per developer account.
- Since February 2025, token keys come in two kinds. Team-scoped keys work for every topic in the team and can be restricted to sandbox or production (two keys per environment). Topic-specific keys cover up to 400 named bundle ids in one environment (200 keys per environment). A connection opened with one kind of key rejects tokens from the other kind, from another environment, or from an unrelated key.
- With token auth, APNs allows one stream until the first request with a valid token, then raises the limit. Apple says not to assume a number.
- Provider JWTs must be refreshed at least every 60 minutes and at most every 20. Using a new token more often than every 20 minutes on one connection returns `TooManyProviderTokenUpdates`.
- Apple's best practices: do an uncached DNS lookup before each new connection; spread traffic over many connections rather than bursting over a few; reuse connections for hours or days; a PING after an hour idle is enough.
- Payload limit 4 KB (5 KB for VoIP). `apns-collapse-id` up to 64 bytes. APNs stores one undelivered notification per app per device.
- The documented error list includes reasons that are connection or key problems, not credential failures: `UnrelatedKeyIdInToken` ("open a new connection"), `BadEnvironmentKeyIdInToken`, `IdleTimeout`. `BadDeviceToken` also covers a token used against the wrong environment.
- Apple says 5xx responses may be retried after 15 minutes, with backoff.

**FCM HTTP v1**

- Default quota 600,000 messages per minute per Firebase project, counted in messages, with minutes that do not align to the clock. Client errors (4xx except 429) count toward the quota.
- `QUOTA_EXCEEDED` (429) requires exponential backoff with a minimum initial delay of one minute. `UNAVAILABLE` and `INTERNAL` retry with backoff and honour `Retry-After`.
- Per-device limits on Android: 240 messages per minute and 5,000 per hour.
- Notification messages are always collapsible and ignore `collapse_key`. Collapsible messages are throttled per device: a burst of 20, refilling one every three minutes. FCM stores at most four collapse keys per device.
- Payload limit 4,096 bytes. TTL from 0 to 28 days.

### What the measurements say about provider latency

Knock publishes daily response-time data from its production push traffic. For July to October 2026 it reports APNs at p50 64 ms, p95 121 ms, p99 145 ms, and FCM at p50 114 ms, p95 208 ms, p99 347 ms. These are someone else's numbers from an unstated region, but they set the scale: a provider round trip takes tens to hundreds of milliseconds, against single-digit milliseconds for our own processing. End-to-end latency is won or lost in queue wait, connection warmth and retries, not in CPU time.

## 4. System overview

```
                       Customer backend                 Customer app (device)
                              │ HTTPS                          │ HTTPS
                              ▼                                ▼
 apps/platform ──HTTPS──▶  Application Load Balancer + WAF  (api.<domain>)
 (Cloudflare)                              │
                                           ▼
                   ┌──────────── push-api (ECS Fargate, ≥2 tasks, 2 AZs) ─────────────┐
                   │ auth · validate · idempotency · resolve recipients · COMMIT · 202 │
                   └───────┬──────────────────────────────────────────┬────────────────┘
                           │ accept transaction                      │ post-commit hint
                           ▼                                          ▼
               Aurora PostgreSQL (writer + reader)          SQS Standard (fair queues)
               source of truth: deliveries are              "look at delivery X";
               the outbox                                     loss or duplication is harmless
                           ▲                                          │
                           │ claim / record (epoch-fenced)            ▼
                   ┌──────────── push-worker (ECS Fargate, ≥2 tasks, 2 AZs) ──────────┐
                   │ claim · decrypt (KMS, cached) · submit · record · retry · sweeps  │
                   │   APNs connection manager            FCM transport                │
                   └──────────────┬───────────────────────────────┬────────────────────┘
                                  │ HTTP/2 via NAT                 │ HTTP/2 via NAT
                                  ▼                                ▼
                       api.push.apple.com              fcm.googleapis.com
```

One send on the happy path is four short Postgres transactions and three SQS calls (each batched):

1. **Accept** (`push-api`): idempotency key, recipient resolution, notification, encrypted payload and one delivery row per device, committed before the 202.
2. **Mark enqueued** (`push-api`, after the response): send hints to SQS in batches of ten, then record `enqueued_at`. If this step is lost, the lost-hint sweep re-sends within about five seconds.
3. **Claim** (`push-worker`, on receiving a hint): take a fenced lease, re-check eligibility, write the attempt row and the usage ledger row.
4. **Submit** outside any transaction, then **record** the outcome under the lease epoch, and delete the SQS message.

This is spec section 6. The infrastructure consequence is that Postgres commit latency and commit throughput set the engine's ceiling; section 11 sizes it.

## 5. Compute

### Options

| Option | Fit for `push-worker` | Fit for `push-api` | Verdict |
| --- | --- | --- | --- |
| ECS on Fargate | Long-lived tasks keep warm HTTP/2 pools; no hosts to patch; ARM64 supported | Behind an ALB, autoscaling on request count or CPU | **Recommended** |
| ECS on EC2 (Graviton) | Same container, more control, cheaper at sustained high volume | Same | The upgrade path once volume is steady. Same image, no rewrite |
| EKS | Works, but we would operate Kubernetes for two services | Same | Rejected: operational cost with no benefit at this size |
| Lambda | Short-lived; cannot hold an APNs pool across invocations; a per-invocation TLS handshake defeats Apple's connection guidance | Would need RDS Proxy for connections | Rejected for both |
| App Runner | — | — | Closed to new customers; AWS points to ECS Express Mode |
| ECS Express Mode | Opinionated wrapper over Fargate plus an ALB | Same | Not needed: we define the same resources directly in IaC |

### Recommendation

ECS on Fargate, Linux ARM64 (Graviton), one container image with two binaries, run as two ECS services:

| Service | Starting size | Minimum tasks | Scales on |
| --- | --- | --- | --- |
| `push-api` | 0.5 vCPU, 1 GB | 2, one per AZ | ALB request count per target, then CPU |
| `push-worker` | 1 vCPU, 2 GB | 2, one per AZ | SQS backlog per task (visible messages divided by running tasks), then CPU |

The sizes are placeholders until the M5 benchmarks. Baseline capacity runs on-demand. Fargate Spot is available for ARM and can add worker capacity later, because a worker interrupted mid-send is already handled by the lease and ambiguity rules; it should never be the baseline.

Deployments are rolling with the ECS deployment circuit breaker and automatic rollback. A worker receives SIGTERM, stops polling, finishes in-flight sends (each bounded by a 10 s deadline), records them, and exits well inside Fargate's 120 s maximum stop timeout. Correctness never depends on that completing (spec section 10).

Schema migrations run as a one-off ECS task in the deploy pipeline before either service updates. Every migration is expand-then-contract, so old and new code run against the same schema during a rollout and a rollback.

**What would change this:** measured cost per million deliveries on Fargate well above EC2 at steady volume (move the worker service to an EC2 capacity provider), or a need for host-level network tuning that Fargate cannot give.

## 6. Database

The database is the outbox, the lease table, the idempotency store, the device registry and the billing ledger. Its commit latency is on the accept path, and its write throughput is the engine's ceiling. It is the most consequential infrastructure choice in this document.

### Options

| Option | Failover | Writes | Cost at launch (approx., `us-east-1`) | Verdict |
| --- | --- | --- | --- | --- |
| RDS PostgreSQL, Multi-AZ instance | 60 to 120 s; synchronous standby is not readable | Commit waits for synchronous EBS replication to the standby | `db.m7g.large` about $250/month for the pair, plus storage | Cheapest HA. Slowest failover and commits |
| RDS PostgreSQL, Multi-AZ DB cluster | Typically under 35 s; two readable standbys | Semi-synchronous to one of two standbys on local NVMe; AWS claims up to 2× faster commits | Three instances, roughly 3× a single instance | Good, but pays for three instances to get faster failover |
| **Aurora PostgreSQL, provisioned** | Typically under 30 s with a replica | Six copies across three AZs, quorum writes; readers share storage | `db.r7g.large` about $200/month per instance; writer plus reader about $400/month, plus storage and I/O | **Recommended** |
| Aurora Serverless v2 | As Aurora | As Aurora | Billed per ACU-hour; costs more than provisioned at steady load | Not for a steady write workload |
| Aurora DSQL | Multi-region active-active | Optimistic concurrency: conflicts fail at commit instead of waiting; at most 3,000 modified rows per transaction; no manual partitioning, no PL/pgSQL | Per request | Rejected: the accept path relies on `insert ... on conflict` waiting for a concurrent duplicate, the sweeps rely on `for update skip locked`, and retention relies on dropping partitions |
| PlanetScale Postgres | Primary plus two replicas across AZs; failover time undocumented | Local NVMe ("Metal") option | From $50/month; HA pricing not published | Credible, but outside our VPC, IAM and KMS; reach it over PrivateLink. Revisit if Aurora disappoints |
| DynamoDB | Multi-AZ by default | Scales horizontally | Per request | Rejected for v1: the schema's core rules are partial unique indexes (one active token per installation, one active user per installation) and the claim is a multi-table join, neither of which DynamoDB expresses. A possible store for delivery state much later |

### Recommendation (open decision A2)

Aurora PostgreSQL, provisioned, Graviton, one writer and one reader in a different AZ, latest major version Aurora offers (Aurora tends to trail RDS on new majors; the engine's CI runs Postgres 17 and nothing in the schema needs 18).

Why Aurora over RDS Multi-AZ:

- **Failover.** Under 30 s against 60 to 120 s. During a failover the API returns 503 and never a 202 (spec section 10), and customer backends retry with the same idempotency key. Thirty seconds is a retry or two; two minutes is an incident for every customer.
- **The second instance does work.** RDS Multi-AZ's standby is idle and unreadable. Aurora's reader is the failover target and also serves dashboard history queries (paginated deliveries, attempts), keeping them off the writer.
- **Storage.** No volume to size or grow; storage durability is three-AZ, not two.

The cost is about $150 to $200 a month more at launch. The risk is that Aurora commit latency or I/O cost under our write pattern turns out worse than RDS. So this decision is gated by an experiment in M2: run `push-db`'s accept, claim and record transactions at increasing concurrency against both Aurora and RDS Multi-AZ, and compare p50/p99 commit latency, throughput and cost per million deliveries. If RDS wins clearly, use RDS Multi-AZ DB cluster or instance and accept the failover window. Either way the code does not change: both are PostgreSQL.

Choose Aurora's I/O-Optimized storage configuration once I/O exceeds roughly a quarter of the database bill, which a write-heavy workload like this one is likely to reach.

### How the engine uses the database

- **Connections.** Each task holds a bounded SQLx pool (spec section 12: 32 for the API, 48 for the worker). Total connections stay well under the instance limit without RDS Proxy; add the proxy only if task count makes the arithmetic fail.
- **Authentication.** IAM database authentication for the `push_api` and `push_worker` roles, so no database password exists in the application's configuration. Tokens last 15 minutes and are only needed when opening a connection; the pool refreshes its connect options. Each role gets only the grants it needs.
- **Partitioning from day one.** `notifications`, `notification_payloads`, `deliveries` and `delivery_attempts` are range-partitioned by notification id, whose UUIDv7 prefix is time (as the spec already anticipates), weekly to start. `idempotency_keys` is not partitioned: its uniqueness must hold across all keys for 24 hours, and a time column in its key would break that; it is small and deleted in batches. Retention then drops whole partitions instead of deleting billions of rows, which is the difference between steady vacuum and permanent bloat. Converting a live, hot table to partitions later is a migration nobody wants to run.
- **Vacuum.** `deliveries` rows are updated three or four times in their first seconds of life, and the partial indexes on `state` mean those updates are not HOT. Set a lower `fillfactor` and aggressive per-table autovacuum on the delivery tables, and alarm on dead-tuple ratio and transaction-ID age.
- **Retention** (spec decision D3, still open): notification content 7 days, delivery metadata 30 days, ledger and aggregates 13 months. Section 12 notes a foreign key that currently prevents this.
- **Backups.** Continuous backup with point-in-time restore, 35 days. Copy daily snapshots to a second region for disaster recovery (section 11). Restore is rehearsed, not assumed.
- **What is not in Postgres.** Long-term interaction analytics and archived history go to S3 when volume needs it; there is no analytics database in v1.

## 7. Queue

### Options

| Option | Verdict |
| --- | --- |
| **SQS Standard with fair queues, carrying hints only** | **Recommended.** At-least-once, effectively unlimited throughput, multi-AZ, no capacity to manage. Fair queues (July 2025) deprioritise a noisy tenant without throttling it. Its backlog metric drives worker autoscaling directly. |
| Postgres as the queue (`select ... for update skip locked` polling, optionally `LISTEN/NOTIFY`) | Fewer moving parts, and the deliveries table is already the outbox. Rejected because it moves the "find work" query onto the database that is already the ceiling, and tenant fairness would have to be built into that query. Kept as the fallback the sweeper already implements. |
| SQS FIFO | Ordering is not needed; FIFO adds per-group ordering constraints and costs more. Rejected. |
| Kinesis, MSK (Kafka), EventBridge | Partitioned logs or event buses; the wrong shape for per-delivery work items with individual retries. Rejected. |

### How SQS is used

The spec (section 7) defines the contract: one queue, `push-dispatch`; messages under 300 bytes that say "look at delivery X"; the worker trusts only the row it claims; a dead-letter queue as an alarm, not a recovery path. Losing, duplicating or reordering messages can delay a send but cannot cause a wrong one. Additions from this pass:

- **Fair queue behaviour.** SQS flags a tenant as noisy when it holds more than about 10% of the queue's in-flight messages and at least 30 of its own, or more than 10% of consumer processing time. Consumers then receive quiet tenants' messages first; the noisy tenant still gets any spare capacity. There is no limit on the number of message groups. `MessageGroupId` is the environment id, as in the spec. Fairness only engages when enough messages are in flight at once, which a worker fleet holding hundreds of in-flight deliveries provides.
- **Fair queue cost.** Requests that carry a `MessageGroupId` are billed at the fair queue rate on top of the standard rate (about $0.50 per million instead of $0.40). Batching sends, receives and deletes in tens keeps this to well under a dollar per million deliveries.
- **Retries.** A retry within 15 minutes is a new hint with `DelaySeconds`. A retry further out relies on the lost-hint sweep, which re-hints any pending row whose `next_attempt_at` has passed. Postgres holds the schedule; SQS only wakes workers up.
- **Monitoring.** Alarm on oldest-message age, on `ApproximateNumberOfMessagesVisible` rising while `ApproximateNumberOfMessagesVisibleInQuietGroups` stays flat (one tenant's burst, expected) versus both rising (we are under-provisioned, not expected), and on any message in the DLQ.

## 8. APNs and FCM transports

The transports are the part of the engine that is genuinely ours: everything else is careful use of managed services. They live in a `push-providers` crate inside `push-worker`, take a resolved request, and return a `SubmissionOutcome` within a deadline. They never touch the database (spec section 8).

### Build, don't buy

We write the HTTP/2 clients on `hyper` and `h2` with `rustls`, rather than wrapping an existing APNs or FCM crate or reselling a push service. The value of the product is exactly the behaviour those libraries hide: connection scoping per team and key, GOAWAY handling, telling "never sent" apart from "maybe sent", and per-project throttling. Existing crates such as `a2` are useful prior art to read, not dependencies.

**Risk to retire first (M1 spike).** The spec's `NotSent` versus `Indeterminate` split depends on seeing HTTP/2 detail: whether a stream was refused (`REFUSED_STREAM`), and whether a stream id was above a GOAWAY's last-stream-id. We must confirm that `h2` exposes this through `hyper`'s client, or drive `h2` directly. If it cannot be seen reliably, those cases must be treated as `Indeterminate`, which changes how often the ambiguous-retry rule fires.

### APNs connection manager

The spec's design (section 8.2) holds up against Apple's current documentation, with these adjustments:

- **Pool key** is (APNs host, team id, key id). This already isolates teams, environments and the new topic-specific keys, because each key has its own id. The credential record must also store which environment the key may use and, for topic-specific keys, which bundle ids it covers (section 12).
- **Spread, don't pack.** Apple asks for traffic spread across connections. Keep a minimum of two connections for any pool that has sent in the last minute, and round-robin across them, rather than opening a second connection only when the first is full.
- **Fresh DNS per connection.** Resolve the APNs host with an uncached lookup before each new connection, as Apple asks, so connections spread across Apple's servers.
- **Warm-up.** A new token-auth connection allows one stream until its first authenticated request succeeds. The pool sends one request first and only then fans out.
- **Idle and health.** Close connections idle for 10 minutes (spec). Apple's guidance is a PING after an hour of inactivity, so the spec's 60 s idle PING can be relaxed; keep a PING as a liveness check before reusing a connection that has been idle for more than a few minutes.
- **Connection fan-out at scale.** Each worker opens pools for every team it serves, so connections grow as workers × active teams. That is fine at launch. At Stage 2 (section 11), route each environment's traffic to a subset of workers so pools stay warm and connection count stays bounded.

### FCM transport

The spec's design (section 8.3) holds: one shared HTTP/2 client to `fcm.googleapis.com` per worker, an OAuth token per service account refreshed before its one-hour expiry, and an adaptive token bucket per Firebase project. Adjustments:

- On `QUOTA_EXCEEDED`, wait at least 60 s before the first retry, as Google requires, in addition to halving the project's rate.
- Because Google counts 4xx responses against the customer's quota, the invalidation breaker (spec section 9.5) protects the customer's quota as well as their device list.
- Google documents notification messages as always collapsible (they ignore `collapse_key`), and documents a per-device throttle on collapsible messages: a burst of 20, then one every three minutes. Two consequences. Duplicate suppression on Android must use `android.notification.tag`, not `collapse_key`. And if the throttle applies to notification messages, as those two statements together imply, a chatty app sending many notifications to one device will see Google delay them. That needs a real-device test in M1 before we promise anything about Android bursts; if it holds, it raises the priority of the later SDK mode where our SDK draws the notification from a data message.

### Where providers are reached from

Workers reach APNs and FCM over the public internet through the NAT gateway (section 9). There is no private path to either provider.

## 9. Network, ingress, keys and accounts

### Network

- One VPC, two availability zones at launch (three when the worker fleet is large enough that losing one AZ's third matters less than losing half).
- Public subnets hold only the ALB and the NAT gateway. Tasks and the database sit in private subnets.
- **Egress through one Regional NAT Gateway** (November 2025): one resource that expands into each AZ that has workloads and keeps traffic in-zone. About $33/month per active AZ plus $0.045/GB. Push payloads are small, so data charges are negligible.
- An S3 gateway endpoint (free) for ECR image layers and archives. Interface endpoints for SQS, KMS, ECR, CloudWatch Logs and STS cost about $15/month each across two AZs, so they wait until NAT data charges or a security requirement justify them.
- Security groups: the ALB accepts 443 from anywhere; `push-api` accepts traffic only from the ALB; the database accepts only the two services; workers accept nothing inbound.
- Apple documents 17.0.0.0/8 as its push range, so APNs egress could be restricted by IP; Google's ranges cannot be pinned the same way. Egress filtering by hostname (AWS Network Firewall) is a later hardening step, not a launch requirement.

### Ingress

| Option | Verdict |
| --- | --- |
| **ALB + AWS WAF** | **Recommended.** TLS with an ACM certificate, HTTP/2 to clients, WAF managed rules and rate-based rules in front of our own per-key limits. About $25/month for the ALB at launch, plus WAF. |
| API Gateway (HTTP API) in front of the ALB | Adds latency, about $1 per million requests, and a second rate-limit layer that does not know our keys. Rejected. |
| CloudFront in front of the ALB | Useful for global TLS termination and DDoS absorption. Not needed in one region at launch. |
| Cloudflare proxying `api.<domain>` | Our DNS is at Cloudflare for the websites. Proxying would put a third party in the path of every API key and payload. Keep the API record DNS-only. |

`apps/platform` runs on Cloudflare Workers and calls the management API over the public internet like any other client. How it authenticates is open decision A3.

### Keys and secrets

The spec's credential design (section 11) stands: customer provider secrets are encrypted with per-credential data keys wrapped by a KMS key that only `push-worker` may use to decrypt, and each environment's data key is wrapped by a second KMS key. Checks against KMS limits:

- KMS allows 100,000 symmetric cryptographic requests per second per account in `us-east-1` (20,000 in `eu-west-2`, adjustable). With the spec's 15-minute decrypt cache, KMS calls scale with credentials × workers, not with sends: thousands per hour, not per second.
- Cost: $1 per key per month plus $0.03 per 10,000 requests. Negligible.
- Our own secrets: none long-lived. The database uses IAM auth, CI deploys through GitHub OIDC roles, and anything that remains (for example a Stripe key later) goes in Secrets Manager.

### AWS accounts

Use AWS Organizations with IAM Identity Center for human access: a management account (billing and identity only), a staging account and a production account. An account is the strongest boundary AWS offers: a mistaken policy in staging cannot reach production resources unless someone creates a cross-account trust on purpose, and the production blast radius is explicit. A separate account for log archive and security tooling can follow.

### Infrastructure as code (open decision A4)

| Option | Verdict |
| --- | --- |
| **OpenTofu** | **Recommended.** Terraform-compatible HCL under the MPL licence, governed by the Linux Foundation; native state encryption (our state will name databases, roles and keys); S3 state with native locking. The AWS provider is shared with Terraform. |
| Terraform | Equally capable; BSL licence since 2023 and now IBM-owned. The code is portable between the two, so this choice is cheap to reverse. |
| AWS CDK | Generates CloudFormation; TypeScript-defined infrastructure. Slower plan/apply loop and CloudFormation's failure modes. |
| SST, Pulumi | Viable, smaller ecosystems for this kind of plain AWS setup. |

Whichever is chosen, state lives in an encrypted, versioned S3 bucket in the management account with native locking, production changes go through a reviewed plan in CI, and customer secrets never pass through IaC.

## 10. Observability

Two separate concerns:

- **Customer-facing history** (a delivery's attempts and the provider's raw reason) comes from Postgres through the API, not from logs or traces. It has the same correctness requirements as the rest of the product.
- **Operating the engine** uses telemetry.

For operations:

- **Instrumentation:** `tracing` plus OpenTelemetry in both binaries. One trace per delivery spans accept, commit, hint, claim, provider request and record (the sent-at timestamp in the hint gives queue wait).
- **Metrics and alarms:** CloudWatch, because autoscaling and the SQS, ALB, Aurora and NAT metrics are there already. Application metrics go out as CloudWatch embedded metric format through logs or through an OpenTelemetry collector sidecar. Labels are low-cardinality only (provider, outcome, normalized reason, never a token, user or tenant id); per-tenant views come from Postgres.
- **Traces:** an OpenTelemetry collector sidecar on each task, exporting to X-Ray at first. Because the code speaks OTLP, moving traces to Honeycomb or Grafana Cloud later is a collector configuration change.
- **Service-level indicators from day one:** accept latency (p50/p99), accept availability, accept-to-provider-acknowledgement latency, queue oldest-message age, provider 429/5xx rates by provider, unknown-outcome rate, registration invalidation rate per app (the breaker's input), deliveries with no terminal state after their expiry (must be zero), and database commit latency.
- **Synthetic checks:** a small set of real iOS and Android devices receiving a scheduled test push through production, alarming on missing receipts. A provider's HTTP 200 is not proof of display.

## 11. Capacity, scaling, failure and cost

### Write budget per delivery

A one-device send costs four commits and about ten row writes (insert idempotency key, notification, payload, delivery; update enqueued; update delivery and insert attempt and ledger at claim; update attempt, delivery and notification at record). At 1 billion deliveries a month that averages about 1,500 commits per second and 4,000 row writes per second, with peaks several times higher. A single Postgres writer on a mid-sized Graviton instance should handle that range. That is an estimate, not a measurement; the M2 database experiment and the M5 benchmarks replace it.

The accept path is synchronous, so a burst of sends hits the writer directly. The levers, in order: group commit and multi-row inserts inside one accept; a batch send endpoint (many notifications per request) for customers looping over users; a larger writer; then cells.

### Scaling stages

| Stage | Trigger | Change |
| --- | --- | --- |
| 1. Launch | — | Two API and two worker tasks, one Aurora writer and reader, one queue |
| 2. Growth | Worker CPU or provider pools saturate; writer CPU above about 50% at peak | More workers; route each environment's traffic to a subset of workers (queue per shard) so APNs pools stay warm and bounded; larger writer; dashboard reads on the reader; batch accept |
| 3. Cells | One writer cannot hold peak, or one tenant needs isolation | Several independent stacks (database, queue, workers) in one region, each owning a set of environments. Every tenant row already carries `environment_id` and nothing joins across environments in the hot path (spec section 2), so moving an environment is moving rows with one key. A thin global directory maps environment to cell |
| 4. Regions | EU data residency or regional latency demand | A cell in a second region, with tenants pinned to it |

### Failure behaviour

| Failure | What happens | Customer impact |
| --- | --- | --- |
| One AZ lost | ALB and tasks fail over to the other AZ; Aurora promotes the reader if the writer was there | Under a minute of errors on in-flight requests; no accepted work lost |
| Database failover | API returns 503 with `Retry-After`, never a false 202; workers pause claims and record from memory where possible; the expired-lease sweep recovers the rest | Sends rejected for up to about 30 s (Aurora); retries with the same idempotency key succeed |
| SQS unavailable | Accept still commits and returns 202; hints fail; the lost-hint sweep re-sends when SQS returns | Dispatch delayed, nothing lost |
| APNs or FCM degraded | Retryable outcomes back off per provider; deliveries stay pending until their expiry and then end `expired` | Delayed or expired sends, each with an explicit reason |
| KMS unavailable | Cached keys keep working for up to 15 minutes; new credentials cannot be decrypted after that | Sends for cold credentials pause |
| Customer's credential revoked or misconfigured | Credential breaker opens for that app config only | That customer sees a credential error in the dashboard; others unaffected |
| Bad deploy | Circuit breaker rolls back; expand-then-contract migrations keep the old version working | Usually none |
| Region lost | Single region in v1. Restore from cross-region snapshot copies; recovery takes hours, and up to a day of history (the snapshot interval) can be lost | Full outage until restored. Acceptable for v1 only if Jack agrees (A5) |

### Cost at launch

Production, `us-east-1`, on-demand, approximate (October 2026 list prices; storage, data and logs estimated for about 10 million deliveries a month):

| Item | Monthly |
| --- | --- |
| Fargate ARM: 2 × (0.5 vCPU, 1 GB) API and 2 × (1 vCPU, 2 GB) worker | $90 |
| Aurora PostgreSQL: 2 × `db.r7g.large`, storage and I/O | $430 |
| Application Load Balancer, WAF | $40 |
| Regional NAT Gateway (2 AZs) and data | $70 |
| Public IPv4 addresses (ALB) | $10 |
| SQS, KMS, S3, ECR | $10 |
| CloudWatch logs, metrics, alarms, traces | $50 |
| **Production total** | **about $700** |
| Staging (single-AZ, smaller instances, scaled to zero overnight where possible) | about $150 |

The database is more than half the bill, and it is fixed cost: the same setup serves 10 million or several hundred million deliveries a month. Choosing RDS Multi-AZ instead of Aurora would save about $150 to $200 a month (A2). Cost per million deliveries becomes meaningful only at volume and is an M5 deliverable.

## 12. Changes to the spec and the current code

Found while checking the spec and `push-core`/`push-db` against current provider documentation and the decisions above. None is built yet; they are for the engine work to pick up once this document is agreed.

**Provider classification (`push-core/src/classify`)**

1. APNs `403 UnrelatedKeyIdInToken` currently classifies as `credential_invalid`, which would mark a healthy credential failing. It means our pool sent a token on a connection bound to a different key: retry on a new connection, and alert, because it is our bug.
2. APNs `403 BadEnvironmentKeyIdInToken` means an environment-restricted key was used against the other environment. It is a credential-and-environment fault: stop sending that credential to that APNs host, not everywhere.
3. APNs `400 IdleTimeout` currently classifies as `payload_invalid`. It is a connection condition: retry on a fresh connection.
4. Apple says 5xx responses may be retried after 15 minutes. The current policy starts at 2 s. Decide whether to start APNs 5xx retries at a minute or more; for short-TTL sends the trade-off is real.
5. FCM `QUOTA_EXCEEDED` needs a 60 s minimum first delay.
6. APNs `BadDeviceToken` also covers a sandbox token sent to production. The invalidation breaker (spec 9.5) is what stops a misconfigured app from wiping its device list; it should be built in the same milestone as the classifier is wired up, not after.

**Credentials model (`provider_credentials`)**

7. Add the APNs key's allowed environment (`sandbox`, `production` or both, for older keys) and, for topic-specific keys, its bundle ids. Credential verification (spec section 11) must check each environment the key claims, and app config validation must reject a credential that cannot serve the app's bundle id.

**Payload mapping (spec section 8.1)**

8. FCM ignores `collapse_key` on notification messages. Use `android.notification.tag` for duplicate suppression of alerts. For background (data-only) messages, do not set `collapse_key` by default: it would make them collapsible and subject to the per-device collapse throttle.

**Schema (`migrations/0001_init.sql`)**

9. `usage_ledger` has a foreign key to `deliveries`, but the ledger is kept for 13 months and delivery rows for 30 days (spec D3). The foreign key would block deleting or dropping delivery partitions. Drop the foreign key (the primary key still guarantees one ledger row per delivery) or keep deliveries for 13 months.
10. Partition the delivery-path tables by time from the start (section 6).

**APNs connection behaviour (spec section 8.2)**

11. Spread traffic across at least two connections per active pool, use an uncached DNS lookup per new connection, and send one authenticated request before fanning out on a new connection (section 8).

## 13. Open decisions

These need Jack's call. Each has a recommendation; work continues on the recommended option unless noted.

| # | Question | Recommendation | Blocks |
| --- | --- | --- | --- |
| A1 | Which AWS region? | `us-east-1`, where many customer backends already run (it is Vercel's default function region, for example). Confirm with a one-day test in M1: send sandbox pushes from `us-east-1`, `us-east-2` and `eu-west-2` and compare provider latency. `eu-west-2` (London) is the choice if UK/EU data residency matters more than US customer latency; residency can otherwise come later as a second cell | M3 infrastructure |
| A2 | Aurora PostgreSQL or RDS PostgreSQL Multi-AZ? | Aurora (faster failover, a readable second instance), confirmed by the M2 commit-latency experiment against RDS. About $150 to $200 a month more at launch | M3 infrastructure |
| A3 | Who owns dashboard accounts, and how does the dashboard authenticate to the management API? | A managed auth provider with organisations and roles (WorkOS AuthKit, or Clerk); the engine stores a mapping from the provider's organisation id to ours and verifies the provider's access tokens on every management call. The alternatives are building accounts in the engine (login, invites, MFA, SSO: months of work outside the core) or self-hosted Better Auth in the platform (no vendor, but the platform then owns a second user database) | Management API, `apps/platform` sign-in |
| A4 | OpenTofu or Terraform? | OpenTofu. Low stakes: the code is portable | M3 infrastructure |
| A5 | Is a single-region outage with a recovery time of hours acceptable for v1? | Yes for launch, with cross-region snapshot copies and a rehearsed restore. A warm standby region roughly doubles the database cost | Disaster recovery plan |
| A6 | Should the engine spec move into the repo? | Yes: `apps/engine/SPEC.md`, reviewed in pull requests like code, so the spec and the code cannot drift. The Claude Doc becomes a pointer | Nothing; housekeeping |

Still open from the spec (section 16), unchanged by this document: D1 (retry an ambiguous send once, recommended), D2 (credit permanently failed deliveries), D3 (retention periods), D4 (suppress alerts to devices with permission denied), D5 (snake_case wire format), D6 (TTL default 24 h, maximum 7 days) and D7 (50-device cap per user).

### Experiments that confirm these decisions

| Experiment | Milestone | Decides |
| --- | --- | --- |
| Provider latency from three candidate regions | M1 | A1 |
| `h2` visibility of `REFUSED_STREAM` and GOAWAY last-stream-id through `hyper` | M1 | Transport design, frequency of `Indeterminate` |
| Android collapse throttle on real devices (notification messages, 30 in a minute to one device) | M1 | Android burst promises, SDK data-message mode priority |
| Accept, claim and record at rising concurrency on Aurora and on RDS Multi-AZ | M2 | A2, instance size |
| Fair queue test: one tenant floods 100,000 hints while others send normally | M2 | Confidence in tenant isolation |
| End-to-end throughput and cost per million deliveries, Fargate versus EC2 Graviton | M5 | Stage 2 compute |

## 14. Sources

Checked in October 2026.

- Apple, [Establishing a token-based connection to APNs](https://developer.apple.com/documentation/usernotifications/establishing-a-token-based-connection-to-apns): team-scoped and topic-specific keys, one team per connection, token refresh window.
- Apple, [Sending notification requests to APNs](https://developer.apple.com/documentation/usernotifications/sending-notification-requests-to-apns): hosts, streams, headers, payload limits, best practices.
- Apple, [Handling notification responses from APNs](https://developer.apple.com/documentation/usernotifications/handling-notification-responses-from-apns): status codes, error reasons, retry guidance.
- Apple Developer Forums, [New features for APNs token authentication now available](https://developer.apple.com/forums/thread/774646) (February 2025).
- Google, [FCM throttling and quotas](https://firebase.google.com/docs/cloud-messaging/throttling-and-quotas): project quota, per-device limits, collapsible throttling.
- Google, [FCM error codes](https://firebase.google.com/docs/cloud-messaging/error-codes) and [Non-collapsible and collapsible messages](https://firebase.google.com/docs/cloud-messaging/customize-messages/collapsible-message-types).
- Knock, push API response-time benchmarks for [APNs](https://knock.app/push-api-benchmarks/compare/apns-vs-onesignal) and [FCM](https://knock.app/push-api-benchmarks/compare/fcm-vs-onesignal).
- AWS, [Amazon SQS fair queues](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-fair-queues.html) and [how they work](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-fair-queues-detailed.html); [SQS pricing](https://aws.amazon.com/sqs/pricing/).
- AWS, [Choosing between Single-AZ, Multi-AZ instance and Multi-AZ DB cluster](https://aws.amazon.com/blogs/database/choose-the-right-amazon-rds-deployment-option-single-az-instance-multi-az-instance-or-multi-az-database-cluster/); [Aurora availability and durability FAQ](https://docs.aws.amazon.com/rds/latest/auroraextendedcontent/aurora-faq-availability-and-durability.html); [RDS for PostgreSQL 18](https://aws.amazon.com/about-aws/whats-new/2025/11/amazon-rds-postgresql-major-version-18).
- AWS, [How Aurora DSQL differs from single-instance PostgreSQL](https://aws.amazon.com/blogs/database/dsql-sql-dialect-how-amazon-aurora-dsql-differs-from-single-instance-postgresql) and the [DSQL migration guide](https://docs.aws.amazon.com/aurora-dsql/latest/userguide/working-with-postgresql-compatibility-migration-guide.html).
- PlanetScale, [Postgres architecture](https://planetscale.com/docs/postgres/postgres-architecture) and [pricing](https://planetscale.com/pricing).
- AWS, [Fargate pricing](https://aws.amazon.com/fargate/pricing/); [App Runner availability change](https://docs.aws.amazon.com/apprunner/latest/dg/apprunner-availability-change.html); [Regional and zonal NAT gateways](https://repost.aws/articles/ARTxK2GTAdQZm5zpghxfwE9A/aws-regional-vs-zonal-nat-gateways-when-to-use-each); [KMS request quotas](https://docs.aws.amazon.com/kms/latest/developerguide/requests-per-second.html).
- Instance prices from [Bytebase's RDS and Aurora price tables](https://www.bytebase.com/dbcost/rds/instance/db.m7g.large/); confirm with the AWS Pricing Calculator before committing a budget.
