//! The explorer's integration tests, in one binary (one link, not one per file): the
//! worker and the repository on real CSV and SQLite files, exports, structure changes, and
//! real MySQL / PostgreSQL servers when they're given. The drivers' protocol tests stay a
//! binary of their own (`tests/protocol.rs`: it's also the driver's program they start).

mod csv;
mod export;
mod mysql;
mod postgres;
mod schema;
mod sqlite;
