//! Accept a send (spec 6.1). One transaction: idempotency, recipient resolution,
//! then the notification, its encrypted payload and one pending delivery per device.
//! The caller validates per-platform payload sizes between resolving and inserting.

use push_core::Timestamp;
use push_core::ids::{AppId, DeliveryId, EnvironmentId, InstallationId, NotificationId, TemplateId};
use push_core::model::{Kind, MAX_INSTALLATIONS_PER_SEND, Priority, Provider, Target};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{Db, Result, kind_str, parse_provider, priority_str, provider_str, to_odt};

/// How long an idempotency key holds (spec G2).
const IDEMPOTENCY_WINDOW: &str = "24 hours";

#[derive(Clone, Debug)]
pub struct Idempotency {
    pub key: String,
    /// sha256 of the canonical request body.
    pub request_hash: Vec<u8>,
}

pub enum AcceptStart<'a> {
    /// The key was new (or absent): carry on inside this transaction.
    Fresh(AcceptTx<'a>),
    /// Same key, same request: return the original notification.
    Replayed(NotificationId),
    /// Same key, different request: 409.
    KeyConflict,
}

pub struct AcceptTx<'a> {
    tx: Transaction<'a, Postgres>,
    environment_id: EnvironmentId,
    notification_id: NotificationId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recipient {
    pub installation_id: InstallationId,
    pub app_id: AppId,
    pub provider: Provider,
}

#[derive(Clone, Debug)]
pub struct NewNotification {
    pub target: Target,
    pub kind: Kind,
    pub priority: Priority,
    pub collapse_key: Option<String>,
    pub expires_at: Timestamp,
    pub template_version_id: Option<TemplateId>,
    pub is_test: bool,
    /// Resolved content, encrypted under the environment data key.
    pub content_ct: Vec<u8>,
    pub purge_after: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    pub notification_id: NotificationId,
    /// (delivery, provider) for each pending delivery, for the post-commit SQS hints.
    pub deliveries: Vec<(DeliveryId, Provider)>,
}

impl Db {
    /// Open the accept transaction and settle idempotency first, so a concurrent request
    /// with the same key waits on our row and then sees the committed result.
    pub async fn begin_accept(
        &self,
        environment_id: EnvironmentId,
        idempotency: Option<&Idempotency>,
    ) -> Result<AcceptStart<'static>> {
        let mut tx = self.pool.begin().await?;
        let notification_id = NotificationId::generate();

        if let Some(idem) = idempotency {
            sqlx::query("delete from idempotency_keys where environment_id = $1 and key = $2 and expires_at < now()")
                .bind(environment_id.as_uuid())
                .bind(&idem.key)
                .execute(&mut *tx)
                .await?;
            let inserted = sqlx::query(&format!(
                "insert into idempotency_keys (environment_id, key, request_hash, notification_id, expires_at)
                 values ($1, $2, $3, $4, now() + interval '{IDEMPOTENCY_WINDOW}')
                 on conflict do nothing"
            ))
            .bind(environment_id.as_uuid())
            .bind(&idem.key)
            .bind(&idem.request_hash)
            .bind(notification_id.as_uuid())
            .execute(&mut *tx)
            .await?
            .rows_affected();

            if inserted == 0 {
                let row = sqlx::query(
                    "select request_hash, notification_id from idempotency_keys
                      where environment_id = $1 and key = $2",
                )
                .bind(environment_id.as_uuid())
                .bind(&idem.key)
                .fetch_one(&mut *tx)
                .await?;
                tx.rollback().await?;
                let hash: Vec<u8> = row.try_get("request_hash")?;
                let original: Uuid = row.try_get("notification_id")?;
                return Ok(if hash == idem.request_hash {
                    AcceptStart::Replayed(NotificationId::from_uuid(original))
                } else {
                    AcceptStart::KeyConflict
                });
            }
        }

        Ok(AcceptStart::Fresh(AcceptTx {
            tx,
            environment_id,
            notification_id,
        }))
    }
}

impl AcceptTx<'_> {
    pub fn notification_id(&self) -> NotificationId {
        self.notification_id
    }

    /// Active, unpaused installations for the target, most recently seen first, capped
    /// at `MAX_INSTALLATIONS_PER_SEND`.
    pub async fn resolve(&mut self, target: &Target) -> Result<Vec<Recipient>> {
        const SELECT: &str = "select i.id, i.app_id, r.provider
               from installations i
               join push_registrations r on r.installation_id = i.id and r.state = 'active'
               join app_configs ac on ac.environment_id = i.environment_id and ac.app_id = i.app_id
                                  and ac.paused_at is null";
        let limit = i64::try_from(MAX_INSTALLATIONS_PER_SEND).unwrap_or(i64::MAX);
        let rows = match target {
            Target::User(user) => {
                sqlx::query(&format!(
                    "{SELECT}
                     join user_bindings b on b.installation_id = i.id and b.unbound_at is null
                    where b.environment_id = $1 and b.external_user_id = $2 and i.deleted_at is null
                    order by i.last_seen_at desc
                    limit $3"
                ))
                .bind(self.environment_id.as_uuid())
                .bind(user.as_str())
                .bind(limit)
                .fetch_all(&mut *self.tx)
                .await?
            }
            Target::Installation(id) => {
                sqlx::query(&format!(
                    "{SELECT}
                    where i.environment_id = $1 and i.id = $2 and i.deleted_at is null"
                ))
                .bind(self.environment_id.as_uuid())
                .bind(id.as_uuid())
                .fetch_all(&mut *self.tx)
                .await?
            }
        };

        rows.iter()
            .map(|r| {
                Ok(Recipient {
                    installation_id: InstallationId::from_uuid(r.try_get("id")?),
                    app_id: AppId::from_uuid(r.try_get("app_id")?),
                    provider: parse_provider(r.try_get("provider")?)?,
                })
            })
            .collect()
    }

    /// Insert the notification, payload and deliveries, then commit. Only after this
    /// returns may the API answer 202 (invariant I1).
    pub async fn commit(mut self, n: &NewNotification, recipients: &[Recipient]) -> Result<Accepted> {
        let total = i16::try_from(recipients.len()).unwrap_or(i16::MAX);
        let (target_kind, target_value) = match &n.target {
            Target::User(u) => ("user", u.as_str().to_owned()),
            Target::Installation(i) => ("installation", i.to_string()),
        };
        let collapse_key = n
            .collapse_key
            .clone()
            .unwrap_or_else(|| self.notification_id.to_string());
        let empty = recipients.is_empty();

        sqlx::query(
            "insert into notifications
               (id, environment_id, target_kind, target_value, kind, priority, template_version_id,
                collapse_key, expires_at, status, outcome, is_test, deliveries_total, deliveries_open,
                completed_at)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9,
                     case when $10 then 'completed' else 'processing' end,
                     case when $10 then 'no_eligible_devices' end,
                     $11, $12, $12,
                     case when $10 then now() end)",
        )
        .bind(self.notification_id.as_uuid())
        .bind(self.environment_id.as_uuid())
        .bind(target_kind)
        .bind(target_value)
        .bind(kind_str(n.kind))
        .bind(priority_str(n.priority))
        .bind(n.template_version_id.map(|t| t.as_uuid()))
        .bind(collapse_key)
        .bind(to_odt(n.expires_at))
        .bind(empty)
        .bind(n.is_test)
        .bind(total)
        .execute(&mut *self.tx)
        .await?;

        sqlx::query(
            "insert into notification_payloads (notification_id, environment_id, content_ct, purge_after)
             values ($1, $2, $3, $4)",
        )
        .bind(self.notification_id.as_uuid())
        .bind(self.environment_id.as_uuid())
        .bind(&n.content_ct)
        .bind(to_odt(n.purge_after))
        .execute(&mut *self.tx)
        .await?;

        let deliveries: Vec<(DeliveryId, Provider)> = recipients
            .iter()
            .map(|r| (DeliveryId::generate(), r.provider))
            .collect();
        if !empty {
            let ids: Vec<Uuid> = deliveries.iter().map(|(d, _)| d.as_uuid()).collect();
            let installs: Vec<Uuid> = recipients.iter().map(|r| r.installation_id.as_uuid()).collect();
            let apps: Vec<Uuid> = recipients.iter().map(|r| r.app_id.as_uuid()).collect();
            let providers: Vec<&str> = recipients.iter().map(|r| provider_str(r.provider)).collect();
            sqlx::query(
                "insert into deliveries
                   (notification_id, id, environment_id, installation_id, app_id, provider,
                    next_attempt_at, expires_at)
                 select $1, d.id, $2, d.installation_id, d.app_id, d.provider, now(), $7
                   from unnest($3::uuid[], $4::uuid[], $5::uuid[], $6::text[])
                        as d(id, installation_id, app_id, provider)",
            )
            .bind(self.notification_id.as_uuid())
            .bind(self.environment_id.as_uuid())
            .bind(&ids)
            .bind(&installs)
            .bind(&apps)
            .bind(&providers)
            .bind(to_odt(n.expires_at))
            .execute(&mut *self.tx)
            .await?;
        }

        self.tx.commit().await?;
        Ok(Accepted {
            notification_id: self.notification_id,
            deliveries,
        })
    }

    /// Abandon the send (validation failed after resolving). Releases the idempotency
    /// key so the customer can retry with a corrected request.
    pub async fn rollback(self) -> Result<()> {
        self.tx.rollback().await?;
        Ok(())
    }
}
