//! Connection pool management

pub mod api;
pub mod handle;
pub(crate) mod registry;

pub use handle::{DatabaseBackend, DbPool, PoolHandle};
pub(crate) use registry::ConnectionRegistry;
