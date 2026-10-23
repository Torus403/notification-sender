//! Domain model and pure decision functions for the push delivery engine.
//!
//! Nothing in this crate performs I/O or reads a clock. Every state transition is a
//! function of (current state, event, time, policy), so the rules that decide what
//! happens to a delivery can be tested exhaustively without a database or network.
//! See the engine specification, sections 3, 5 and 9.

pub mod classify;
pub mod decide;
pub mod delivery;
pub mod ids;
pub mod model;
pub mod outcome;
pub mod time;

pub use decide::{Decision, DeliveryView, RetryPolicy, SideEffect, Terminal, decide};
pub use delivery::{DeliveryState, Reason};
pub use outcome::{FaultScope, ProviderError, SubmissionOutcome, TransportFailure};
pub use time::Timestamp;
