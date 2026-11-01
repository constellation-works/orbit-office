//! Clock port.

/// Supplies the current time to tools.
pub trait Clock {
    /// Return the current time.
    fn now(&self) -> std::time::SystemTime;
    /// The host's IANA time zone, for local times such as SCR file names;
    /// `None` (the default) means UTC.
    fn time_zone(&self) -> Option<String> {
        None
    }
}
