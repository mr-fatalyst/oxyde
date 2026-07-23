//! Schema diff computation.
//!
//! This crate owns exactly one responsibility: comparing two schema
//! snapshots and producing the list of migration operations.
//!
//! ```text
//! Snapshot (old) + Snapshot (new) → compute_diff() → Vec<MigrationOp>
//! ```
//!
//! The contract types (`Snapshot`, `TableDef`, `MigrationOp`, ...) live in
//! `oxyde-codec` — they are serialized across the Python boundary. SQL
//! rendering for the produced operations lives in `oxyde-sql`
//! (`migration_to_sql` / `MigrationOpExt`). This crate sees no SQL at all.

mod diff;

pub use diff::compute_diff;

// Contract types re-exported for convenience (canonical home: oxyde-codec).
pub use oxyde_codec::{
    CheckDef, EnumFieldRef, FieldDef, ForeignKeyDef, IndexDef, MigrateError, MigrationOp, Snapshot,
    TableDef,
};

pub type Result<T> = std::result::Result<T, MigrateError>;
