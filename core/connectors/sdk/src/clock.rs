//! Time is injected. Mapper and engine logic never read the system clock themselves, so a
//! test can freeze or step it.

use otw_connector_model::Timestamp;

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// The one place the wall clock is read.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_offset_datetime(time::OffsetDateTime::now_utc()).unwrap_or_default()
    }
}
