//! Aggregate function building

use oxyde_codec::{Aggregate, AggregateOp};
use sea_query::{Asterisk, Expr, Func, SimpleExpr};

use crate::error::{QueryError, Result};
use crate::utils::ColumnIdent;

/// Build aggregate expression from Aggregate specification
pub fn build_aggregate(agg: &Aggregate) -> Result<SimpleExpr> {
    let expr =
        match &agg.op {
            AggregateOp::Count => {
                if let Some(field) = &agg.field {
                    if field == "*" {
                        // COUNT(*) - use Asterisk, not column name
                        Func::count(Expr::col(Asterisk)).into()
                    } else {
                        // COUNT(column_name)
                        Func::count(Expr::col(ColumnIdent(field.clone()))).into()
                    }
                } else {
                    // COUNT(*) - fallback
                    Func::count(Expr::col(Asterisk)).into()
                }
            }
            AggregateOp::Sum => {
                let field = agg.field.as_ref().ok_or_else(|| {
                    QueryError::InvalidQuery("SUM aggregate requires field".into())
                })?;
                Func::sum(Expr::col(ColumnIdent(field.clone()))).into()
            }
            AggregateOp::Avg => {
                let field = agg.field.as_ref().ok_or_else(|| {
                    QueryError::InvalidQuery("AVG aggregate requires field".into())
                })?;
                Func::avg(Expr::col(ColumnIdent(field.clone()))).into()
            }
            AggregateOp::Max => {
                let field = agg.field.as_ref().ok_or_else(|| {
                    QueryError::InvalidQuery("MAX aggregate requires field".into())
                })?;
                Func::max(Expr::col(ColumnIdent(field.clone()))).into()
            }
            AggregateOp::Min => {
                let field = agg.field.as_ref().ok_or_else(|| {
                    QueryError::InvalidQuery("MIN aggregate requires field".into())
                })?;
                Func::min(Expr::col(ColumnIdent(field.clone()))).into()
            }
        };
    Ok(expr)
}
