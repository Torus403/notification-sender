use std::time::Duration;

/// A point in time as Unix milliseconds. The engine never reads a clock inside
/// `push-core`; callers pass `now` in, which keeps every decision deterministic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const fn from_unix_millis(ms: i64) -> Self {
        Self(ms)
    }

    pub const fn unix_millis(self) -> i64 {
        self.0
    }

    pub fn saturating_add(self, d: Duration) -> Self {
        let ms = i64::try_from(d.as_millis()).unwrap_or(i64::MAX);
        Self(self.0.saturating_add(ms))
    }

    pub fn saturating_sub(self, d: Duration) -> Self {
        let ms = i64::try_from(d.as_millis()).unwrap_or(i64::MAX);
        Self(self.0.saturating_sub(ms))
    }
}
