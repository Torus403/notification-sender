# engine

The core of the product: the public API and the push delivery engine, in one Cargo workspace, deployed to AWS.

## Purpose

Accept a customer's request to notify a user, deliver it to each of that user's devices through APNs and FCM, and record an exact outcome for every device. Everything a customer depends on for correctness lives here.

## Scope

In scope:

- **Server API** (secret key): send a notification, read its status and per-device deliveries, cancel, manage installations and users, import tokens.
- **Device API** (publishable key, then installation token): register a device, update its token and permission, bind it to a user, report interactions.
- **Management API** (dashboard session): projects, environments, apps, provider credentials, API keys, templates, test sends, history. Used by `apps/platform`.
- **Delivery**: recipient resolution, APNs and FCM transports, retries, recovery sweeps, provider credential storage and verification, metering.
- `spec/openapi.json`, generated from `push-api`.

Out of scope: dashboard sign-in and billing plans (`apps/platform`), the websites, SDKs and anything that runs on a device (see `PLAN.md`, Later).

## Documents

- [`ARCHITECTURE.md`](ARCHITECTURE.md): how the engine runs on AWS, how it scales, fails and what it costs, and the open decisions.
- [Push Delivery Engine Specification v0.1](https://claude.ai/artifact/9xNDseoAYnaWxst78tQsqV): the delivery protocol: data model, state machines, transactions, queue contract, provider error tables and invariants.

## Layout

| Path | What it is |
| --- | --- |
| `crates/push-core` | Domain types and pure decision functions: ids, delivery states, provider response classification, retry policy and `decide()`. No I/O. |
| `crates/push-db` | PostgreSQL transactions: accept a send, claim a delivery, record an attempt, and the recovery sweeps. Integration tests run against a real database. |
| `migrations/` | PostgreSQL schema. Constraints carry the invariants the database can enforce. |
| `tests/sql/` | Schema invariant checks, run against a freshly migrated database. |

`push-providers` (the APNs and FCM transports), `push-api` and `push-worker` join the workspace under `crates/` as the milestones in `PLAN.md` land.

## Running checks

Run these from `apps/engine`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
# push-db's tests need a Postgres 16+ server; each test gets its own database
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test --workspace

# Needs a Postgres 16+ database
psql -v ON_ERROR_STOP=1 -f migrations/0001_init.sql -f tests/sql/invariants.sql
```
