//! Ratspeak SQLite layer. Owns the schema, migrations, connection pool, and
//! per-domain queries. Depends on `ratspeak-core` for shared types only.

pub mod admission;
pub mod db;
pub mod static_nodes;

pub use db::*;
