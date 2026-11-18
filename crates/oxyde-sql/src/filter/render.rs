//! SQL templates over a filtered column.

use sea_query::{Expr, SimpleExpr};

use crate::Dialect;

/// Render a template whose `{col}` markers stand for the filtered column.
///
/// PostgreSQL references it by number, so repeats render the same
/// expression; `?` dialects consume one value per placeholder, so the column
/// is pushed once per marker.
pub(super) fn render_col(template: &str, col: &SimpleExpr, dialect: Dialect) -> SimpleExpr {
    if dialect == Dialect::Postgres {
        return Expr::cust_with_exprs(template.replace("{col}", "$1"), [col.clone()]);
    }
    let parts: Vec<&str> = template.split("{col}").collect();
    let values = vec![col.clone(); parts.len() - 1];
    Expr::cust_with_exprs(parts.join("?"), values)
}
