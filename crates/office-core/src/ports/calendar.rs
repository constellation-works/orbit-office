//! Calendar snapshot storage and contained offline import reads.
use crate::Result;
use crate::domain::calendar::{CalendarError, CalendarSources, LocalChange, Snapshot};

/// Calendar I/O, independent of gateway reads and normalization.
pub trait Calendar {
    /// Read persistent source choices without creating state.
    fn sources(&self) -> Result<CalendarSources> {
        Ok(CalendarSources::default())
    }
    /// Apply a validated local mutation atomically; never calls a gateway.
    fn change_local(&self, _: &LocalChange) -> Result<Option<String>> {
        Err(CalendarError::Storage.into())
    }

    /// Read without creating state. Errors are safe stable codes only.
    fn load(&self) -> Result<(Snapshot, Option<String>)>;
    /// Atomically replace a complete snapshot and invalidate prior failure status.
    fn replace(&self, snapshot: &Snapshot) -> Result<()>;
    /// Record a failure without changing one byte of the previous snapshot.
    fn record_error(&self, code: &str) -> Result<()>;
    /// Read at most 1 MiB of UTF-8 from a regular workspace-relative ICS file.
    fn read_ics(&self, relative: &str) -> Result<String>;
}
/// Refusing adapter for call contexts without calendar storage.
pub struct UnavailableCalendar;
impl Calendar for UnavailableCalendar {
    fn load(&self) -> Result<(Snapshot, Option<String>)> {
        Err(CalendarError::Storage.into())
    }
    fn replace(&self, _: &Snapshot) -> Result<()> {
        Err(CalendarError::Storage.into())
    }
    fn record_error(&self, _: &str) -> Result<()> {
        Err(CalendarError::Storage.into())
    }
    fn read_ics(&self, _: &str) -> Result<String> {
        Err(CalendarError::Storage.into())
    }
}
