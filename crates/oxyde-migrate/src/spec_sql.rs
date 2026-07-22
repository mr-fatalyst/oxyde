//! `ColumnTypeSpec`-driven SQL type resolution.
//!
//! The single DDL type resolver: semantic kind from `ColumnTypeSpec`,
//! verbatim user DDL from `FieldDef.db_type` (translated only for the
//! SERIAL family). Byte-equality with the historical output is pinned by
//! the golden DDL suite.
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
    if contains_enum_spec(spec) {
        return canonical_type(spec, dialect, is_pk);
    }
    if let Some(db_type) = db_type {
        return translate_user_db_type(db_type, dialect);
    }
    canonical_type(spec, dialect, is_pk)
}

fn contains_enum_spec(spec: &ColumnTypeSpec) -> bool {
    match spec {
        ColumnTypeSpec::Enum { .. } => true,
        ColumnTypeSpec::Array { item } => contains_enum_spec(item),
        _ => false,
    }
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
        S::Enum { name, values } => match dialect {
            D::Postgres => quote_postgres_type_name(name),
            D::Mysql => format!(
                "ENUM({})",
                values
                    .iter()
                    .map(|value| quote_sql_string(value))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
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

pub(crate) fn quote_postgres_type_name(name: &str) -> String {
    name.split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

pub(crate) fn quote_sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_string(length: Option<u32>) -> ColumnTypeSpec {
        ColumnTypeSpec::String { length }
    }

    /// Canonical renderings, pinned directly (the golden DDL suite pins the
    /// full statements; this covers the resolver in isolation).
    #[test]
    fn canonical_scalar_types() {
        use ColumnTypeSpec as S;
        let cases: Vec<(ColumnTypeSpec, [&str; 3])> = vec![
            // (spec, [postgres, mysql, sqlite])
            (S::BigInteger, ["INTEGER", "BIGINT", "INTEGER"]),
            (S::Double, ["DOUBLE PRECISION", "DOUBLE", "REAL"]),
            (S::Boolean, ["BOOLEAN", "TINYINT", "INTEGER"]),
            (
                spec_string(None),
                ["VARCHAR(255)", "VARCHAR(255)", "VARCHAR(255)"],
            ),
            (
                spec_string(Some(100)),
                ["VARCHAR(100)", "VARCHAR(100)", "VARCHAR(100)"],
            ),
            (S::Blob, ["BYTEA", "LONGBLOB", "BLOB"]),
            (S::DateTime, ["TIMESTAMP", "DATETIME(6)", "TEXT"]),
            (S::DateTimeUtc, ["TIMESTAMPTZ", "DATETIME(6)", "TEXT"]),
            (S::Date, ["DATE", "DATE", "TEXT"]),
            (S::Time, ["TIME", "TIME(6)", "TEXT"]),
            (S::Timedelta, ["BIGINT", "BIGINT", "BIGINT"]),
            (S::Uuid, ["UUID", "CHAR(36)", "TEXT"]),
            (
                S::Decimal {
                    precision: None,
                    scale: None,
                },
                ["NUMERIC", "DECIMAL", "TEXT"],
            ),
            (
                S::Decimal {
                    precision: Some(10),
                    scale: Some(2),
                },
                ["NUMERIC(10,2)", "DECIMAL(10,2)", "TEXT"],
            ),
            (S::Json, ["JSONB", "JSON", "TEXT"]),
            (S::JsonBinary, ["JSONB", "JSON", "TEXT"]),
            (S::Unknown, ["TEXT", "TEXT", "TEXT"]),
        ];
        let dialects = [Dialect::Postgres, Dialect::Mysql, Dialect::Sqlite];
        for (spec, expected) in &cases {
            for (dialect, want) in dialects.iter().zip(expected) {
                assert_eq!(
                    &resolve_spec_type(spec, None, *dialect, false),
                    want,
                    "spec={spec:?} dialect={dialect:?}"
                );
            }
        }
    }

    #[test]
    fn int_pk_is_serial_only_on_postgres() {
        assert_eq!(
            resolve_spec_type(&ColumnTypeSpec::BigInteger, None, Dialect::Postgres, true),
            "BIGSERIAL"
        );
        assert_eq!(
            resolve_spec_type(&ColumnTypeSpec::BigInteger, None, Dialect::Mysql, true),
            "BIGINT"
        );
        assert_eq!(
            resolve_spec_type(&ColumnTypeSpec::BigInteger, None, Dialect::Sqlite, true),
            "INTEGER"
        );
    }

    #[test]
    fn arrays_render_per_dialect() {
        let arr = ColumnTypeSpec::Array {
            item: Box::new(ColumnTypeSpec::Uuid),
        };
        assert_eq!(
            resolve_spec_type(&arr, None, Dialect::Postgres, false),
            "UUID[]"
        );
        assert_eq!(resolve_spec_type(&arr, None, Dialect::Mysql, false), "JSON");
        assert_eq!(
            resolve_spec_type(&arr, None, Dialect::Sqlite, false),
            "TEXT"
        );
    }

    #[test]
    fn user_db_type_wins_verbatim_with_serial_translation() {
        let spec = ColumnTypeSpec::Unknown;
        assert_eq!(
            resolve_spec_type(&spec, Some("MONEY"), Dialect::Mysql, false),
            "MONEY"
        );
        assert_eq!(
            resolve_spec_type(&spec, Some("SERIAL"), Dialect::Sqlite, false),
            "INTEGER"
        );
        assert_eq!(
            resolve_spec_type(&spec, Some("BIGSERIAL"), Dialect::Mysql, false),
            "BIGINT"
        );
        assert_eq!(
            resolve_spec_type(&spec, Some("SERIAL"), Dialect::Postgres, false),
            "SERIAL"
        );
    }

    #[test]
    fn enum_type_rendering() {
        let spec = ColumnTypeSpec::Enum {
            name: "post_status_enum".to_string(),
            values: vec!["draft".to_string(), "published".to_string()],
        };
        assert_eq!(
            resolve_spec_type(&spec, None, Dialect::Postgres, false),
            r#""post_status_enum""#
        );
        assert_eq!(
            resolve_spec_type(&spec, None, Dialect::Mysql, false),
            "ENUM('draft','published')"
        );
        assert_eq!(
            resolve_spec_type(&spec, None, Dialect::Sqlite, false),
            "TEXT"
        );
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
