//! What happens to a delivery after one submission (spec 9.4).
//!
//! `decide` is pure: the worker passes in the delivery's history, the outcome, the
//! time and a jitter sample, and persists whatever comes back inside the record
//! transaction (spec 6.4).

use std::time::Duration;

use crate::delivery::{DeliveryState, Reason};
use crate::outcome::{FaultScope, SubmissionOutcome};
use crate::time::Timestamp;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub base: Duration,
    pub cap: Duration,
    /// Floor on every retry delay, so a run of zero-jitter samples cannot spin.
    pub min_delay: Duration,
    /// Attempts that use the budget. Connection-setup failures do not.
    pub max_attempts: u16,
    /// A retry must land at least this long before `expires_at`.
    pub expiry_margin: Duration,
    /// How many ambiguous sends we retry (decision D1: one).
    pub ambiguous_retries: u16,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(2),
            cap: Duration::from_secs(300),
            min_delay: Duration::from_millis(250),
            max_attempts: 6,
            expiry_margin: Duration::from_secs(1),
            ambiguous_retries: 1,
        }
    }
}

impl RetryPolicy {
    /// Full-jitter exponential backoff. `n` is the number of budgeted attempts made so
    /// far (at least 1); `jitter` is a uniform sample in `[0, 1]`.
    pub fn backoff(&self, n: u16, jitter: f64) -> Duration {
        let exp = u32::from(n.saturating_sub(1)).min(30);
        let ceiling = self.base.saturating_mul(1u32 << exp).min(self.cap);
        ceiling.mul_f64(jitter.clamp(0.0, 1.0)).max(self.min_delay)
    }
}

/// The parts of a delivery's history `decide` needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeliveryView {
    /// Budgeted attempts before the one that just finished.
    pub budget_used: u16,
    /// Ambiguous sends already retried.
    pub ambiguous_used: u16,
    /// Copied from the notification at accept and never changed.
    pub expires_at: Timestamp,
}

/// Terminal states reachable from a claimed delivery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Terminal {
    ProviderAccepted { provider_message_id: Option<String> },
    Failed { reason: Reason },
    Unknown { reason: Reason },
    Expired,
}

impl From<Terminal> for DeliveryState {
    fn from(t: Terminal) -> Self {
        match t {
            Terminal::ProviderAccepted { provider_message_id } => {
                DeliveryState::ProviderAccepted { provider_message_id }
            }
            Terminal::Failed { reason } => DeliveryState::Failed { reason },
            Terminal::Unknown { reason } => DeliveryState::Unknown { reason },
            Terminal::Expired => DeliveryState::Expired,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Finish(Terminal),
    RetryAt {
        at: Timestamp,
        reason: Reason,
        /// Persist `budget_used + 1`.
        uses_budget: bool,
        /// Persist `ambiguous_used + 1`.
        ambiguous: bool,
    },
}

/// Changes outside the delivery row, each applied conditionally by the worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SideEffect {
    /// Invalidate exactly the registration this attempt used, if still active.
    InvalidateRegistration { reason: Reason },
    /// Mark the credential `failing` and feed the credential breaker.
    FlagCredential { reason: Reason },
    /// Mint a fresh provider auth token before the next attempt.
    RefreshAuth,
    /// The provider rejected something our validation should have caught.
    AlertInternalBug { reason: Reason },
}

pub fn decide(
    view: &DeliveryView,
    outcome: &SubmissionOutcome,
    now: Timestamp,
    policy: &RetryPolicy,
    jitter: f64,
) -> (Decision, Vec<SideEffect>) {
    let mut effects = Vec::new();
    let latest_retry = view.expires_at.saturating_sub(policy.expiry_margin);
    let budget_after = view.budget_used.saturating_add(1);

    // Shared by every retryable branch: honour the budget and the deadline.
    let retry = |reason, uses_budget: bool, ambiguous: bool, floor: Option<Duration>| {
        let n = if uses_budget {
            budget_after
        } else {
            view.budget_used.max(1)
        };
        let delay = policy.backoff(n, jitter).max(floor.unwrap_or_default());
        let at = now.saturating_add(delay);
        let exhausted = uses_budget && budget_after >= policy.max_attempts;
        if exhausted || at >= latest_retry {
            None
        } else {
            Some(Decision::RetryAt {
                at,
                reason,
                uses_budget,
                ambiguous,
            })
        }
    };

    let decision = match outcome {
        SubmissionOutcome::Accepted { provider_message_id } => Decision::Finish(Terminal::ProviderAccepted {
            provider_message_id: provider_message_id.clone(),
        }),

        SubmissionOutcome::Rejected { error, scope } => {
            effects.push(match scope {
                FaultScope::Registration => SideEffect::InvalidateRegistration { reason: error.reason },
                FaultScope::Credential => SideEffect::FlagCredential { reason: error.reason },
                FaultScope::Payload => SideEffect::AlertInternalBug { reason: error.reason },
            });
            Decision::Finish(Terminal::Failed { reason: error.reason })
        }

        SubmissionOutcome::Retryable {
            error,
            retry_after,
            refresh_auth,
        } => {
            if *refresh_auth {
                effects.push(SideEffect::RefreshAuth);
            }
            if error.reason == Reason::InternalProviderMisuse {
                effects.push(SideEffect::AlertInternalBug { reason: error.reason });
            }
            let give_up = if budget_after >= policy.max_attempts {
                Terminal::Failed { reason: error.reason }
            } else {
                Terminal::Expired
            };
            retry(error.reason, true, false, *retry_after).unwrap_or(Decision::Finish(give_up))
        }

        SubmissionOutcome::NotSent { failure } => {
            let uses_budget = !failure.is_connection_setup();
            let give_up = if uses_budget && budget_after >= policy.max_attempts {
                Terminal::Failed {
                    reason: Reason::TransportNotSent,
                }
            } else {
                Terminal::Expired
            };
            retry(Reason::TransportNotSent, uses_budget, false, None).unwrap_or(Decision::Finish(give_up))
        }

        // The provider may already have it. Retrying risks a duplicate (collapsed on
        // the device by the collapse id); not retrying risks a loss. Decision D1.
        SubmissionOutcome::Indeterminate { .. } => {
            let allowed = view.ambiguous_used < policy.ambiguous_retries;
            allowed
                .then(|| retry(Reason::TransportAmbiguous, true, true, None))
                .flatten()
                // Never `Expired` here: it may have been delivered.
                .unwrap_or(Decision::Finish(Terminal::Unknown {
                    reason: Reason::TransportAmbiguous,
                }))
        }
    };

    (decision, effects)
}
