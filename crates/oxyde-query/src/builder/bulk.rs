//! Bulk UPDATE query building

use std::collections::{BTreeSet, HashMap};

use oxyde_codec::{BulkUpdate, BulkUpdateRow, QueryIR};
use sea_query::{
    CaseStatement, Cond, Expr, MysqlQueryBuilder, PostgresQueryBuilder, Query, SimpleExpr,
    SqliteQueryBuilder, Value,
};

use crate::error::{QueryError, Result};
use crate::filter::build_filter_node;
use crate::utils::{rmpv_to_value_typed, ColumnIdent, TableIdent};
use crate::Dialect;

/// Build bulk UPDATE query using CASE WHEN statements.
///
/// Generates a single UPDATE with CASE WHEN expressions to update multiple rows
/// in one query. Each row has its own filter conditions and values.
///
/// Example output:
/// ```sql
/// UPDATE users SET name = CASE WHEN id = 1 THEN 'Alice'
///                               WHEN id = 2 THEN 'Bob'
///                               ELSE name END
///              WHERE id = 1 OR id = 2
/// ```
pub fn build_bulk_update(
    ir: &QueryIR,
    bulk: &BulkUpdate,
    dialect: Dialect,
) -> Result<(String, Vec<Value>)> {
    let table = TableIdent(ir.table.clone());
    let mut query = Query::update();
    query.table(table);

    let col_types = ir.col_types.as_ref();

    // Collect all columns that need updating across all rows
    let mut update_columns: BTreeSet<String> = BTreeSet::new();
    // Build WHERE conditions for each row (e.g., id = 1, id = 2)
    let mut row_conditions: Vec<Cond> = Vec::new();

    for row in &bulk.rows {
        for column in row.values.keys() {
            update_columns.insert(column.clone());
        }
        let cond = build_bulk_row_condition(row, col_types)?;
        row_conditions.push(cond);
    }

    if update_columns.is_empty() {
        return Err(QueryError::InvalidQuery(
            "bulk_update requires at least one column to update".into(),
        ));
    }

    // For each column, build a CASE WHEN expression:
    // CASE WHEN <row1_filter> THEN <row1_value>
    //      WHEN <row2_filter> THEN <row2_value>
    //      ELSE <current_column_value> END
    for column in update_columns {
        let mut case_stmt = CaseStatement::new();
        let col_type = col_types.and_then(|ct| ct.get(&column).map(String::as_str));
        for (row, cond) in bulk.rows.iter().zip(&row_conditions) {
            if let Some(value) = row.values.get(&column) {
                case_stmt = case_stmt.case(
                    cond.clone(),
                    Expr::val(rmpv_to_value_typed(value, col_type)),
                );
            }
        }
        // ELSE keeps current value for rows not matched by this column
        case_stmt = case_stmt.finally(Expr::col(ColumnIdent(column.clone())));
        query.value(ColumnIdent(column), case_stmt);
    }

    // WHERE clause: OR of all row conditions to limit update scope
    let mut filter_cond = Cond::any();
    for cond in &row_conditions {
        filter_cond = filter_cond.add(cond.clone());
    }
    query.cond_where(filter_cond);

    if let Some(filter_tree) = &ir.filter_tree {
        let expr = build_filter_node(filter_tree, None, col_types, None)?;
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

/// Build AND condition from a row's filters (e.g., `id = 1 AND tenant = 'a'`).
fn build_bulk_row_condition(
    row: &BulkUpdateRow,
    col_types: Option<&HashMap<String, String>>,
) -> Result<Cond> {
    let mut cond = Cond::all();
    for (column, value) in &row.filters {
        cond = cond.add(build_match_expression(column, value, col_types));
    }
    Ok(cond)
}

/// Build a single column match expression, handling NULL with IS NULL.
fn build_match_expression(
    column: &str,
    value: &rmpv::Value,
    col_types: Option<&HashMap<String, String>>,
) -> SimpleExpr {
    if value.is_nil() {
        Expr::col(ColumnIdent(column.to_string())).is_null()
    } else {
        let col_type = col_types.and_then(|ct| ct.get(column).map(String::as_str));
        Expr::col(ColumnIdent(column.to_string())).eq(rmpv_to_value_typed(value, col_type))
    }
}
