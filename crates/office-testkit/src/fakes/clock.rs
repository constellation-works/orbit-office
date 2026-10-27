//! Fake clock port.

/// A clock with a fixed time.
pub struct FakeClock(pub std::time::SystemTime);
impl office_core::ports::Clock for FakeClock {
    fn now(&self) -> std::time::SystemTime {
        self.0
    }
}
