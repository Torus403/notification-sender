//! Delivery states and normalized reasons (spec sections 5 and 9).

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

/// A delivery's state as stored in `deliveries.state`. Allowed transitions are listed
/// in spec section 5; the database enforces them with guarded `update ... where`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryState {
    Pending {
        next_attempt_at: Timestamp,
    },
    Claimed {
        lease_epoch: u64,
        lease_until: Timestamp,
    },
    ProviderAccepted {
        provider_message_id: Option<String>,
    },
    Failed {
        reason: Reason,
    },
    /// Ambiguous and not retried.
    Unknown {
        reason: Reason,
    },
    Expired,
    /// Never sent: unbound, permission denied, paused.
    Suppressed {
        reason: Reason,
    },
    Cancelled,
}

impl DeliveryState {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Pending { .. } | Self::Claimed { .. })
    }

    /// The value stored in `deliveries.state`.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Pending { .. } => "pending",
            Self::Claimed { .. } => "claimed",
            Self::ProviderAccepted { .. } => "provider_accepted",
            Self::Failed { .. } => "failed",
            Self::Unknown { .. } => "unknown",
            Self::Expired => "expired",
            Self::Suppressed { .. } => "suppressed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Normalized reasons shown to customers. Stable strings: the API, CLI, MCP server and
/// docs key on them, so a variant is never renamed once shipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    // Registration faults
    InvalidRegistration,
    WrongApp,
    // Payload faults (our validation should have caught these)
    PayloadInvalid,
    PayloadTooLarge,
    // Credential faults
    CredentialInvalid,
    CredentialTokenExpired,
    CredentialTopicNotAllowed,
    // Transient
    DeviceThrottled,
    ProviderThrottled,
    ProviderUnavailable,
    /// Our own misuse of a provider API, e.g. refreshing APNs tokens too often.
    InternalProviderMisuse,
    // Transport
    TransportNotSent,
    TransportAmbiguous,
    // Never sent
    RecipientUnbound,
    NoActiveRegistration,
    PermissionDenied,
    AppPaused,
    EnvironmentPaused,
    TemplateDisabled,
    // Ends
    RetriesExhausted,
    Expired,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRegistration => "invalid_registration",
            Self::WrongApp => "wrong_app",
            Self::PayloadInvalid => "payload_invalid",
            Self::PayloadTooLarge => "payload_too_large",
            Self::CredentialInvalid => "credential_invalid",
            Self::CredentialTokenExpired => "credential_token_expired",
            Self::CredentialTopicNotAllowed => "credential_topic_not_allowed",
            Self::DeviceThrottled => "device_throttled",
            Self::ProviderThrottled => "provider_throttled",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::InternalProviderMisuse => "internal_provider_misuse",
            Self::TransportNotSent => "transport_not_sent",
            Self::TransportAmbiguous => "transport_ambiguous",
            Self::RecipientUnbound => "recipient_unbound",
            Self::NoActiveRegistration => "no_active_registration",
            Self::PermissionDenied => "permission_denied",
            Self::AppPaused => "app_paused",
            Self::EnvironmentPaused => "environment_paused",
            Self::TemplateDisabled => "template_disabled",
            Self::RetriesExhausted => "retries_exhausted",
            Self::Expired => "expired",
        }
    }
}
