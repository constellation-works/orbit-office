//! Port traits: how the application (`office-tools`) reaches files, models,
//! Orbit and the clock. A port is declared here by the first task that needs
//! it, in its own file, together with its adapter (`office-store`,
//! `office-models`, `office-orbit`) and its fake (`office-testkit`).
//! Declaring a trait does no I/O, so this crate stays pure.

mod clock;
mod library;
mod models;
mod orbit;
