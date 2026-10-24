//! Record one attempt's outcome and apply `decide()` (spec 6.4). Fenced by the lease
//! epoch, so a worker that lost its lease cannot overwrite its successor (invariant I6).

use push_core::delivery::Reason;
use push_core::ids::{CredentialId, DeliveryId, NotificationId, RegistrationId};
use push_core::{Decision, SideEffect, SubmissionOutcome, Terminal};

use crate::{Db, Result, close_deliveries, to_odt};

/// Identifies the attempt being recorded; taken from the claim.
#[derive(Clone, Copy, Debug)]
pub struct AttemptRef {
    pub notification_id: NotificationId,
    pub delivery_id: DeliveryId,
    pub attempt_no: i16,
    pub lease_epoch: i64,
    pub registration_id: RegistrationId,
    pub credential_id: Option<CredentialId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recorded {
    Applied,
    /// Another worker owns the delivery now. The attempt row was still written.
    Fenced,
}

struct AttemptColumns<'a> {
    state: &'static str,
    http_status: Option<i16>,
    provider_reason: Option<&'a str>,
    normalized_reason: Option<&'static str>,
    provider_message_id: Option<&'a str>,
}

fn attempt_columns(outcome: &SubmissionOutcome, abandoned: bool) -> AttemptColumns<'_> {
    let http = |s: u16| i16::try_from(s).ok();
    match outcome {
        _ if abandoned => AttemptColumns {
            state: "abandoned",
            http_status: None,
            provider_reason: None,
            normalized_reason: Some(Reason::TransportAmbiguous.as_str()),
            provider_message_id: None,
        },
        SubmissionOutcome::Accepted { provider_message_id } => AttemptColumns {
            state: "accepted",
            http_status: Some(200),
            provider_reason: None,
            normalized_reason: None,
            provider_message_id: provider_message_id.as_deref(),
        },
        SubmissionOutcome::Rejected { error, .. } => AttemptColumns {
            state: "rejected",
            http_status: http(error.http_status),
            provider_reason: Some(&error.raw_reason),
            normalized_reason: Some(error.reason.as_str()),
            provider_message_id: None,
        },
        SubmissionOutcome::Retryable { error, .. } => AttemptColumns {
            state: "retryable",
            http_status: http(error.http_status),
            provider_reason: Some(&error.raw_reason),
            normalized_reason: Some(error.reason.as_str()),
            provider_message_id: None,
        },
        SubmissionOutcome::NotSent { .. } => AttemptColumns {
            state: "not_sent",
            http_status: None,
            provider_reason: None,
            normalized_reason: Some(Reason::TransportNotSent.as_str()),
            provider_message_id: None,
        },
        SubmissionOutcome::Indeterminate { .. } => AttemptColumns {
            state: "indeterminate",
            http_status: None,
            provider_reason: None,
            normalized_reason: Some(Reason::TransportAmbiguous.as_str()),
            provider_message_id: None,
        },
    }
}

impl Db {
    /// Persist the attempt, the delivery's next state and any side effects in one
    /// transaction. `abandoned` marks an attempt recovered from an expired lease.
    pub async fn record(
        &self,
        attempt: &AttemptRef,
        outcome: &SubmissionOutcome,
        latency_us: Option<i32>,
        abandoned: bool,
        decision: &Decision,
        effects: &[SideEffect],
    ) -> Result<Recorded> {
        let mut tx = self.pool.begin().await?;
        let cols = attempt_columns(outcome, abandoned);

        sqlx::query(
            "update delivery_attempts
                set state = $4, http_status = $5, provider_reason = $6, normalized_reason = $7,
                    provider_message_id = $8, latency_us = $9, finished_at = now()
              where notification_id = $1 and delivery_id = $2 and attempt_no = $3 and state = 'in_flight'",
        )
        .bind(attempt.notification_id.as_uuid())
        .bind(attempt.delivery_id.as_uuid())
        .bind(attempt.attempt_no)
        .bind(cols.state)
        .bind(cols.http_status)
        .bind(cols.provider_reason)
        .bind(cols.normalized_reason)
        .bind(cols.provider_message_id)
        .bind(latency_us)
        .execute(&mut *tx)
        .await?;

        let updated = match decision {
            Decision::Finish(terminal) => {
                let (state, reason, message_id) = match terminal {
                    Terminal::ProviderAccepted { provider_message_id } => {
                        ("provider_accepted", None, provider_message_id.as_deref())
                    }
                    Terminal::Failed { reason } => ("failed", Some(reason.as_str()), None),
                    Terminal::Unknown { reason } => ("unknown", Some(reason.as_str()), None),
                    Terminal::Expired => ("expired", Some(Reason::Expired.as_str()), None),
                };
                sqlx::query(
                    "update deliveries
                        set state = $4, reason = $5, provider_message_id = $6, finished_at = now(),
                            lease_until = null, lease_owner = null
                      where notification_id = $1 and id = $2 and lease_epoch = $3 and state = 'claimed'",
                )
                .bind(attempt.notification_id.as_uuid())
                .bind(attempt.delivery_id.as_uuid())
                .bind(attempt.lease_epoch)
                .bind(state)
                .bind(reason)
                .bind(message_id)
                .execute(&mut *tx)
                .await?
                .rows_affected()
            }
            Decision::RetryAt {
                at,
                reason,
                uses_budget,
                ambiguous,
            } => sqlx::query(
                "update deliveries
                    set state = 'pending', reason = $4, next_attempt_at = $5,
                        budget_used = budget_used + $6, ambiguous_count = ambiguous_count + $7,
                        lease_until = null, lease_owner = null, enqueued_at = null
                  where notification_id = $1 and id = $2 and lease_epoch = $3 and state = 'claimed'",
            )
            .bind(attempt.notification_id.as_uuid())
            .bind(attempt.delivery_id.as_uuid())
            .bind(attempt.lease_epoch)
            .bind(reason.as_str())
            .bind(to_odt(*at))
            .bind(i16::from(*uses_budget))
            .bind(i16::from(*ambiguous))
            .execute(&mut *tx)
            .await?
            .rows_affected(),
        };

        if updated == 0 {
            tx.commit().await?;
            return Ok(Recorded::Fenced);
        }

        if matches!(decision, Decision::Finish(_)) {
            close_deliveries(&mut tx, &[attempt.notification_id.as_uuid()]).await?;
        }

        for effect in effects {
            match effect {
                // Only the exact registration used, and only while still active (I11).
                SideEffect::InvalidateRegistration { reason } => {
                    sqlx::query(
                        "update push_registrations
                            set state = 'invalid', ended_reason = $2, ended_at = now()
                          where id = $1 and state = 'active'",
                    )
                    .bind(attempt.registration_id.as_uuid())
                    .bind(reason.as_str())
                    .execute(&mut *tx)
                    .await?;
                }
                SideEffect::FlagCredential { reason } => {
                    if let Some(cred) = attempt.credential_id {
                        sqlx::query(
                            "update provider_credentials
                                set state = 'failing', last_error = $2
                              where id = $1 and state in ('unverified', 'valid')",
                        )
                        .bind(cred.as_uuid())
                        .bind(reason.as_str())
                        .execute(&mut *tx)
                        .await?;
                    }
                }
                // In-memory or alerting effects belong to the worker.
                SideEffect::RefreshAuth | SideEffect::AlertInternalBug { .. } => {}
            }
        }

        tx.commit().await?;
        Ok(Recorded::Applied)
    }
}
