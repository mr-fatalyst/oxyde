//! Connection pool management

pub mod handle;
pub mod registry;

pub use handle::{DatabaseBackend, DbPool, PoolHandle};
pub(crate) use registry::ConnectionRegistry;
