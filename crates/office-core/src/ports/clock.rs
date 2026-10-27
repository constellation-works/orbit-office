//! Clock port.

/// Supplies the current time to tools.
pub trait Clock {
    /// Return the current time.
    fn now(&self) -> std::time::SystemTime;
}
