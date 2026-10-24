# engine

The push delivery engine: one Cargo workspace for every Rust crate, deployed to AWS.

| Path | What it is |
| --- | --- |
| `crates/push-core` | Domain types and pure decision functions: ids, delivery states, provider response classification, retry policy and `decide()`. No I/O. |
| `crates/push-db` | PostgreSQL transactions: accept a send, claim a delivery, record an attempt, and the recovery sweeps. Integration tests run against a real database. |
| `migrations/` | PostgreSQL schema. Constraints carry the invariants the database can enforce. |
| `tests/sql/` | Schema invariant checks, run against a freshly migrated database. |

`push-providers`, `push-api` and `push-worker` join the workspace under `crates/` as the engine spec's milestones land.

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
