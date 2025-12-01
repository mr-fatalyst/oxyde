//! Connection pool management

pub mod handle;
pub mod registry;

pub(crate) use handle::DbPool;
pub use handle::{DatabaseBackend, PoolHandle};
pub(crate) use registry::ConnectionRegistry;
