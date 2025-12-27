//! UPDATE query building

use oxyde_codec::QueryIR;
use sea_query::{Expr, MysqlQueryBuilder, PostgresQueryBuilder, Query, SqliteQueryBuilder, Value};

use crate::error::Result;
use crate::filter::build_filter_node;
use crate::utils::{json_to_simple_expr, json_to_value_typed, ColumnIdent, TableIdent};
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

    // Helper to get column type from col_types
    let get_col_type = |col: &str| -> Option<&str> {
        ir.col_types
            .as_ref()
            .and_then(|ct| ct.get(col).map(|s| s.as_str()))
    };

    if let Some(values) = &ir.values {
        for (col, val) in values {
            if let Some(expr) = json_to_simple_expr(val)? {
                query.value(ColumnIdent(col.clone()), expr);
            } else {
                let col_type = get_col_type(col);
                query.value(
                    ColumnIdent(col.clone()),
                    Expr::val(json_to_value_typed(val, col_type)),
                );
            }
        }
    }

    // Add filters (no JOIN in UPDATE, so no table qualification needed)
    if let Some(filter_tree) = &ir.filter_tree {
        let expr = build_filter_node(filter_tree, None)?;
        query.and_where(expr);
    }

    // Add RETURNING clause for Postgres/SQLite
    if ir.returning.unwrap_or(false) && matches!(dialect, Dialect::Postgres | Dialect::Sqlite) {
        query.returning_all();
    }

    let (sql, values) = match dialect {
        Dialect::Postgres => query.build(PostgresQueryBuilder),
        Dialect::Sqlite => query.build(SqliteQueryBuilder),
        Dialect::Mysql => query.build(MysqlQueryBuilder),
    };

    Ok((sql, values.0))
}
