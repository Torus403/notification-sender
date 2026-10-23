//! Property tests for the invariants `decide` owns (spec section 15).

use std::time::Duration;

use proptest::prelude::*;
use push_core::model::Provider;
use push_core::{
    Decision, DeliveryView, FaultScope, ProviderError, Reason, RetryPolicy, SideEffect, SubmissionOutcome, Terminal,
    Timestamp, TransportFailure, decide,
};

fn reason() -> impl Strategy<Value = Reason> {
    prop_oneof![
        Just(Reason::InvalidRegistration),
        Just(Reason::WrongApp),
        Just(Reason::PayloadInvalid),
        Just(Reason::PayloadTooLarge),
        Just(Reason::CredentialInvalid),
        Just(Reason::ProviderThrottled),
        Just(Reason::ProviderUnavailable),
        Just(Reason::DeviceThrottled),
    ]
}

fn failure() -> impl Strategy<Value = TransportFailure> {
    use TransportFailure::*;
    prop_oneof![
        Just(Dns),
        Just(Connect),
        Just(Tls),
        Just(NoCapacity),
        Just(RefusedStream),
        Just(GoawayUnprocessed),
        Just(TimeoutAfterSend),
        Just(ResetAfterSend),
        Just(GoawayAfterSend),
    ]
}

fn outcome() -> impl Strategy<Value = SubmissionOutcome> {
    let err = || {
        (reason(), 400u16..600).prop_map(|(reason, http_status)| ProviderError {
            provider: Provider::Fcm,
            http_status,
            raw_reason: String::new(),
            reason,
        })
    };
    let scope = prop_oneof![
        Just(FaultScope::Payload),
        Just(FaultScope::Registration),
        Just(FaultScope::Credential)
    ];
    prop_oneof![
        Just(SubmissionOutcome::Accepted {
            provider_message_id: None
        }),
        (err(), scope).prop_map(|(error, scope)| SubmissionOutcome::Rejected { error, scope }),
        (err(), proptest::option::of(0u64..900), any::<bool>()).prop_map(|(error, ra, refresh_auth)| {
            SubmissionOutcome::Retryable {
                error,
                retry_after: ra.map(Duration::from_secs),
                refresh_auth,
            }
        }),
        failure().prop_map(SubmissionOutcome::from_transport),
    ]
}

prop_compose! {
    fn scenario()(
        budget_used in 0u16..8,
        ambiguous_used in 0u16..3,
        now_ms in 0i64..1_000_000_000,
        ttl_ms in 0i64..(8 * 24 * 3_600_000),
        jitter in 0.0f64..=1.0,
    ) -> (DeliveryView, Timestamp, f64) {
        let now = Timestamp::from_unix_millis(now_ms);
        let view = DeliveryView {
            budget_used,
            ambiguous_used,
            expires_at: Timestamp::from_unix_millis(now_ms + ttl_ms),
        };
        (view, now, jitter)
    }
}

proptest! {
    /// I9 and I15: a retry is always scheduled before the delivery's deadline, which
    /// `decide` has no way to change.
    #[test]
    fn retries_land_before_expiry((view, now, jitter) in scenario(), o in outcome()) {
        let p = RetryPolicy::default();
        if let (Decision::RetryAt { at, .. }, _) = decide(&view, &o, now, &p, jitter) {
            prop_assert!(at > now);
            prop_assert!(at < view.expires_at.saturating_sub(p.expiry_margin));
        }
    }

    /// I12: only a registration-scoped rejection can invalidate a token.
    #[test]
    fn only_registration_faults_invalidate((view, now, jitter) in scenario(), o in outcome()) {
        let (_, effects) = decide(&view, &o, now, &RetryPolicy::default(), jitter);
        let invalidates = effects.iter().any(|e| matches!(e, SideEffect::InvalidateRegistration { .. }));
        let registration_fault = matches!(o, SubmissionOutcome::Rejected { scope: FaultScope::Registration, .. });
        prop_assert_eq!(invalidates, registration_fault);
    }

    /// Spec 9.4: an ambiguous send is retried at most `ambiguous_retries` times, and an
    /// ambiguous send that is not retried ends `unknown`, never `expired` or `failed`.
    #[test]
    fn ambiguity_is_bounded_and_honest((view, now, jitter) in scenario(), f in failure()) {
        let o = SubmissionOutcome::from_transport(f);
        let p = RetryPolicy::default();
        let (d, _) = decide(&view, &o, now, &p, jitter);
        if matches!(o, SubmissionOutcome::Indeterminate { .. }) {
            match d {
                Decision::RetryAt { ambiguous, .. } => {
                    prop_assert!(ambiguous);
                    prop_assert!(view.ambiguous_used < p.ambiguous_retries);
                }
                Decision::Finish(t) => {
                    let is_unknown = matches!(t, Terminal::Unknown { .. });
                    prop_assert!(is_unknown, "ambiguous send finished as {:?}", t);
                }
            }
        }
    }

    /// Provider guidance is a floor on the retry delay.
    #[test]
    fn retry_after_is_honoured((view, now, jitter) in scenario(), secs in 0u64..900, r in reason()) {
        let error = ProviderError { provider: Provider::Fcm, http_status: 429, raw_reason: String::new(), reason: r };
        let o = SubmissionOutcome::Retryable { error, retry_after: Some(Duration::from_secs(secs)), refresh_auth: false };
        if let (Decision::RetryAt { at, .. }, _) = decide(&view, &o, now, &RetryPolicy::default(), jitter) {
            prop_assert!(at >= now.saturating_add(Duration::from_secs(secs)));
        }
    }

    /// Termination: following `decide` with arbitrary outcomes always reaches a terminal
    /// state within the attempt budget plus the setup-failure retries the deadline allows.
    #[test]
    fn every_delivery_terminates(
        outcomes in proptest::collection::vec(outcome(), 1..400),
        jitter in 0.0f64..=1.0,
        ttl_secs in 1i64..600,
    ) {
        let p = RetryPolicy::default();
        let mut now = Timestamp::from_unix_millis(0);
        let mut view = DeliveryView {
            budget_used: 0,
            ambiguous_used: 0,
            expires_at: Timestamp::from_unix_millis(ttl_secs * 1000),
        };
        let mut finished = false;
        for o in outcomes.iter().cycle().take(10_000) {
            match decide(&view, o, now, &p, jitter).0 {
                Decision::Finish(_) => { finished = true; break; }
                Decision::RetryAt { at, uses_budget, ambiguous, .. } => {
                    prop_assert!(at > now);
                    now = at;
                    view.budget_used += u16::from(uses_budget);
                    view.ambiguous_used += u16::from(ambiguous);
                    prop_assert!(view.budget_used < p.max_attempts);
                }
            }
        }
        prop_assert!(finished);
    }
}
