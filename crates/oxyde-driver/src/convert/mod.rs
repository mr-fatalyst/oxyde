//! Type conversion utilities for database rows.
//!
//! ## Encoder modules (new — direct msgpack)
//!
//! `encoder` defines the `CellEncoder` trait and generic columnar encoding.
//! `postgres_enc`, `mysql_enc`, `sqlite_enc` implement it per backend.
//! These write directly to `Vec<u8>` msgpack buffers, no intermediate serde_json.
//!
//! ## Legacy modules (serde_json path)
//!
//! `postgres`, `mysql`, `sqlite` convert rows to `HashMap<String, serde_json::Value>`.
//! Still used by the non-columnar `query()` method in traits (driver tests, etc).

pub mod mysql;
pub mod postgres;
pub mod sqlite;

pub mod encoder;
pub mod mysql_enc;
pub mod postgres_enc;
pub mod sqlite_enc;

pub use mysql::convert_mysql_rows_typed;
pub use postgres::{convert_pg_rows, convert_pg_rows_typed};
pub use sqlite::{convert_sqlite_rows, convert_sqlite_rows_typed};
