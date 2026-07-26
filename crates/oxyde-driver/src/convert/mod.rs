//! Type conversion: database rows → msgpack bytes.
//!
//! `encoder` defines the `CellEncoder` trait and generic columnar encoding.
//! `postgres`, `mysql`, `sqlite` implement it per backend.
//! All encoding writes directly to `Vec<u8>` msgpack buffers.

// msgpack container lengths are u32 by format (counts never approach it);
// i64 → f64 for REAL columns is the documented SQLite conversion.
#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

pub mod encoder;
pub mod mysql;
pub mod postgres;
pub mod sqlite;
