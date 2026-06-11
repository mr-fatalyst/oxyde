//! UPDATE query building

use oxyde_codec::{ColumnTypeSpec, QueryIR};
use sea_query::{Expr, MysqlQueryBuilder, PostgresQueryBuilder, Query, SqliteQueryBuilder, Value};

use crate::error::Result;
use crate::filter::build_filter_node;
use crate::utils::{bind_value, rmpv_to_simple_expr, ColumnIdent, TableIdent};
use crate::Dialect;

/// Build UPDATE query from QueryIR
pub fn build_update(ir: &QueryIR, dialect: Dialect) -> Result<(String, Vec<Value>)> {
    // Check for bulk update first
    if let Some(bulk) = &ir.bulk_update {
        return super::bulk::build_bulk_update(ir, bulk, dialect);
    }

    let table = TableIdent(ir.table.clone());
    let mut query = Query::update();
    query.table(table);

    // Helper to get the column spec; Unknown = native conversion
    let get_spec = |col: &str| -> &ColumnTypeSpec {
        ir.column_types
            .as_ref()
            .and_then(|ct| ct.get(col))
            .unwrap_or(&ColumnTypeSpec::Unknown)
    };

    if let Some(values) = &ir.values {
        for (col, val) in values {
            if let Some(expr) = rmpv_to_simple_expr(val)? {
                query.value(ColumnIdent(col.clone()), expr);
            } else {
                query.value(
                    ColumnIdent(col.clone()),
                    Expr::val(bind_value(val, get_spec(col))),
                );
            }
        }
    }

    // Add filters (no JOIN in UPDATE, so no table qualification needed)
    if let Some(filter_tree) = &ir.filter_tree {
        let expr = build_filter_node(filter_tree, None, ir.column_types.as_ref(), None)?;
        query.and_where(expr);
    }

    // Add RETURNING clause for Postgres/SQLite
    if crate::emits_returning(ir, dialect) {
        query.returning_all();
    }

    let (sql, values) = match dialect {
        Dialect::Postgres => query.build(PostgresQueryBuilder),
        Dialect::Sqlite => query.build(SqliteQueryBuilder),
        Dialect::Mysql => query.build(MysqlQueryBuilder),
    };

    Ok((sql, values.0))
}
