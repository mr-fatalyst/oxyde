//! Query builder errors

use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),

    #[error("SQL generation error: {0}")]
    SqlError(String),
}

pub type Result<T> = std::result::Result<T, QueryError>;

impl From<sea_query::error::Error> for QueryError {
    fn from(err: sea_query::error::Error) -> Self {
        QueryError::SqlError(err.to_string())
    }
}
