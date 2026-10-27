//! Fake services for tool tests.

mod clock;
mod library;
mod models;
mod orbit;

pub use clock::FakeClock;
pub use library::FakeLibrary;
pub use models::FakeModels;
pub use orbit::FakeOrbit;

/// Builder for a complete set of fake services.
pub struct PortsBuilder {
    now: std::time::SystemTime,
}

impl Default for PortsBuilder {
    fn default() -> Self {
        Self {
            now: std::time::SystemTime::UNIX_EPOCH,
        }
    }
}

impl PortsBuilder {
    /// Set the fixed time returned by the fake clock.
    #[must_use]
    pub fn now(mut self, now: std::time::SystemTime) -> Self {
        self.now = now;
        self
    }

    /// Construct the fake ports.
    #[must_use]
    pub fn build(self) -> office_core::ports::Ports<'static> {
        office_core::ports::Ports {
            library: Box::new(FakeLibrary::initialized()),
            models: Box::new(FakeModels::new()),
            orbit: Box::new(FakeOrbit),
            clock: Box::new(FakeClock(self.now)),
        }
    }
}
