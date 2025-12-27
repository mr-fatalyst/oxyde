//! Bulk UPDATE query building

use std::collections::BTreeSet;

use oxyde_codec::{BulkUpdate, BulkUpdateRow, QueryIR};
use sea_query::{
    CaseStatement, Cond, Expr, MysqlQueryBuilder, PostgresQueryBuilder, Query, SimpleExpr,
    SqliteQueryBuilder, Value,
};

use crate::error::{QueryError, Result};
use crate::filter::build_filter_node;
use crate::utils::{json_to_value, ColumnIdent, TableIdent};
use crate::Dialect;

/// Build bulk UPDATE query using CASE WHEN statements
pub fn build_bulk_update(
    ir: &QueryIR,
    bulk: &BulkUpdate,
    dialect: Dialect,
) -> Result<(String, Vec<Value>)> {
    let table = TableIdent(ir.table.clone());
    let mut query = Query::update();
    query.table(table);

    let mut update_columns: BTreeSet<String> = BTreeSet::new();
    let mut row_conditions: Vec<Cond> = Vec::new();

    for row in &bulk.rows {
        for column in row.values.keys() {
            update_columns.insert(column.clone());
        }
        let cond = build_bulk_row_condition(row)?;
        row_conditions.push(cond);
    }

    if update_columns.is_empty() {
        return Err(QueryError::InvalidQuery(
            "bulk_update requires at least one column to update".into(),
        ));
    }

    for column in update_columns {
        let mut case_stmt = CaseStatement::new();
        for (row, cond) in bulk.rows.iter().zip(&row_conditions) {
            if let Some(value) = row.values.get(&column) {
                case_stmt = case_stmt.case(cond.clone(), Expr::val(json_to_value(value)));
            }
        }
        case_stmt = case_stmt.finally(Expr::col(ColumnIdent(column.clone())));
        query.value(ColumnIdent(column), case_stmt);
    }

    let mut filter_cond = Cond::any();
    for cond in &row_conditions {
        filter_cond = filter_cond.add(cond.clone());
    }
    query.cond_where(filter_cond);

    if let Some(filter_tree) = &ir.filter_tree {
        let expr = build_filter_node(filter_tree, None)?;
        query.and_where(expr);
    }

    if ir.returning.unwrap_or(false) && matches!(dialect, Dialect::Postgres | Dialect::Sqlite) {
        query.returning_all();
    }

    let built = match dialect {
        Dialect::Postgres => query.build(PostgresQueryBuilder),
        Dialect::Sqlite => query.build(SqliteQueryBuilder),
        Dialect::Mysql => query.build(MysqlQueryBuilder),
    };

    Ok((built.0, built.1 .0))
}

fn build_bulk_row_condition(row: &BulkUpdateRow) -> Result<Cond> {
    let mut cond = Cond::all();
    for (column, value) in &row.filters {
        cond = cond.add(build_match_expression(column, value));
    }
    Ok(cond)
}

fn build_match_expression(column: &str, value: &serde_json::Value) -> SimpleExpr {
    if value.is_null() {
        Expr::col(ColumnIdent(column.to_string())).is_null()
    } else {
        Expr::col(ColumnIdent(column.to_string())).eq(json_to_value(value))
    }
}
