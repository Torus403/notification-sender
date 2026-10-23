use std::time::Duration;

use crate::delivery::Reason;
use crate::model::Provider;
use crate::outcome::{FaultScope, ProviderError, SubmissionOutcome};

/// Apple asks senders to back off a throttled device; we wait at least this long.
pub const DEVICE_THROTTLE_BACKOFF: Duration = Duration::from_secs(60);

/// Classify an APNs response from its HTTP status and the `reason` field of its JSON
/// body. `apns_unique_id` is the `apns-unique-id` response header when present.
pub fn classify(status: u16, reason: Option<&str>, apns_unique_id: Option<String>) -> SubmissionOutcome {
    if status == 200 {
        return SubmissionOutcome::Accepted {
            provider_message_id: apns_unique_id,
        };
    }
    let raw = reason.unwrap_or("");
    let err = |reason: Reason| ProviderError {
        provider: Provider::Apns,
        http_status: status,
        raw_reason: raw.to_owned(),
        reason,
    };
    let rejected = |reason, scope| SubmissionOutcome::Rejected {
        error: err(reason),
        scope,
    };
    let retryable = |reason, retry_after, refresh_auth| SubmissionOutcome::Retryable {
        error: err(reason),
        retry_after,
        refresh_auth,
    };

    match (status, raw) {
        (400, "BadDeviceToken") | (410, "Unregistered" | "ExpiredToken") => {
            rejected(Reason::InvalidRegistration, FaultScope::Registration)
        }
        (400, "DeviceTokenNotForTopic") => rejected(Reason::WrongApp, FaultScope::Registration),
        (400, "TopicDisallowed" | "BadTopic") => rejected(Reason::CredentialTopicNotAllowed, FaultScope::Credential),
        (403, "ExpiredProviderToken") => retryable(Reason::CredentialTokenExpired, None, true),
        (403, _) => rejected(Reason::CredentialInvalid, FaultScope::Credential),
        (413, _) => rejected(Reason::PayloadTooLarge, FaultScope::Payload),
        (429, "TooManyProviderTokenUpdates") => retryable(Reason::InternalProviderMisuse, None, false),
        (429, _) => retryable(Reason::DeviceThrottled, Some(DEVICE_THROTTLE_BACKOFF), false),
        (400, _) => rejected(Reason::PayloadInvalid, FaultScope::Payload),
        (500..=599, _) => retryable(Reason::ProviderUnavailable, None, false),
        // 404 BadPath, 405 MethodNotAllowed and anything undocumented: our bug, and
        // never a reason to touch a token.
        _ => rejected(Reason::InternalProviderMisuse, FaultScope::Payload),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope_of(status: u16, reason: &str) -> Option<FaultScope> {
        match classify(status, Some(reason), None) {
            SubmissionOutcome::Rejected { scope, .. } => Some(scope),
            _ => None,
        }
    }

    fn reason_of(status: u16, reason: &str) -> Reason {
        match classify(status, Some(reason), None) {
            SubmissionOutcome::Rejected { error, .. } | SubmissionOutcome::Retryable { error, .. } => error.reason,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn accepted_keeps_unique_id() {
        assert_eq!(
            classify(200, None, Some("abc".into())),
            SubmissionOutcome::Accepted {
                provider_message_id: Some("abc".into())
            }
        );
    }

    // One assertion per row of spec table 9.1.
    #[test]
    fn spec_table_9_1() {
        assert_eq!(scope_of(400, "BadDeviceToken"), Some(FaultScope::Registration));
        assert_eq!(reason_of(400, "BadDeviceToken"), Reason::InvalidRegistration);
        assert_eq!(scope_of(400, "DeviceTokenNotForTopic"), Some(FaultScope::Registration));
        assert_eq!(reason_of(400, "DeviceTokenNotForTopic"), Reason::WrongApp);
        for r in [
            "BadCollapseId",
            "BadExpirationDate",
            "BadPriority",
            "PayloadEmpty",
            "MissingTopic",
        ] {
            assert_eq!(scope_of(400, r), Some(FaultScope::Payload), "{r}");
        }
        assert_eq!(scope_of(400, "TopicDisallowed"), Some(FaultScope::Credential));
        assert_eq!(scope_of(400, "BadTopic"), Some(FaultScope::Credential));
        assert!(matches!(
            classify(403, Some("ExpiredProviderToken"), None),
            SubmissionOutcome::Retryable { refresh_auth: true, .. }
        ));
        for r in [
            "InvalidProviderToken",
            "BadCertificate",
            "BadCertificateEnvironment",
            "MissingProviderToken",
        ] {
            assert_eq!(scope_of(403, r), Some(FaultScope::Credential), "{r}");
        }
        assert_eq!(scope_of(410, "Unregistered"), Some(FaultScope::Registration));
        assert_eq!(scope_of(410, "ExpiredToken"), Some(FaultScope::Registration));
        assert_eq!(reason_of(413, "PayloadTooLarge"), Reason::PayloadTooLarge);
        assert!(matches!(
            classify(429, Some("TooManyRequests"), None),
            SubmissionOutcome::Retryable { retry_after: Some(d), .. } if d == DEVICE_THROTTLE_BACKOFF
        ));
        assert_eq!(
            reason_of(429, "TooManyProviderTokenUpdates"),
            Reason::InternalProviderMisuse
        );
        for (s, r) in [
            (500, "InternalServerError"),
            (503, "ServiceUnavailable"),
            (503, "Shutdown"),
        ] {
            assert!(
                matches!(classify(s, Some(r), None), SubmissionOutcome::Retryable { .. }),
                "{r}"
            );
        }
    }

    #[test]
    fn unknown_responses_never_touch_tokens() {
        // Includes 410: only the two named 410 reasons are a token verdict.
        for s in [400, 401, 404, 405, 410, 418, 422] {
            assert_ne!(scope_of(s, "SomethingNew"), Some(FaultScope::Registration), "{s}");
        }
    }
}
