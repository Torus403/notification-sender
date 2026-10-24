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
