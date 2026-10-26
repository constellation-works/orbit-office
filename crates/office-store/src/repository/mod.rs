//! Repository facades: the surfaces callers actually use. A facade owns
//! the drivers it coordinates and exposes Library operations, never SQL
//! or file paths beyond Library-relative ones.

mod library;

pub use library::LibraryStore;
