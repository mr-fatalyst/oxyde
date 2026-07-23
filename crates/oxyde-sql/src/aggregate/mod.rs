//! Aggregate function building

use oxyde_codec::{Aggregate, AggregateOp};
use sea_query::{Asterisk, Expr, Func, SimpleExpr};

use crate::error::{QueryError, Result};
use crate::utils::ColumnIdent;

/// Build aggregate expression from Aggregate specification
pub fn build_aggregate(agg: &Aggregate) -> Result<SimpleExpr> {
    let distinct = agg.distinct.unwrap_or(false);
    let expr =
        match &agg.op {
            AggregateOp::Count => {
                let col = match &agg.field {
                    Some(f) if f != "*" => Expr::col(ColumnIdent(f.clone())),
                    _ => Expr::col(Asterisk),
                };
                if distinct && agg.field.as_deref() != Some("*") {
                    Func::count_distinct(col).into()
                } else {
                    Func::count(col).into()
                }
            }
            AggregateOp::Sum => {
                let field = agg.field.as_ref().ok_or_else(|| {
                    QueryError::InvalidQuery("SUM aggregate requires field".into())
                })?;
                let col = Expr::col(ColumnIdent(field.clone()));
                if distinct {
                    Expr::cust_with_expr("SUM(DISTINCT $1)", col)
                } else {
                    Func::sum(col).into()
                }
            }
            AggregateOp::Avg => {
                let field = agg.field.as_ref().ok_or_else(|| {
                    QueryError::InvalidQuery("AVG aggregate requires field".into())
                })?;
                let col = Expr::col(ColumnIdent(field.clone()));
                if distinct {
                    Expr::cust_with_expr("AVG(DISTINCT $1)", col)
                } else {
                    Func::avg(col).into()
                }
            }
            AggregateOp::Max => {
                let field = agg.field.as_ref().ok_or_else(|| {
                    QueryError::InvalidQuery("MAX aggregate requires field".into())
                })?;
                // DISTINCT is meaningless for MAX
                Func::max(Expr::col(ColumnIdent(field.clone()))).into()
            }
            AggregateOp::Min => {
                let field = agg.field.as_ref().ok_or_else(|| {
                    QueryError::InvalidQuery("MIN aggregate requires field".into())
                })?;
                // DISTINCT is meaningless for MIN
                Func::min(Expr::col(ColumnIdent(field.clone()))).into()
            }
        };
    Ok(expr)
}
