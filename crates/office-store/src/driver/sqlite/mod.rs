//! The SQLite driver: the rebuildable Library index (FTS5 plus link and
//! citation tables). One writer connection behind a mutex — WAL permits a
//! single writer, and lock scope is one method call, never held across
//! unrelated I/O. (After orbit-store's SQLite `Store`.)

pub(crate) mod connection;
mod migration;
