//! What a transport returns for one submission (spec sections 3 and 9).

use std::time::Duration;

use crate::delivery::Reason;
use crate::model::Provider;

/// Who is at fault decides what else may change. Only `Registration` may ever touch
/// a token: a payload mistake must never cost a customer their device list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultScope {
    /// This notification is wrong. Touch nothing else.
    Payload,
    /// This token is dead. Invalidate exactly the registration that was used.
    Registration,
    /// Authentication or configuration. Flag the credential.
    Credential,
}

/// A provider's answer, kept verbatim for diagnostics and normalized for customers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderError {
    pub provider: Provider,
    pub http_status: u16,
    /// APNs `reason` or FCM error code, exactly as received.
    pub raw_reason: String,
    pub reason: Reason,
}

/// Network failures, split by whether the provider could have processed the request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportFailure {
    // Provably never reached the provider.
    Dns,
    Connect,
    Tls,
    /// No free stream or throttle permit before the deadline; never written.
    NoCapacity,
    /// HTTP/2 `REFUSED_STREAM`: the peer guarantees it did not process the stream.
    RefusedStream,
    /// Stream id above the `GOAWAY` last-stream-id: guaranteed unprocessed.
    GoawayUnprocessed,
    // May have reached the provider.
    TimeoutAfterSend,
    ResetAfterSend,
    GoawayAfterSend,
}

impl TransportFailure {
    /// True when HTTP/2 or the socket layer proves the provider never saw the request.
    pub fn provably_not_sent(self) -> bool {
        matches!(
            self,
            Self::Dns | Self::Connect | Self::Tls | Self::NoCapacity | Self::RefusedStream | Self::GoawayUnprocessed
        )
    }

    /// Connection-setup failures do not use up a delivery's attempt budget, so a
    /// worker restart storm cannot exhaust deliveries (spec 9.4).
    pub fn is_connection_setup(self) -> bool {
        matches!(self, Self::Dns | Self::Connect | Self::Tls)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmissionOutcome {
    Accepted {
        provider_message_id: Option<String>,
    },
    Rejected {
        error: ProviderError,
        scope: FaultScope,
    },
    Retryable {
        error: ProviderError,
        retry_after: Option<Duration>,
        /// Refresh the provider auth token before the next attempt.
        refresh_auth: bool,
    },
    NotSent {
        failure: TransportFailure,
    },
    Indeterminate {
        failure: TransportFailure,
    },
}

impl SubmissionOutcome {
    /// Classify a transport failure into `NotSent` or `Indeterminate`.
    pub fn from_transport(failure: TransportFailure) -> Self {
        if failure.provably_not_sent() {
            Self::NotSent { failure }
        } else {
            Self::Indeterminate { failure }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_table_matches_spec_9_3() {
        use TransportFailure::*;
        for f in [Dns, Connect, Tls, NoCapacity, RefusedStream, GoawayUnprocessed] {
            assert!(
                matches!(SubmissionOutcome::from_transport(f), SubmissionOutcome::NotSent { .. }),
                "{f:?}"
            );
        }
        for f in [TimeoutAfterSend, ResetAfterSend, GoawayAfterSend] {
            assert!(
                matches!(
                    SubmissionOutcome::from_transport(f),
                    SubmissionOutcome::Indeterminate { .. }
                ),
                "{f:?}"
            );
        }
    }
}
