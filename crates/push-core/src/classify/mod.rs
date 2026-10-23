//! Provider response classification (spec 9.1 and 9.2).
//!
//! The rule that governs both tables: only an explicitly named, token-specific error
//! is classified as `FaultScope::Registration`. Anything unrecognised falls back to a
//! scope that touches no tokens, because wrongly invalidating registrations is the
//! one mistake a customer cannot recover from.

pub mod apns;
pub mod fcm;
