//! Recovery sweeps (spec section 10). Every worker runs them; `for update skip locked`
//! lets them share the work without a leader, in small batches.

use push_core::Timestamp;
use push_core::ids::{CredentialId, DeliveryId, EnvironmentId, NotificationId, RegistrationId};
use sqlx::Row;
use uuid::Uuid;

use crate::{Db, Result, close_deliveries, from_odt};

/// A pending delivery that needs an SQS hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DueHint {
    pub environment_id: EnvironmentId,
    pub notification_id: NotificationId,
    pub delivery_id: DeliveryId,
}

/// A claimed delivery whose worker stopped renewing it, with its in-flight attempt.
#[derive(Clone, Copy, Debug)]
pub struct ExpiredLease {
    pub notification_id: NotificationId,
    pub delivery_id: DeliveryId,
    pub lease_epoch: i64,
    pub attempt_no: i16,
    pub registration_id: RegistrationId,
    pub credential_id: Option<CredentialId>,
    pub budget_used: u16,
    pub ambiguous_used: u16,
    pub expires_at: Timestamp,
}

impl Db {
    /// Record that hints were sent, so the lost-hint sweep leaves these rows alone.
    pub async fn mark_enqueued(&self, deliveries: &[(NotificationId, DeliveryId)]) -> Result<()> {
        let n: Vec<Uuid> = deliveries.iter().map(|(n, _)| n.as_uuid()).collect();
        let d: Vec<Uuid> = deliveries.iter().map(|(_, d)| d.as_uuid()).collect();
        sqlx::query(
            "update deliveries x set enqueued_at = now()
               from unnest($1::uuid[], $2::uuid[]) as k(n, d)
              where x.notification_id = k.n and x.id = k.d and x.state = 'pending'",
        )
        .bind(&n)
        .bind(&d)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lost hints: due pending deliveries with no hint sent in the last `grace` seconds.
    /// Marks them enqueued and returns them for the caller to send.
    pub async fn take_due_hints(&self, grace_secs: i32, limit: i64) -> Result<Vec<DueHint>> {
        let rows = sqlx::query(
            "with due as (
               select notification_id, id from deliveries
                where state = 'pending'
                  and next_attempt_at <= now() - make_interval(secs => $1)
                  and (enqueued_at is null or enqueued_at < now() - make_interval(secs => $1))
                order by next_attempt_at
                limit $2
                for update skip locked
             )
             update deliveries d set enqueued_at = now()
               from due
              where d.notification_id = due.notification_id and d.id = due.id
             returning d.environment_id, d.notification_id, d.id",
        )
        .bind(grace_secs)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(DueHint {
                    environment_id: EnvironmentId::from_uuid(r.try_get("environment_id")?),
                    notification_id: NotificationId::from_uuid(r.try_get("notification_id")?),
                    delivery_id: DeliveryId::from_uuid(r.try_get("id")?),
                })
            })
            .collect()
    }

    /// Finish pending deliveries past their deadline as `expired` (invariant I9).
    pub async fn expire_pending(&self, limit: i64) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query(
            "with due as (
               select notification_id, id from deliveries
                where state = 'pending' and expires_at <= now()
                limit $1
                for update skip locked
             )
             update deliveries d
                set state = 'expired', reason = 'expired', finished_at = now()
               from due
              where d.notification_id = due.notification_id and d.id = due.id
             returning d.notification_id",
        )
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        let finished: Vec<Uuid> = rows
            .iter()
            .map(|r| r.try_get("notification_id"))
            .collect::<std::result::Result<_, sqlx::Error>>()?;
        close_deliveries(&mut tx, &finished).await?;
        tx.commit().await?;
        Ok(finished.len() as u64)
    }

    /// Claimed deliveries whose lease ran out. The caller treats the in-flight attempt
    /// as `Indeterminate`, runs `decide()`, and records it with `abandoned = true`;
    /// the epoch fence makes a race with a slow original worker harmless.
    pub async fn expired_leases(&self, limit: i64) -> Result<Vec<ExpiredLease>> {
        let rows = sqlx::query(
            "select d.notification_id, d.id, d.lease_epoch, d.budget_used, d.ambiguous_count, d.expires_at,
                    a.attempt_no, a.registration_id, ac.credential_id
               from deliveries d
               join delivery_attempts a on a.notification_id = d.notification_id
                                       and a.delivery_id = d.id and a.state = 'in_flight'
               join app_configs ac on ac.environment_id = d.environment_id and ac.app_id = d.app_id
              where d.state = 'claimed' and d.lease_until < now()
              order by d.lease_until
              limit $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(ExpiredLease {
                    notification_id: NotificationId::from_uuid(r.try_get("notification_id")?),
                    delivery_id: DeliveryId::from_uuid(r.try_get("id")?),
                    lease_epoch: r.try_get("lease_epoch")?,
                    attempt_no: r.try_get("attempt_no")?,
                    registration_id: RegistrationId::from_uuid(r.try_get("registration_id")?),
                    credential_id: r
                        .try_get::<Option<Uuid>, _>("credential_id")?
                        .map(CredentialId::from_uuid),
                    budget_used: u16::try_from(r.try_get::<i16, _>("budget_used")?).unwrap_or(0),
                    ambiguous_used: u16::try_from(r.try_get::<i16, _>("ambiguous_count")?).unwrap_or(0),
                    expires_at: from_odt(r.try_get("expires_at")?),
                })
            })
            .collect()
    }
}
