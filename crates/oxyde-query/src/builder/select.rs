//! SELECT query building

use oxyde_codec::{JoinSpec, LockType as OxydeLockType, QueryIR};
use sea_query::{
    Alias, Asterisk, ColumnRef, Expr, Func, LockType as SeaLockType, MysqlQueryBuilder, Order,
    PostgresQueryBuilder, Query, SeaRc, SqliteQueryBuilder, UnionType, Value,
};

use crate::aggregate::build_aggregate;
use crate::error::Result;
use crate::filter::build_filter_node;
use crate::utils::{renumber_postgres_placeholders, ColumnIdent, TableIdent};
use crate::Dialect;

/// Build SELECT query from QueryIR
pub fn build_select(ir: &QueryIR, dialect: Dialect) -> Result<(String, Vec<Value>)> {
    let table = TableIdent(ir.table.clone());
    let mut query = Query::select();
    query.from(table.clone());

    if ir.distinct.unwrap_or(false) {
        query.distinct();
    }

    // Add aggregates or columns
    if let Some(aggregates) = &ir.aggregates {
        // Add all aggregate expressions
        for agg in aggregates {
            let agg_expr = build_aggregate(agg)?;
            if let Some(alias) = &agg.alias {
                query.expr_as(agg_expr, Alias::new(alias.clone()));
            } else {
                query.expr(agg_expr);
            }
        }
        // If GROUP BY is present, also add grouped columns
        if let Some(group_by) = &ir.group_by {
            for field in group_by {
                let column_ref = ColumnRef::TableColumn(
                    SeaRc::new(table.clone()),
                    SeaRc::new(ColumnIdent(field.clone())),
                );
                query.expr_as(Expr::col(column_ref), Alias::new(field.clone()));
            }
        }
    } else if let Some(cols) = &ir.cols {
        for col in cols {
            let mapped = ir
                .column_mappings
                .as_ref()
                .and_then(|mappings| mappings.get(col))
                .cloned()
                .unwrap_or_else(|| col.clone());
            let column_ref = ColumnRef::TableColumn(
                SeaRc::new(table.clone()),
                SeaRc::new(ColumnIdent(mapped.clone())),
            );
            query.expr_as(Expr::col(column_ref), Alias::new(col.clone()));
        }
    } else {
        query.column(Asterisk);
    }

    // Determine default table for filter qualification (needed for JOINs to avoid ambiguity)
    let default_table = if ir.joins.is_some() {
        Some(ir.table.as_str())
    } else {
        None
    };

    // Add filters
    if let Some(filter_tree) = &ir.filter_tree {
        let expr = build_filter_node(filter_tree, default_table)?;
        query.and_where(expr);
    }

    // Add GROUP BY (with table qualification for JOINs to avoid ambiguity)
    if let Some(group_by) = &ir.group_by {
        for field in group_by {
            let col_expr = if ir.joins.is_some() {
                Expr::col(ColumnRef::TableColumn(
                    SeaRc::new(table.clone()),
                    SeaRc::new(ColumnIdent(field.clone())),
                ))
            } else {
                Expr::col(ColumnIdent(field.clone()))
            };
            query.add_group_by([col_expr.into()]);
        }
    }

    // Add HAVING
    if let Some(having) = &ir.having {
        let expr = build_filter_node(having, default_table)?;
        query.and_having(expr);
    }

    // Add order by (with table qualification for JOINs to avoid ambiguity)
    if let Some(order_by) = &ir.order_by {
        for (field, direction) in order_by {
            let order = match direction.to_uppercase().as_str() {
                "ASC" => Order::Asc,
                "DESC" => Order::Desc,
                _ => Order::Asc,
            };
            if ir.joins.is_some() {
                let col_ref = ColumnRef::TableColumn(
                    SeaRc::new(table.clone()),
                    SeaRc::new(ColumnIdent(field.clone())),
                );
                query.order_by(col_ref, order);
            } else {
                query.order_by(ColumnIdent(field.clone()), order);
            }
        }
    }

    // Add limit
    if let Some(limit) = ir.limit {
        query.limit(limit as u64);
    }

    // Add offset
    if let Some(offset) = ir.offset {
        query.offset(offset as u64);
    }

    if let Some(joins) = &ir.joins {
        apply_select_joins(&mut query, joins, &table)?;
    }

    // Add FOR UPDATE / FOR SHARE
    if let Some(lock_type) = &ir.lock {
        match lock_type {
            OxydeLockType::Update => query.lock(SeaLockType::Update),
            OxydeLockType::Share => query.lock(SeaLockType::Share),
        };
    }

    // Handle UNION
    if let Some(union_query_ir) = &ir.union_query {
        let (union_sql, union_values) = crate::build_sql(union_query_ir, dialect)?;
        let union_type = if ir.union_all.unwrap_or(false) {
            UnionType::All
        } else {
            UnionType::Distinct
        };

        // Build the UNION manually since sea_query's union support is limited
        let (base_sql, base_values) = match dialect {
            Dialect::Postgres => query.build(PostgresQueryBuilder),
            Dialect::Sqlite => query.build(SqliteQueryBuilder),
            Dialect::Mysql => query.build(MysqlQueryBuilder),
        };

        // Renumber placeholders in union query for PostgreSQL
        // PostgreSQL uses $1, $2, $3... so we need to offset the union query placeholders
        let union_sql_renumbered = match dialect {
            Dialect::Postgres => renumber_postgres_placeholders(&union_sql, base_values.0.len()),
            _ => union_sql, // MySQL and SQLite use ? placeholders, no renumbering needed
        };

        // Construct UNION manually since sea_query's union support is limited
        let union_keyword = match union_type {
            UnionType::All => "UNION ALL",
            UnionType::Distinct => "UNION",
            UnionType::Intersect => "INTERSECT",
            UnionType::Except => "EXCEPT",
        };
        let combined_sql = format!("{} {} {}", base_sql, union_keyword, union_sql_renumbered);
        let mut combined_values = base_values.0;
        combined_values.extend(union_values);

        // Handle EXISTS wrapping if needed
        if ir.exists.unwrap_or(false) {
            let exists_sql = format!("SELECT EXISTS({})", combined_sql);
            return Ok((exists_sql, combined_values));
        }

        return Ok((combined_sql, combined_values));
    }

    // Handle COUNT(*) - short path
    if ir.count.unwrap_or(false) {
        // Build minimal query: SELECT COUNT(*) FROM table WHERE ...
        let mut count_query = Query::select();
        count_query.from(table.clone());
        count_query.expr_as(Func::count(Expr::col(Asterisk)), Alias::new("_count"));

        // Determine default table for filter qualification (needed for JOINs)
        let count_default_table = if ir.joins.is_some() {
            Some(ir.table.as_str())
        } else {
            None
        };

        // Add filters
        if let Some(filter_tree) = &ir.filter_tree {
            let expr = build_filter_node(filter_tree, count_default_table)?;
            count_query.and_where(expr);
        }

        // Add joins if needed for filtering (without columns - only JOIN clause)
        if let Some(joins) = &ir.joins {
            apply_joins_only(&mut count_query, joins, &table)?;
        }

        let (sql, values) = match dialect {
            Dialect::Postgres => count_query.build(PostgresQueryBuilder),
            Dialect::Sqlite => count_query.build(SqliteQueryBuilder),
            Dialect::Mysql => count_query.build(MysqlQueryBuilder),
        };
        return Ok((sql, values.0));
    }

    // Handle EXISTS wrapping
    if ir.exists.unwrap_or(false) {
        let (base_sql, base_values) = match dialect {
            Dialect::Postgres => query.build(PostgresQueryBuilder),
            Dialect::Sqlite => query.build(SqliteQueryBuilder),
            Dialect::Mysql => query.build(MysqlQueryBuilder),
        };
        let exists_sql = format!("SELECT EXISTS({})", base_sql);
        return Ok((exists_sql, base_values.0));
    }

    let (sql, values) = match dialect {
        Dialect::Postgres => query.build(PostgresQueryBuilder),
        Dialect::Sqlite => query.build(SqliteQueryBuilder),
        Dialect::Mysql => query.build(MysqlQueryBuilder),
    };

    Ok((sql, values.0))
}

fn apply_select_joins(
    query: &mut sea_query::SelectStatement,
    joins: &[JoinSpec],
    base_table: &TableIdent,
) -> Result<()> {
    for join in joins {
        let join_alias = Alias::new(join.alias.clone());
        let mut table_ref = sea_query::TableRef::Table(SeaRc::new(TableIdent(join.table.clone())));
        table_ref = table_ref.alias(join_alias.clone());
        let left_col = match &join.parent {
            Some(parent_alias) => ColumnRef::TableColumn(
                SeaRc::new(Alias::new(parent_alias.clone())),
                SeaRc::new(ColumnIdent(join.source_column.clone())),
            ),
            None => ColumnRef::TableColumn(
                SeaRc::new(base_table.clone()),
                SeaRc::new(ColumnIdent(join.source_column.clone())),
            ),
        };
        let right_col = ColumnRef::TableColumn(
            SeaRc::new(join_alias.clone()),
            SeaRc::new(ColumnIdent(join.target_column.clone())),
        );
        query.left_join(table_ref, Expr::col(left_col).equals(right_col));
        for column in &join.columns {
            let expr = Expr::col((join_alias.clone(), ColumnIdent(column.column.clone())));
            let alias = Alias::new(format!("{}__{}", join.result_prefix, column.field));
            query.expr_as(expr, alias);
        }
    }
    Ok(())
}

/// Apply JOINs without adding columns to SELECT (for COUNT queries)
fn apply_joins_only(
    query: &mut sea_query::SelectStatement,
    joins: &[JoinSpec],
    base_table: &TableIdent,
) -> Result<()> {
    for join in joins {
        let join_alias = Alias::new(join.alias.clone());
        let mut table_ref = sea_query::TableRef::Table(SeaRc::new(TableIdent(join.table.clone())));
        table_ref = table_ref.alias(join_alias.clone());
        let left_col = match &join.parent {
            Some(parent_alias) => ColumnRef::TableColumn(
                SeaRc::new(Alias::new(parent_alias.clone())),
                SeaRc::new(ColumnIdent(join.source_column.clone())),
            ),
            None => ColumnRef::TableColumn(
                SeaRc::new(base_table.clone()),
                SeaRc::new(ColumnIdent(join.source_column.clone())),
            ),
        };
        let right_col = ColumnRef::TableColumn(
            SeaRc::new(join_alias.clone()),
            SeaRc::new(ColumnIdent(join.target_column.clone())),
        );
        query.left_join(table_ref, Expr::col(left_col).equals(right_col));
        // Note: no columns added - only JOIN clause for filtering
    }
    Ok(())
}
