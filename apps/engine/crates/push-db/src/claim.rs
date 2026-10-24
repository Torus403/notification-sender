//! Claim and validate a delivery (spec 6.2). One transaction: take the lease, recheck
//! everything that may have changed since accept, then either finish the delivery
//! without sending or commit to sending by writing the in-flight attempt first.

use std::time::Duration;

use push_core::Timestamp;
use push_core::delivery::Reason;
use push_core::ids::{AppId, CredentialId, DeliveryId, InstallationId, NotificationId, RegistrationId};
use push_core::model::{ApnsEnv, Kind, Priority, Provider};
use sqlx::Row;
use uuid::Uuid;

use crate::{Db, Error, Result, close_deliveries, from_odt, parse_provider};

/// Everything the worker needs to build and send one provider request.
#[derive(Clone, Debug)]
pub struct ClaimedDelivery {
    pub notification_id: NotificationId,
    pub delivery_id: DeliveryId,
    pub installation_id: InstallationId,
    pub app_id: AppId,
    pub provider: Provider,
    pub lease_epoch: i64,
    pub lease_until: Timestamp,
    pub attempt_no: i16,
    pub budget_used: u16,
    pub ambiguous_used: u16,
    pub expires_at: Timestamp,
    pub registration_id: RegistrationId,
    pub token_ct: Vec<u8>,
    pub apns_env: Option<ApnsEnv>,
    pub credential_id: Option<CredentialId>,
    pub kind: Kind,
    pub priority: Priority,
    pub collapse_key: String,
    pub content_ct: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum Claim {
    /// Not claimable now: already claimed, finished, or not yet due. Delete the hint.
    Stale,
    /// Finished without sending (suppressed or expired). Nothing billed.
    Finished { reason: Reason },
    /// Leased, attempt row written, usage recorded: send it.
    Send(Box<ClaimedDelivery>),
}

impl Db {
    pub async fn claim(
        &self,
        notification_id: NotificationId,
        delivery_id: DeliveryId,
        worker_id: &str,
        lease: Duration,
    ) -> Result<Claim> {
        let mut tx = self.pool.begin().await?;
        let lease_ms = i64::try_from(lease.as_millis()).unwrap_or(i64::MAX);

        let row = sqlx::query(
            "with c as (
               update deliveries d
                  set state = 'claimed', lease_epoch = d.lease_epoch + 1, lease_owner = $3,
                      lease_until = now() + make_interval(secs => $4::double precision / 1000),
                      attempt_count = d.attempt_count + 1
                where d.notification_id = $1 and d.id = $2
                  and d.state = 'pending' and d.next_attempt_at <= now()
               returning d.*
             )
             select c.installation_id, c.app_id, c.provider, c.lease_epoch, c.lease_until,
                    c.attempt_count, c.budget_used, c.ambiguous_count, c.expires_at, c.environment_id,
                    c.expires_at <= now() as expired,
                    n.kind, n.priority, n.collapse_key, n.is_test,
                    r.id as registration_id, r.token_ct, r.apns_env,
                    ac.credential_id, ac.paused_at is not null as app_paused,
                    e.paused_at is not null as env_paused,
                    coalesce(tp.disabled_at is not null, false) as template_disabled,
                    i.permission,
                    (n.target_kind <> 'user' or exists (
                       select 1 from user_bindings b
                        where b.installation_id = c.installation_id and b.unbound_at is null
                          and b.external_user_id = n.target_value)) as still_bound,
                    p.content_ct
               from c
               join notifications n on n.id = c.notification_id
               join installations i on i.id = c.installation_id
               join app_configs ac on ac.environment_id = c.environment_id and ac.app_id = c.app_id
               join environments e on e.id = c.environment_id
               join notification_payloads p on p.notification_id = c.notification_id
               left join template_versions tv on tv.id = n.template_version_id
               left join template_publications tp on tp.template_id = tv.template_id
                                                 and tp.environment_id = c.environment_id
               left join push_registrations r on r.installation_id = c.installation_id
                                             and r.state = 'active'",
        )
        .bind(notification_id.as_uuid())
        .bind(delivery_id.as_uuid())
        .bind(worker_id)
        .bind(lease_ms)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(Claim::Stale);
        };

        let kind = match row.try_get::<&str, _>("kind")? {
            "alert" => Kind::Alert,
            "background" => Kind::Background,
            other => {
                return Err(Error::Corrupt {
                    column: "kind",
                    value: other.to_owned(),
                });
            }
        };
        let registration: Option<Uuid> = row.try_get("registration_id")?;
        let permission: &str = row.try_get("permission")?;

        // Order matters only for which reason is shown; any one of them stops the send.
        let suppress = if row.try_get("env_paused")? {
            Some(Reason::EnvironmentPaused)
        } else if row.try_get("app_paused")? {
            Some(Reason::AppPaused)
        } else if row.try_get("template_disabled")? {
            Some(Reason::TemplateDisabled)
        } else if row.try_get("expired")? {
            Some(Reason::Expired)
        } else if registration.is_none() {
            Some(Reason::NoActiveRegistration)
        } else if !row.try_get::<bool, _>("still_bound")? {
            Some(Reason::RecipientUnbound)
        } else if kind == Kind::Alert && permission == "denied" {
            Some(Reason::PermissionDenied)
        } else {
            None
        };

        if let Some(reason) = suppress {
            let state = if reason == Reason::Expired {
                "expired"
            } else {
                "suppressed"
            };
            sqlx::query(
                "update deliveries
                    set state = $3, reason = $4, finished_at = now(), lease_until = null, lease_owner = null
                  where notification_id = $1 and id = $2",
            )
            .bind(notification_id.as_uuid())
            .bind(delivery_id.as_uuid())
            .bind(state)
            .bind(reason.as_str())
            .execute(&mut *tx)
            .await?;
            close_deliveries(&mut tx, &[notification_id.as_uuid()]).await?;
            tx.commit().await?;
            return Ok(Claim::Finished { reason });
        }

        let registration = registration.unwrap_or_default();
        let lease_epoch: i64 = row.try_get("lease_epoch")?;
        let attempt_no: i16 = row.try_get("attempt_count")?;

        // The attempt row commits before any byte goes to a provider (invariant I7).
        sqlx::query(
            "insert into delivery_attempts
               (notification_id, delivery_id, attempt_no, lease_epoch, registration_id, worker_id, started_at)
             values ($1, $2, $3, $4, $5, $6, now())",
        )
        .bind(notification_id.as_uuid())
        .bind(delivery_id.as_uuid())
        .bind(attempt_no)
        .bind(lease_epoch)
        .bind(registration)
        .bind(worker_id)
        .execute(&mut *tx)
        .await?;

        // First attempt bills; later attempts are no-ops (invariant I13).
        if !row.try_get::<bool, _>("is_test")? {
            sqlx::query(
                "insert into usage_ledger (notification_id, delivery_id, environment_id)
                 values ($1, $2, $3) on conflict do nothing",
            )
            .bind(notification_id.as_uuid())
            .bind(delivery_id.as_uuid())
            .bind(row.try_get::<Uuid, _>("environment_id")?)
            .execute(&mut *tx)
            .await?;
        }

        let claimed = ClaimedDelivery {
            notification_id,
            delivery_id,
            installation_id: InstallationId::from_uuid(row.try_get("installation_id")?),
            app_id: AppId::from_uuid(row.try_get("app_id")?),
            provider: parse_provider(row.try_get("provider")?)?,
            lease_epoch,
            lease_until: from_odt(row.try_get("lease_until")?),
            attempt_no,
            budget_used: u16::try_from(row.try_get::<i16, _>("budget_used")?).unwrap_or(0),
            ambiguous_used: u16::try_from(row.try_get::<i16, _>("ambiguous_count")?).unwrap_or(0),
            expires_at: from_odt(row.try_get("expires_at")?),
            registration_id: RegistrationId::from_uuid(registration),
            token_ct: row.try_get("token_ct")?,
            apns_env: match row.try_get::<Option<&str>, _>("apns_env")? {
                None => None,
                Some("sandbox") => Some(ApnsEnv::Sandbox),
                Some("production") => Some(ApnsEnv::Production),
                Some(other) => {
                    return Err(Error::Corrupt {
                        column: "apns_env",
                        value: other.to_owned(),
                    });
                }
            },
            credential_id: row
                .try_get::<Option<Uuid>, _>("credential_id")?
                .map(CredentialId::from_uuid),
            kind,
            priority: match row.try_get::<&str, _>("priority")? {
                "normal" => Priority::Normal,
                _ => Priority::High,
            },
            collapse_key: row.try_get("collapse_key")?,
            content_ct: row.try_get("content_ct")?,
        };

        tx.commit().await?;
        Ok(Claim::Send(Box::new(claimed)))
    }
}
