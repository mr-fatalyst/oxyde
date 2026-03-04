//! Schema migration system with diff computation and SQL generation.
//!
//! This crate provides Django-style migrations for Oxyde ORM. It compares
//! model schemas (snapshots) and generates migration operations.
//!
//! # Architecture
//!
//! ```text
//! Models → Snapshot (JSON) → compute_diff() → MigrationOp[] → to_sql() → DDL
//! ```
//!
//! # Core Types
//!
//! ## Snapshot
//! Point-in-time representation of database schema:
//! - `tables`: HashMap of TableDef
//! - `version`: Schema version number
//!
//! ## TableDef
//! Table schema definition:
//! - `fields`: Column definitions (FieldDef)
//! - `indexes`: Index definitions (IndexDef)
//! - `foreign_keys`: FK constraints (ForeignKeyDef)
//! - `checks`: CHECK constraints (CheckDef)
//!
//! ## MigrationOp
//! Individual migration operation (enum):
//! - CreateTable, DropTable, RenameTable
//! - AddColumn, DropColumn, RenameColumn, AlterColumn
//! - CreateIndex, DropIndex
//! - AddForeignKey, DropForeignKey
//! - AddCheck, DropCheck
//!
//! # Dialect Support
//!
//! - **PostgreSQL**: Full ALTER TABLE support
//! - **SQLite**: Limited ALTER (requires table rebuild for some ops)
//! - **MySQL**: Full support with CHANGE/MODIFY syntax
//!
//! # SQLite Limitations
//!
//! SQLite doesn't support:
//! - ALTER TABLE ADD CONSTRAINT (FK/CHECK)
//! - ALTER COLUMN (type changes)
//!
//! Solution: Table rebuild migration (12-step process):
//! 1. PRAGMA foreign_keys=OFF
//! 2. CREATE TABLE _new_X with new schema
//! 3. INSERT INTO _new_X SELECT * FROM X
//! 4. DROP TABLE X
//! 5. ALTER TABLE _new_X RENAME TO X
//! 6. Recreate indexes
//! 7. PRAGMA foreign_keys=ON
//!
//! # Usage
//!
//! ```rust,ignore
//! // Compute diff between snapshots
//! let ops = compute_diff(&old_snapshot, &new_snapshot);
//!
//! // Generate SQL for PostgreSQL
//! let migration = Migration { name: "0001".into(), operations: ops };
//! let sql_statements = migration.to_sql(Dialect::Postgres)?;
//! ```

mod diff;
mod op;
mod sql;
mod types;

#[cfg(test)]
mod tests;

// Public re-exports (API не меняется)
pub use diff::{compute_diff, Migration};
pub use op::MigrationOp;
pub use types::*;
