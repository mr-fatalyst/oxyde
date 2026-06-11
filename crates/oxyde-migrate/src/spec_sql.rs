//! `ColumnTypeSpec`-driven SQL type resolution.
//!
//! Successor of `sql.rs`'s string-driven `resolve_field_type` /
//! `python_type_to_sql` / `translate_db_type`. During этап 1 of the
//! type-mapping refactor both paths coexist; the differential test below
//! proves byte-equality against the legacy resolver for every type the
//! golden DDL suite covers.
//!
//! Like the legacy path, this resolver produces the SQL type **string**
//! (rendered via `ColumnDef::custom`), not a `sea_query::ColumnType`
//! variant: sea-query's own per-dialect defaults differ from Oxyde's
//! contract (e.g. `ColumnType::Uuid` → `binary(16)` on MySQL, while the
//! whole bind/decode pipeline speaks CHAR(36)).

use oxyde_codec::ColumnTypeSpec;

use crate::types::Dialect;

/// Resolve the SQL type string for a column.
///
/// Priority (same as legacy `resolve_field_type`):
/// 1. `db_type` — user-supplied verbatim DDL, translated only for the
///    SERIAL family (the user owns the string otherwise);
/// 2. canonical per-dialect rendering of the spec.
pub fn resolve_spec_type(
    spec: &ColumnTypeSpec,
    db_type: Option<&str>,
    dialect: Dialect,
    is_pk: bool,
) -> String {
    if let Some(db_type) = db_type {
        return translate_user_db_type(db_type, dialect);
    }
    canonical_type(spec, dialect, is_pk)
}

/// SERIAL/BIGSERIAL are PostgreSQL-specific — translate for other dialects.
/// Everything else renders verbatim: the user owns the string.
fn translate_user_db_type(db_type: &str, dialect: Dialect) -> String {
    let upper = db_type.to_uppercase();
    match (upper.as_str(), dialect) {
        ("SERIAL" | "BIGSERIAL", Dialect::Sqlite) => "INTEGER".to_string(),
        ("SERIAL", Dialect::Mysql) => "INT".to_string(),
        ("BIGSERIAL", Dialect::Mysql) => "BIGINT".to_string(),
        _ => db_type.to_string(),
    }
}

/// Canonical per-dialect SQL type for a spec (no user override).
fn canonical_type(spec: &ColumnTypeSpec, dialect: Dialect, is_pk: bool) -> String {
    use ColumnTypeSpec as S;
    use Dialect as D;

    match spec {
        // Python str carries VARCHAR semantics; 255 is the legacy default.
        S::String { length } => format!("VARCHAR({})", length.unwrap_or(255)),
        S::Text => "TEXT".to_string(),

        S::BigInteger => match (dialect, is_pk) {
            (D::Postgres, true) => "BIGSERIAL".to_string(),
            (D::Postgres, false) => "INTEGER".to_string(),
            (D::Mysql, _) => "BIGINT".to_string(),
            (D::Sqlite, _) => "INTEGER".to_string(),
        },
        S::Double => match dialect {
            D::Postgres => "DOUBLE PRECISION".to_string(),
            D::Mysql => "DOUBLE".to_string(),
            D::Sqlite => "REAL".to_string(),
        },
        S::Boolean => match dialect {
            D::Postgres => "BOOLEAN".to_string(),
            D::Mysql => "TINYINT".to_string(),
            D::Sqlite => "INTEGER".to_string(),
        },
        S::Blob => match dialect {
            D::Postgres => "BYTEA".to_string(),
            D::Mysql => "LONGBLOB".to_string(),
            D::Sqlite => "BLOB".to_string(),
        },
        S::DateTime => match dialect {
            D::Postgres => "TIMESTAMP".to_string(),
            D::Mysql => "DATETIME(6)".to_string(),
            D::Sqlite => "TEXT".to_string(),
        },
        S::DateTimeUtc => match dialect {
            D::Postgres => "TIMESTAMPTZ".to_string(),
            D::Mysql => "DATETIME(6)".to_string(),
            D::Sqlite => "TEXT".to_string(),
        },
        S::Date => match dialect {
            D::Postgres | D::Mysql => "DATE".to_string(),
            D::Sqlite => "TEXT".to_string(),
        },
        S::Time => match dialect {
            D::Postgres => "TIME".to_string(),
            D::Mysql => "TIME(6)".to_string(),
            D::Sqlite => "TEXT".to_string(),
        },
        S::Timedelta => "BIGINT".to_string(),
        S::Uuid => match dialect {
            D::Postgres => "UUID".to_string(),
            D::Mysql => "CHAR(36)".to_string(),
            D::Sqlite => "TEXT".to_string(),
        },
        S::Decimal { precision, scale } => match dialect {
            D::Sqlite => "TEXT".to_string(),
            D::Postgres => match precision {
                Some(p) => format!("NUMERIC({},{})", p, scale.unwrap_or(0)),
                None => "NUMERIC".to_string(),
            },
            D::Mysql => match precision {
                Some(p) => format!("DECIMAL({},{})", p, scale.unwrap_or(0)),
                None => "DECIMAL".to_string(),
            },
        },
        S::Json | S::JsonBinary => match dialect {
            D::Postgres => "JSONB".to_string(),
            D::Mysql => "JSON".to_string(),
            D::Sqlite => "TEXT".to_string(),
        },
        S::Array { item } => match dialect {
            D::Postgres => format!("{}[]", canonical_type(item, dialect, false)),
            D::Mysql => "JSON".to_string(),
            D::Sqlite => "TEXT".to_string(),
        },
        S::Unknown => "TEXT".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::resolve_field_type;
    use crate::types::FieldDef;

    fn field(python_type: &str) -> FieldDef {
        FieldDef {
            name: "col".into(),
            python_type: python_type.into(),
            db_type: None,
            nullable: false,
            primary_key: false,
            unique: false,
            default: None,
            auto_increment: false,
            max_length: None,
            max_digits: None,
            decimal_places: None,
        }
    }

    fn spec_string(length: Option<u32>) -> ColumnTypeSpec {
        ColumnTypeSpec::String { length }
    }

    /// Differential check: while both resolvers are alive, the new one must
    /// produce byte-identical SQL type strings for every fixture the golden
    /// DDL suite covers, across all dialects.
    #[test]
    fn matches_legacy_resolver() {
        use ColumnTypeSpec as S;

        // (legacy FieldDef, equivalent spec)
        let mut cases: Vec<(FieldDef, ColumnTypeSpec)> = vec![
            (field("int"), S::BigInteger),
            (field("str"), spec_string(None)),
            (
                {
                    let mut f = field("str");
                    f.max_length = Some(100);
                    f
                },
                spec_string(Some(100)),
            ),
            (field("float"), S::Double),
            (field("bool"), S::Boolean),
            (field("bytes"), S::Blob),
            (field("datetime"), S::DateTime),
            (field("date"), S::Date),
            (field("time"), S::Time),
            (field("timedelta"), S::Timedelta),
            (field("uuid"), S::Uuid),
            (
                field("decimal"),
                S::Decimal {
                    precision: None,
                    scale: None,
                },
            ),
            (
                {
                    let mut f = field("decimal");
                    f.max_digits = Some(10);
                    f.decimal_places = Some(2);
                    f
                },
                S::Decimal {
                    precision: Some(10),
                    scale: Some(2),
                },
            ),
            (field("json"), S::Json),
            (field("custom_thing"), S::Unknown),
            (
                field("int[]"),
                S::Array {
                    item: Box::new(S::BigInteger),
                },
            ),
            (
                field("str[]"),
                S::Array {
                    item: Box::new(spec_string(None)),
                },
            ),
            (
                field("uuid[]"),
                S::Array {
                    item: Box::new(S::Uuid),
                },
            ),
            (
                field("decimal[]"),
                S::Array {
                    item: Box::new(S::Decimal {
                        precision: None,
                        scale: None,
                    }),
                },
            ),
        ];

        // db_type overrides from the golden suite (incl. unknown MONEY)
        for db_type in [
            "SERIAL",
            "BIGSERIAL",
            "JSONB",
            "TIMESTAMPTZ",
            "NUMERIC(10,2)",
            "VARCHAR(100)",
            "CHAR(36)",
            "MONEY",
        ] {
            let mut f = field("int");
            f.db_type = Some(db_type.to_string());
            // kind is irrelevant when db_type wins; Unknown is what Python
            // would send for MONEY, BigInteger for SERIAL — both must agree.
            cases.push((f, S::Unknown));
        }

        for dialect in [Dialect::Postgres, Dialect::Mysql, Dialect::Sqlite] {
            for (f, spec) in &cases {
                for is_pk in [false, true] {
                    // Conscious deviation from legacy: an int-array PK used to
                    // render `BIGSERIAL[]` (is_pk leaked into the element type)
                    // — invalid SQL that PG rejects, so no working setup can
                    // depend on it. The new resolver never marks array
                    // elements as PK; skip that pathological combination here.
                    if is_pk && matches!(spec, S::Array { .. }) {
                        continue;
                    }
                    let mut f = f.clone();
                    f.primary_key = is_pk;
                    let legacy = resolve_field_type(&f, dialect);
                    let new = resolve_spec_type(spec, f.db_type.as_deref(), dialect, is_pk);
                    assert_eq!(
                        new, legacy,
                        "divergence: python_type={} db_type={:?} dialect={dialect:?} is_pk={is_pk}",
                        f.python_type, f.db_type
                    );
                }
            }
        }
    }

    #[test]
    fn pk_str_and_uuid_do_not_become_serial() {
        assert_eq!(
            resolve_spec_type(&spec_string(Some(32)), None, Dialect::Postgres, true),
            "VARCHAR(32)"
        );
        assert_eq!(
            resolve_spec_type(&ColumnTypeSpec::Uuid, None, Dialect::Postgres, true),
            "UUID"
        );
    }
}
