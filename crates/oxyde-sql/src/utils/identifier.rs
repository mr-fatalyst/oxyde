//! Dynamic SQL identifiers for sea_query

use sea_query::Iden;

/// Dynamic table identifier
#[derive(Debug, Clone)]
pub struct TableIdent(pub String);

impl Iden for TableIdent {
    fn unquoted(&self, s: &mut dyn std::fmt::Write) {
        // Write should never fail for in-memory strings
        // If it does fail, we silently ignore it as there's no recovery path
        // and the error will propagate when sea_query tries to use the incomplete SQL
        let _ = write!(s, "{}", self.0);
    }
}

/// Dynamic column identifier
#[derive(Debug, Clone)]
pub struct ColumnIdent(pub String);

impl Iden for ColumnIdent {
    fn unquoted(&self, s: &mut dyn std::fmt::Write) {
        // Write should never fail for in-memory strings
        // If it does fail, we silently ignore it as there's no recovery path
        // and the error will propagate when sea_query tries to use the incomplete SQL
        let _ = write!(s, "{}", self.0);
    }
}
