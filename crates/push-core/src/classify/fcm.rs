use std::time::Duration;

use crate::delivery::Reason;
use crate::model::Provider;
use crate::outcome::{FaultScope, ProviderError, SubmissionOutcome};

/// The parts of an FCM HTTP v1 error body the classifier needs. The transport parses
/// the JSON; this crate stays free of the wire format.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FcmError<'a> {
    /// The most specific code available: `details[].errorCode` of the
    /// `google.firebase.fcm.v1.FcmError` detail, else the top-level `status`.
    pub code: Option<&'a str>,
    /// True when the error names the registration token: a `BadRequest` field
    /// violation on `message.token`, or a message saying the token is not valid.
    pub names_registration_token: bool,
}

/// Classify an FCM response. `message_name` is the `name` of a successful send,
/// `retry_after` the parsed `Retry-After` header.
pub fn classify(
    status: u16,
    error: &FcmError<'_>,
    retry_after: Option<Duration>,
    message_name: Option<String>,
) -> SubmissionOutcome {
    if status == 200 {
        return SubmissionOutcome::Accepted {
            provider_message_id: message_name,
        };
    }
    let code = error.code.unwrap_or("");
    let err = |reason: Reason| ProviderError {
        provider: Provider::Fcm,
        http_status: status,
        raw_reason: code.to_owned(),
        reason,
    };
    let rejected = |reason, scope| SubmissionOutcome::Rejected {
        error: err(reason),
        scope,
    };
    let retryable = |reason, refresh_auth| SubmissionOutcome::Retryable {
        error: err(reason),
        retry_after,
        refresh_auth,
    };

    match code {
        "UNREGISTERED" => rejected(Reason::InvalidRegistration, FaultScope::Registration),
        // INVALID_ARGUMENT usually means our payload is wrong. Only an error that names
        // the token may invalidate it (spec 9.2, invariant I12).
        "INVALID_ARGUMENT" if error.names_registration_token => {
            rejected(Reason::InvalidRegistration, FaultScope::Registration)
        }
        "INVALID_ARGUMENT" => rejected(Reason::PayloadInvalid, FaultScope::Payload),
        "SENDER_ID_MISMATCH" => rejected(Reason::WrongApp, FaultScope::Registration),
        "UNAUTHENTICATED" => retryable(Reason::CredentialTokenExpired, true),
        "THIRD_PARTY_AUTH_ERROR" | "PERMISSION_DENIED" => rejected(Reason::CredentialInvalid, FaultScope::Credential),
        "QUOTA_EXCEEDED" | "RESOURCE_EXHAUSTED" => retryable(Reason::ProviderThrottled, false),
        "UNAVAILABLE" | "INTERNAL" => retryable(Reason::ProviderUnavailable, false),
        // Unrecognised code: decide by status, never invalidating a token.
        _ => match status {
            401 => retryable(Reason::CredentialTokenExpired, true),
            403 => rejected(Reason::CredentialInvalid, FaultScope::Credential),
            429 => retryable(Reason::ProviderThrottled, false),
            500..=599 => retryable(Reason::ProviderUnavailable, false),
            _ => rejected(Reason::InternalProviderMisuse, FaultScope::Payload),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(status: u16, code: &str, names_token: bool) -> SubmissionOutcome {
        let e = FcmError {
            code: Some(code),
            names_registration_token: names_token,
        };
        classify(status, &e, Some(Duration::from_secs(30)), None)
    }

    fn scope(o: &SubmissionOutcome) -> Option<FaultScope> {
        match o {
            SubmissionOutcome::Rejected { scope, .. } => Some(*scope),
            _ => None,
        }
    }

    // One assertion per row of spec table 9.2.
    #[test]
    fn spec_table_9_2() {
        assert!(matches!(
            classify(200, &FcmError::default(), None, Some("projects/p/messages/1".into())),
            SubmissionOutcome::Accepted {
                provider_message_id: Some(_)
            }
        ));
        assert_eq!(scope(&run(404, "UNREGISTERED", false)), Some(FaultScope::Registration));
        assert_eq!(
            scope(&run(400, "INVALID_ARGUMENT", true)),
            Some(FaultScope::Registration)
        );
        assert_eq!(scope(&run(400, "INVALID_ARGUMENT", false)), Some(FaultScope::Payload));
        assert_eq!(
            scope(&run(403, "SENDER_ID_MISMATCH", false)),
            Some(FaultScope::Registration)
        );
        assert!(matches!(
            run(401, "UNAUTHENTICATED", false),
            SubmissionOutcome::Retryable { refresh_auth: true, .. }
        ));
        assert_eq!(
            scope(&run(401, "THIRD_PARTY_AUTH_ERROR", false)),
            Some(FaultScope::Credential)
        );
        assert_eq!(
            scope(&run(403, "PERMISSION_DENIED", false)),
            Some(FaultScope::Credential)
        );
        assert!(matches!(
            run(429, "QUOTA_EXCEEDED", false),
            SubmissionOutcome::Retryable {
                retry_after: Some(_),
                refresh_auth: false,
                ..
            }
        ));
        assert!(matches!(
            run(503, "UNAVAILABLE", false),
            SubmissionOutcome::Retryable { .. }
        ));
        assert!(matches!(
            run(500, "INTERNAL", false),
            SubmissionOutcome::Retryable { .. }
        ));
    }

    #[test]
    fn unrecognised_codes_never_touch_tokens() {
        for s in [400, 401, 403, 404, 409, 429, 500] {
            assert_ne!(
                scope(&run(s, "SOMETHING_NEW", false)),
                Some(FaultScope::Registration),
                "{s}"
            );
            let empty = classify(s, &FcmError::default(), None, None);
            assert_ne!(scope(&empty), Some(FaultScope::Registration), "{s}");
        }
    }
}
