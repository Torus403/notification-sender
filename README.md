# notification-sender

A managed push notification service for iOS and Android: one API for your backend, direct delivery through APNs and FCM, and honest per-device results.

## Layout

| Path | What it is |
| --- | --- |
| `crates/push-core` | Domain types and pure decision functions: ids, delivery states, provider response classification, retry policy and `decide()`. No I/O. |
| `migrations/` | PostgreSQL schema. Constraints carry the invariants the database can enforce. |
| `tests/sql/` | Schema invariant checks, run against a freshly migrated database. |

`push-db`, `push-providers`, `push-api` and `push-worker` join the workspace as the engine spec's milestones land.

## Running checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Needs a Postgres 16+ database
psql -v ON_ERROR_STOP=1 -f migrations/0001_init.sql -f tests/sql/invariants.sql
```
