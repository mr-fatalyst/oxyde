//! DELETE query building

use oxyde_codec::QueryIR;
use sea_query::{MysqlQueryBuilder, PostgresQueryBuilder, Query, SqliteQueryBuilder, Value};

use crate::error::Result;
use crate::filter::build_filter_node;
use crate::utils::TableIdent;
use crate::Dialect;

/// Build DELETE query from QueryIR
pub fn build_delete(ir: &QueryIR, dialect: Dialect) -> Result<(String, Vec<Value>)> {
    let table = TableIdent(ir.table.clone());
    let mut query = Query::delete();
    query.from_table(table);

    // Add filters (no JOIN in DELETE, so no table qualification needed)
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
