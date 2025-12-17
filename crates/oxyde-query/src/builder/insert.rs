//! INSERT query building

use oxyde_codec::{ConflictAction, QueryIR};
use sea_query::{
    Expr, MysqlQueryBuilder, PostgresQueryBuilder, Query, SimpleExpr, SqliteQueryBuilder, Value,
};

use crate::error::{QueryError, Result};
use crate::utils::{json_to_simple_expr, json_to_value_typed, ColumnIdent, TableIdent};
use crate::Dialect;

/// Build INSERT query from QueryIR
pub fn build_insert(ir: &QueryIR, dialect: Dialect) -> Result<(String, Vec<Value>)> {
    let table = TableIdent(ir.table.clone());
    let mut query = Query::insert();
    query.into_table(table);

    // Helper to get column type from col_types
    let get_col_type = |col: &str| -> Option<&str> {
        ir.col_types
            .as_ref()
            .and_then(|ct| ct.get(col).map(|s| s.as_str()))
    };

    // Single row insert
    if let Some(values) = &ir.values {
        let mut columns = Vec::new();
        let mut vals: Vec<SimpleExpr> = Vec::new();

        for (col, val) in values {
            columns.push(ColumnIdent(col.clone()));
            if let Some(expr) = json_to_simple_expr(val)? {
                vals.push(expr);
            } else {
                let col_type = get_col_type(col);
                vals.push(Expr::val(json_to_value_typed(val, col_type)).into());
            }
        }

        query.columns(columns);
        query.values(vals)?;
    }
    // Bulk insert
    else if let Some(bulk_values) = &ir.bulk_values {
        if bulk_values.is_empty() {
            return Err(QueryError::InvalidQuery(
                "Bulk insert requires at least one row".into(),
            ));
        }

        // Extract columns from first row
        let first_row = &bulk_values[0];
        let mut columns: Vec<ColumnIdent> =
            first_row.keys().map(|k| ColumnIdent(k.clone())).collect();
        columns.sort_by(|a, b| a.0.cmp(&b.0));

        query.columns(columns.clone());

        // Add each row
        for row in bulk_values {
            let mut vals: Vec<SimpleExpr> = Vec::new();
            for col in &columns {
                let val = row.get(&col.0).ok_or_else(|| {
                    QueryError::InvalidQuery(format!(
                        "Missing column '{}' in bulk insert row",
                        col.0
                    ))
                })?;
                if let Some(expr) = json_to_simple_expr(val)? {
                    vals.push(expr);
                } else {
                    let col_type = get_col_type(&col.0);
                    vals.push(Expr::val(json_to_value_typed(val, col_type)).into());
                }
            }
            query.values(vals)?;
        }
    }

    // Add ON CONFLICT clause (UPSERT)
    if let Some(on_conflict) = &ir.on_conflict {
        match dialect {
            Dialect::Postgres | Dialect::Sqlite => {
                // Postgres & SQLite: ON CONFLICT (columns) DO NOTHING/UPDATE
                let target_cols: Vec<ColumnIdent> = on_conflict
                    .columns
                    .iter()
                    .map(|c| ColumnIdent(c.clone()))
                    .collect();

                match on_conflict.action {
                    ConflictAction::Nothing => {
                        query.on_conflict(
                            sea_query::OnConflict::columns(target_cols)
                                .do_nothing()
                                .to_owned(),
                        );
                    }
                    ConflictAction::Update => {
                        // Require update_values for Update action
                        let update_vals = on_conflict.update_values.as_ref().ok_or_else(|| {
                            QueryError::InvalidQuery(
                                "ON CONFLICT UPDATE requires update_values".into(),
                            )
                        })?;

                        if update_vals.is_empty() {
                            return Err(QueryError::InvalidQuery(
                                "ON CONFLICT UPDATE requires at least one value to update".into(),
                            ));
                        }

                        let mut conflict = sea_query::OnConflict::columns(target_cols);
                        for (col, val) in update_vals {
                            if let Some(expr) = json_to_simple_expr(val)? {
                                conflict.value(ColumnIdent(col.clone()), expr);
                            } else {
                                let col_type = get_col_type(col);
                                conflict.value(
                                    ColumnIdent(col.clone()),
                                    Expr::val(json_to_value_typed(val, col_type)),
                                );
                            }
                        }

                        query.on_conflict(conflict.to_owned());
                    }
                }
            }
            Dialect::Mysql => {
                // MySQL uses ON DUPLICATE KEY UPDATE instead of ON CONFLICT
                // Note: MySQL doesn't support specifying conflict columns - it uses
                // PRIMARY KEY and UNIQUE indexes automatically
                match on_conflict.action {
                    ConflictAction::Nothing => {
                        // MySQL: ON DUPLICATE KEY UPDATE generates "do nothing" behavior
                        // by updating a column to itself (e.g., id = id)
                        // This is cleaner than INSERT IGNORE which suppresses all errors
                        if let Some(first_col) = ir
                            .values
                            .as_ref()
                            .and_then(|v| v.keys().next())
                            .or_else(|| {
                                ir.bulk_values
                                    .as_ref()
                                    .and_then(|bv| bv.first())
                                    .and_then(|row| row.keys().next())
                            })
                        {
                            let mut conflict = sea_query::OnConflict::new();
                            // Update column to itself = no-op
                            conflict.value(
                                ColumnIdent(first_col.clone()),
                                Expr::col(ColumnIdent(first_col.clone())),
                            );
                            query.on_conflict(conflict.to_owned());
                        }
                    }
                    ConflictAction::Update => {
                        // MySQL: ON DUPLICATE KEY UPDATE col1 = VALUES(col1), ...
                        let update_vals = on_conflict.update_values.as_ref().ok_or_else(|| {
                            QueryError::InvalidQuery(
                                "ON CONFLICT UPDATE requires update_values".into(),
                            )
                        })?;

                        if update_vals.is_empty() {
                            return Err(QueryError::InvalidQuery(
                                "ON CONFLICT UPDATE requires at least one value to update".into(),
                            ));
                        }

                        let mut conflict = sea_query::OnConflict::new();
                        for (col, val) in update_vals {
                            if let Some(expr) = json_to_simple_expr(val)? {
                                conflict.value(ColumnIdent(col.clone()), expr);
                            } else {
                                let col_type = get_col_type(col);
                                conflict.value(
                                    ColumnIdent(col.clone()),
                                    Expr::val(json_to_value_typed(val, col_type)),
                                );
                            }
                        }
                        query.on_conflict(conflict.to_owned());
                    }
                }
            }
        }
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
