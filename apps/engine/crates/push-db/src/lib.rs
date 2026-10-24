//! PostgreSQL transactions for the push delivery engine (spec section 6).
//!
//! Each public operation is one short transaction over rows of one environment:
//! accept a send, claim a delivery, record an attempt's outcome, and the recovery
//! sweeps. The database is the source of truth; SQS only carries hints that point at
//! rows here. Secrets and payloads arrive already encrypted: this crate stores bytes.

mod accept;
mod claim;
mod record;
mod sweep;

use push_core::Timestamp;
use push_core::model::{Kind, Priority, Provider};
use sqlx::PgPool;
use time::OffsetDateTime;

pub use accept::{AcceptStart, AcceptTx, Accepted, Idempotency, NewNotification, Recipient};
pub use claim::{Claim, ClaimedDelivery};
pub use record::{AttemptRef, Recorded};
pub use sweep::{DueHint, ExpiredLease};

/// Migrations embedded from `apps/engine/migrations`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error("unexpected value in column {column}: {value}")]
    Corrupt { column: &'static str, value: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

pub(crate) fn to_odt(t: Timestamp) -> OffsetDateTime {
    let nanos = i128::from(t.unix_millis()) * 1_000_000;
    OffsetDateTime::from_unix_timestamp_nanos(nanos).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

pub(crate) fn from_odt(t: OffsetDateTime) -> Timestamp {
    let ms = t.unix_timestamp_nanos() / 1_000_000;
    Timestamp::from_unix_millis(i64::try_from(ms).unwrap_or(i64::MAX))
}

pub(crate) fn provider_str(p: Provider) -> &'static str {
    match p {
        Provider::Apns => "apns",
        Provider::Fcm => "fcm",
    }
}

pub(crate) fn parse_provider(s: &str) -> Result<Provider> {
    match s {
        "apns" => Ok(Provider::Apns),
        "fcm" => Ok(Provider::Fcm),
        other => Err(Error::Corrupt {
            column: "provider",
            value: other.to_owned(),
        }),
    }
}

pub(crate) fn kind_str(k: Kind) -> &'static str {
    match k {
        Kind::Alert => "alert",
        Kind::Background => "background",
    }
}

pub(crate) fn priority_str(p: Priority) -> &'static str {
    match p {
        Priority::High => "high",
        Priority::Normal => "normal",
    }
}

/// Decrement `deliveries_open` for notifications whose deliveries just finished, and
/// complete any that reach zero. `finished` holds one notification id per finished
/// delivery (repeats allowed).
pub(crate) async fn close_deliveries(tx: &mut sqlx::PgConnection, finished: &[uuid::Uuid]) -> Result<()> {
    if finished.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "with c as (select id, count(*)::smallint as n from unnest($1::uuid[]) as id group by id)
         update notifications n
            set deliveries_open = n.deliveries_open - c.n,
                status = case when n.deliveries_open - c.n = 0 and n.status = 'processing'
                              then 'completed' else n.status end,
                completed_at = case when n.deliveries_open - c.n = 0 and n.status = 'processing'
                                    then now() else n.completed_at end
           from c
          where n.id = c.id",
    )
    .bind(finished)
    .execute(tx)
    .await?;
    Ok(())
}
