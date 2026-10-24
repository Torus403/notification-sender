//! Typed identifiers. Storage uses UUIDv7; the public API shows each id with a
//! type prefix and Crockford base32, e.g. `ntf_01k8x3...` (spec section 2).

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
const ENCODED_LEN: usize = 26;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    #[error("expected prefix `{expected}_`")]
    WrongPrefix { expected: &'static str },
    #[error("expected {ENCODED_LEN} base32 characters after the prefix")]
    BadLength,
    #[error("invalid base32 character `{0}`")]
    BadChar(char),
    #[error("value overflows 128 bits")]
    Overflow,
}

fn encode(id: Uuid) -> String {
    let v = id.as_u128();
    (0..ENCODED_LEN)
        .map(|i| {
            let shift = 5 * (ENCODED_LEN - 1 - i);
            char::from(ALPHABET[((v >> shift) & 31) as usize])
        })
        .collect()
}

fn decode_char(c: char) -> Result<u128, IdError> {
    // Crockford: case-insensitive, with I/L read as 1 and O read as 0.
    let n = match c.to_ascii_lowercase() {
        c @ '0'..='9' => c as u32 - '0' as u32,
        'o' => 0,
        'i' | 'l' => 1,
        c => match ALPHABET.iter().position(|&a| char::from(a) == c) {
            Some(p) => p as u32,
            None => return Err(IdError::BadChar(c)),
        },
    };
    Ok(u128::from(n))
}

fn decode(s: &str) -> Result<Uuid, IdError> {
    if s.chars().count() != ENCODED_LEN {
        return Err(IdError::BadLength);
    }
    let mut chars = s.chars();
    let mut v: u128 = 0;
    // The first character carries only the top 3 bits of a 128-bit value.
    if let Some(first) = chars.next() {
        let n = decode_char(first)?;
        if n > 7 {
            return Err(IdError::Overflow);
        }
        v = n;
    }
    for c in chars {
        v = (v << 5) | decode_char(c)?;
    }
    Ok(Uuid::from_u128(v))
}

macro_rules! typed_id {
    ($(#[$doc:meta])* $name:ident, $prefix:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            /// A new time-ordered id. The only place this crate touches a clock.
            pub fn generate() -> Self {
                Self(Uuid::now_v7())
            }

            pub const fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", $prefix, encode(self.0))
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let rest = s
                    .strip_prefix($prefix)
                    .and_then(|r| r.strip_prefix('_'))
                    .ok_or(IdError::WrongPrefix { expected: $prefix })?;
                decode(rest).map(Self)
            }
        }
    };
}

typed_id!(OrganizationId, "org");
typed_id!(ProjectId, "prj");
typed_id!(EnvironmentId, "env");
typed_id!(AppId, "app");
typed_id!(CredentialId, "cred");
typed_id!(InstallationId, "ins");
typed_id!(
    /// A provider registration (one token). A new token is a new row, so this id is
    /// also the registration's version. Never shown in the public API.
    RegistrationId,
    "reg"
);
typed_id!(NotificationId, "ntf");
typed_id!(DeliveryId, "del");
typed_id!(TemplateId, "tpl");
