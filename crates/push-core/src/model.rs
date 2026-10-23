//! What a customer can ask for (spec sections 1 and 3).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::ids::InstallationId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Ios,
    Android,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Apns,
    Fcm,
}

/// Which APNs host a token needs. A property of the registration, not the app:
/// it depends on how the build was signed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApnsEnv {
    Sandbox,
    Production,
}

/// The customer's own user id. Opaque: never parsed, never interpreted.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ExternalUserId(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("external user id must be 1 to 256 bytes")]
pub struct InvalidExternalUserId;

impl ExternalUserId {
    pub const MAX_BYTES: usize = 256;

    pub fn new(s: impl Into<String>) -> Result<Self, InvalidExternalUserId> {
        let s = s.into();
        if s.is_empty() || s.len() > Self::MAX_BYTES {
            return Err(InvalidExternalUserId);
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ExternalUserId {
    type Error = InvalidExternalUserId;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<ExternalUserId> for String {
    fn from(id: ExternalUserId) -> Self {
        id.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    User(ExternalUserId),
    Installation(InstallationId),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Alert,
    /// `content-available` on iOS, data-only on Android. No UI.
    Background,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    #[default]
    High,
    Normal,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    #[default]
    Default,
    Silent,
    /// A logical name from the app's sound catalog, resolved per platform.
    Named(String),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Content {
    pub title: Option<String>,
    /// iOS only; dropped on Android.
    pub subtitle: Option<String>,
    pub body: Option<String>,
    /// Customer data. The key `_p` is reserved for our ids.
    #[serde(default)]
    pub data: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub sound: Sound,
    pub badge: Option<u32>,
    /// APNs `thread-id`; Android group.
    pub group: Option<String>,
}

/// Data key reserved for the engine's own ids (`{"n": ntf, "d": del}`).
pub const RESERVED_DATA_KEY: &str = "_p";

/// TTL bounds (spec decision D6).
pub const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);
pub const MAX_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Most recently seen installations a user-targeted send fans out to (decision D7).
pub const MAX_INSTALLATIONS_PER_SEND: usize = 50;

/// APNs rejects collapse ids above 64 bytes.
pub const MAX_COLLAPSE_KEY_BYTES: usize = 64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_user_id_bounds() {
        assert!(ExternalUserId::new("").is_err());
        assert!(ExternalUserId::new("u".repeat(256)).is_ok());
        assert!(ExternalUserId::new("u".repeat(257)).is_err());
    }
}
