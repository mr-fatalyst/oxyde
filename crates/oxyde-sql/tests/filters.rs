//! SQL generation for filters on array and JSON columns across dialects, and
//! for HAVING conditions on aggregates.

use std::collections::HashMap;

use oxyde_codec::{Aggregate, AggregateOp, ColumnTypeSpec, Filter, FilterNode, QueryIR};
use oxyde_sql::{build_sql, Dialect};
use rust_decimal::Decimal;
use sea_query::Value;

fn array_of(item: ColumnTypeSpec) -> ColumnTypeSpec {
    ColumnTypeSpec::Array {
        item: Box::new(item),
    }
}

fn s(v: &str) -> rmpv::Value {
    rmpv::Value::String(v.into())
}

fn arr(items: Vec<rmpv::Value>) -> rmpv::Value {
    rmpv::Value::Array(items)
}

fn filter(operator: &str, value: rmpv::Value) -> FilterNode {
    FilterNode::Condition(Filter {
        field: "tags".into(),
        operator: operator.into(),
        value,
        column: None,
        escape: None,
    })
}

/// One condition on `posts.tags`, whose spec is `spec`.
struct Condition {
    operator: &'static str,
    value: rmpv::Value,
    spec: ColumnTypeSpec,
    db_type: Option<&'static str>,
}

impl Condition {
    fn new(operator: &'static str, value: rmpv::Value, spec: ColumnTypeSpec) -> Self {
        Self {
            operator,
            value,
            spec,
            db_type: None,
        }
    }

    fn ir(&self) -> QueryIR {
        QueryIR {
            table: "posts".into(),
            cols: Some(vec!["id".into()]),
            column_types: Some(HashMap::from([("tags".into(), self.spec.clone())])),
            column_db_types: self
                .db_type
                .map(|db_type| HashMap::from([("tags".into(), db_type.into())])),
            filter_tree: Some(filter(self.operator, self.value.clone())),
            ..Default::default()
        }
    }

    fn build(&self, dialect: Dialect) -> (String, Vec<Value>) {
        build_sql(&self.ir(), dialect).unwrap()
    }

    fn error(&self, dialect: Dialect) -> String {
        build_sql(&self.ir(), dialect).unwrap_err().to_string()
    }
}

fn array_override(
    operator: &'static str,
    value: rmpv::Value,
    item: ColumnTypeSpec,
    db_type: &'static str,
) -> Condition {
    let mut condition = Condition::new(operator, value, array_of(item));
    condition.db_type = Some(db_type);
    condition
}

fn int_array_override(operator: &'static str, value: rmpv::Value) -> Condition {
    array_override(operator, value, ColumnTypeSpec::BigInteger, "INTEGER[]")
}

#[test]
fn postgres_casts_array_values_to_the_override_for_every_operator() {
    let ints = || arr(vec![1.into(), 2.into()]);
    for (operator, sql_op) in [("=", "="), ("!=", "<>"), ("<", "<"), (">=", ">=")] {
        let (sql, params) = int_array_override(operator, ints()).build(Dialect::Postgres);
        assert!(
            sql.ends_with(&format!(r#""tags" {sql_op} ($1::INTEGER[])"#)),
            "{sql}"
        );
        assert_eq!(params.len(), 1);
    }
    let (sql, params) =
        int_array_override("IN", arr(vec![ints(), arr(vec![3.into()])])).build(Dialect::Postgres);
    assert!(
        sql.ends_with(r#""tags" IN ($1::INTEGER[], $2::INTEGER[])"#),
        "{sql}"
    );
    assert_eq!(params.len(), 2);

    // Other dialects store arrays as JSON: no cast.
    for dialect in [Dialect::Mysql, Dialect::Sqlite] {
        let (sql, _) = int_array_override("=", ints()).build(dialect);
        assert!(!sql.contains("INTEGER[]"), "{dialect:?}: {sql}");
    }
}

#[test]
fn values_the_array_type_cannot_hold_equal_nothing() {
    let too_big = || arr(vec![1.into(), (1_i64 << 31).into()]);

    // `=` is false and `!=` true, NULL for a NULL column either way.
    let (sql, params) = int_array_override("=", too_big()).build(Dialect::Postgres);
    assert!(
        sql.ends_with(r#"(CASE WHEN "tags" IS NULL THEN NULL ELSE FALSE END)"#),
        "{sql}"
    );
    assert!(params.is_empty());
    let (sql, _) = int_array_override("!=", too_big()).build(Dialect::Postgres);
    assert!(
        sql.ends_with(r#"(CASE WHEN "tags" IS NULL THEN NULL ELSE TRUE END)"#),
        "{sql}"
    );

    // IN drops them, and matches nothing when none are left.
    let (sql, params) = int_array_override("IN", arr(vec![arr(vec![1.into()]), too_big()]))
        .build(Dialect::Postgres);
    assert!(sql.ends_with(r#""tags" IN ($1::INTEGER[])"#), "{sql}");
    assert_eq!(params.len(), 1);
    let (sql, _) = int_array_override("IN", arr(vec![too_big()])).build(Dialect::Postgres);
    assert!(sql.ends_with("ELSE FALSE END)"), "{sql}");

    // An ordering against one has no clear answer.
    for operator in ["<", ">="] {
        let err = int_array_override(operator, too_big()).error(Dialect::Postgres);
        assert!(
            err.contains("2147483648 is out of range for INTEGER[]"),
            "{err}"
        );
    }

    // NaN and the infinities fit REAL; 1e300 does not.
    let real = |x: f64| {
        array_override(
            "=",
            arr(vec![rmpv::Value::F64(x)]),
            ColumnTypeSpec::Double,
            "REAL[]",
        )
    };
    for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let (sql, _) = real(x).build(Dialect::Postgres);
        assert!(sql.ends_with(r#""tags" = ($1::REAL[])"#), "{x}: {sql}");
    }
    let (sql, _) = real(1e300).build(Dialect::Postgres);
    assert!(sql.ends_with("ELSE FALSE END)"), "{sql}");
}

#[test]
fn array_elements_of_another_type_are_an_error_on_postgres() {
    // PostgreSQL would bind each of these elements as NULL, and NULL
    // elements compare equal there, so the filter would match rows holding
    // [None]. MySQL and SQLite encode each element of a JSON array as
    // given, exactly as a saved value, so a union element type
    // (`list[str | int]`, whose spec is its first member's) still matches.
    let decimal = || ColumnTypeSpec::Decimal {
        precision: None,
        scale: None,
    };
    let beyond_i64 = || arr(vec![rmpv::Value::from(1_u64 << 63)]);
    let cases = [
        (arr(vec![rmpv::Value::F64(1.5)]), decimal()),
        (arr(vec![rmpv::Value::F64(1.5)]), ColumnTypeSpec::BigInteger),
        (arr(vec![s("2.5")]), ColumnTypeSpec::Double),
        (arr(vec![true.into()]), ColumnTypeSpec::BigInteger),
        (beyond_i64(), ColumnTypeSpec::BigInteger),
        (arr(vec![s("a"), 1.into()]), ColumnTypeSpec::Text),
    ];
    for (value, item) in cases {
        for (operator, value) in [
            ("=", value.clone()),
            ("!=", value.clone()),
            (">", value.clone()),
            ("IN", arr(vec![value.clone()])),
            ("BETWEEN", arr(vec![value.clone(), value.clone()])),
        ] {
            let condition = Condition::new(operator, value, array_of(item.clone()));
            let err = condition.error(Dialect::Postgres);
            assert!(
                err.contains("does not match the array's element type"),
                "{item:?} {operator}: {err}"
            );
            for dialect in [Dialect::Mysql, Dialect::Sqlite] {
                condition.build(dialect);
            }
        }
    }

    // An integer beyond 64 bits is not just out of range for INTEGER[]:
    // it does not bind as an integer at all.
    let err = int_array_override("=", beyond_i64()).error(Dialect::Postgres);
    assert!(err.contains("9223372036854775808"), "{err}");

    // NULL elements fit any array, and integers convert to the element type.
    for value in [arr(vec![rmpv::Value::Nil, 1.into()]), arr(vec![2.into()])] {
        for item in [decimal(), ColumnTypeSpec::Double] {
            for dialect in [Dialect::Postgres, Dialect::Mysql, Dialect::Sqlite] {
                Condition::new("=", value.clone(), array_of(item.clone())).build(dialect);
            }
        }
    }
}

#[test]
fn unreadable_array_overrides_are_an_error() {
    let ints = || arr(vec![1.into()]);
    for db_type in ["INTEGER[][]", "VARCHAR(10[]"] {
        let mut condition = int_array_override("=", ints());
        condition.db_type = Some(db_type);
        let err = condition.error(Dialect::Postgres);
        assert!(err.contains(db_type), "{err}");
    }
    let json = array_override("=", arr(vec![s("a")]), ColumnTypeSpec::Json, "JSON[]");
    assert!(json.error(Dialect::Postgres).contains("use jsonb[]"));

    // Only filters that cast a value need the override.
    let mut isnull = int_array_override("IS NULL", rmpv::Value::Nil);
    isnull.db_type = Some("VARCHAR(10[]");
    let (sql, _) = isnull.build(Dialect::Postgres);
    assert!(sql.ends_with(r#""tags" IS NULL"#), "{sql}");
}

#[test]
fn array_values_without_an_array_override_are_not_cast() {
    let ints = || arr(vec![1.into()]);
    let (sql, _) =
        Condition::new("=", ints(), array_of(ColumnTypeSpec::BigInteger)).build(Dialect::Postgres);
    assert!(sql.ends_with(r#""tags" = $1"#), "{sql}");
    let mut condition = int_array_override("=", ints());
    condition.db_type = Some("JSONB");
    let (sql, _) = condition.build(Dialect::Postgres);
    assert!(sql.ends_with(r#""tags" = $1"#), "{sql}");
}

#[test]
fn joined_columns_use_their_own_db_type() {
    // The base `tags` has no override; the joined `a.tags` is SMALLINT[].
    let ir = QueryIR {
        table: "posts".into(),
        cols: Some(vec!["id".into()]),
        column_types: Some(HashMap::from([
            ("tags".into(), array_of(ColumnTypeSpec::BigInteger)),
            ("a.tags".into(), array_of(ColumnTypeSpec::BigInteger)),
        ])),
        column_db_types: Some(HashMap::from([("a.tags".into(), "SMALLINT[]".into())])),
        filter_tree: Some(FilterNode::And {
            conditions: vec![
                filter("=", arr(vec![1.into()])),
                FilterNode::Condition(Filter {
                    field: "tags".into(),
                    operator: "=".into(),
                    value: arr(vec![1.into()]),
                    column: Some("a.tags".into()),
                    escape: None,
                }),
            ],
        }),
        ..Default::default()
    };
    let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
    assert!(
        sql.contains(r#""tags" = $1 AND "a"."tags" = ($2::SMALLINT[])"#),
        "{sql}"
    );
}

#[test]
fn like_on_json_columns_matches_the_unquoted_string() {
    // A `str` field stored as JSON matches the string it reads back as, not
    // its quoted JSON text.
    for spec in [ColumnTypeSpec::Json, ColumnTypeSpec::JsonBinary] {
        let case_sensitive = Condition::new("LIKE", s("%x%"), spec.clone());
        let case_insensitive = Condition::new("ILIKE", s("%x%"), spec);

        let (sql, _) = case_sensitive.build(Dialect::Postgres);
        assert!(sql.contains(r#"("tags" #>> '{}') LIKE $1"#), "{sql}");
        let (sql, _) = case_insensitive.build(Dialect::Postgres);
        assert!(sql.contains(r#"LOWER("tags" #>> '{}') LIKE $1"#), "{sql}");

        // A JSON null matches nothing, as `#>>` and json_extract() give
        // SQL NULL for it; JSON_UNQUOTE() alone would give 'null'.
        let (sql, params) = case_sensitive.build(Dialect::Mysql);
        assert!(
            sql.contains(
                "(CASE WHEN JSON_TYPE(`tags`) = 'NULL' THEN NULL ELSE JSON_UNQUOTE(`tags`) END) LIKE ?"
            ),
            "{sql}"
        );
        assert_eq!(params.len(), 1);

        let (sql, _) = case_sensitive.build(Dialect::Sqlite);
        assert!(
            sql.contains(
                r#"(CASE WHEN json_valid("tags", 9) THEN json_extract("tags", '$') ELSE "tags" END) LIKE ?"#
            ),
            "{sql}"
        );
    }

    // Text columns are untouched.
    let (sql, _) = Condition::new("LIKE", s("%x%"), ColumnTypeSpec::Text).build(Dialect::Postgres);
    assert!(sql.contains(r#""tags" LIKE $1"#), "{sql}");
}

#[test]
fn having_uses_the_aggregate_not_a_same_named_column() {
    // `tags` is both a JSON array column with an override and the alias of
    // MAX("name"): the HAVING condition is on the aggregate.
    for (operator, expected) in [
        ("LIKE", r#"MAX("name") LIKE $1"#),
        ("ILIKE", r#"LOWER(MAX("name")) LIKE $1"#),
        ("=", r#"MAX("name") = $1"#),
    ] {
        let ir = QueryIR {
            table: "posts".into(),
            cols: Some(vec!["status".into()]),
            column_types: Some(HashMap::from([(
                "tags".into(),
                array_of(ColumnTypeSpec::Json),
            )])),
            column_db_types: Some(HashMap::from([("tags".into(), "JSONB[]".into())])),
            aggregates: Some(vec![Aggregate {
                op: AggregateOp::Max,
                field: Some("name".into()),
                alias: Some("tags".into()),
                distinct: None,
            }]),
            group_by: Some(vec!["status".into()]),
            having: Some(filter(operator, s("x"))),
            ..Default::default()
        };
        let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(sql.contains(&format!("HAVING {expected}")), "{sql}");
    }
}

fn aggregate(op: AggregateOp, field: &str, alias: &str) -> Aggregate {
    Aggregate {
        op,
        field: Some(field.into()),
        alias: Some(alias.into()),
        distinct: None,
    }
}

/// `posts` grouped by `status`, with `HAVING <alias of agg> <operator> value`.
fn having_ir(
    agg: Aggregate,
    operator: &str,
    value: rmpv::Value,
    column_types: &[(&str, ColumnTypeSpec)],
) -> QueryIR {
    QueryIR {
        table: "posts".into(),
        cols: Some(vec!["status".into()]),
        column_types: Some(
            column_types
                .iter()
                .map(|(column, spec)| ((*column).into(), spec.clone()))
                .collect(),
        ),
        group_by: Some(vec!["status".into()]),
        having: Some(FilterNode::Condition(Filter {
            field: agg.alias.clone().unwrap(),
            operator: operator.into(),
            value,
            column: None,
            escape: None,
        })),
        aggregates: Some(vec![agg]),
        ..Default::default()
    }
}

fn status_enum() -> ColumnTypeSpec {
    ColumnTypeSpec::Enum {
        name: "post_status_enum".into(),
        values: vec!["draft".into(), "published".into()],
    }
}

fn decimal(value: i64, scale: u32) -> Value {
    Value::Decimal(Some(Box::new(Decimal::new(value, scale))))
}

#[test]
fn having_binds_values_by_the_aggregate_result_type() {
    let column_types = [
        (
            "price",
            ColumnTypeSpec::Decimal {
                precision: Some(10),
                scale: Some(2),
            },
        ),
        ("status", status_enum()),
    ];
    // Whether the alias is new, the aggregated column, or another column
    // with a type of its own, MAX("price") is a decimal.
    for alias in ["top", "price", "status"] {
        let ir = having_ir(
            aggregate(AggregateOp::Max, "price", alias),
            ">",
            s("5.5"),
            &column_types,
        );
        let (sql, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(
            sql.contains(r#"HAVING MAX("price") > $1"#),
            "{alias}: {sql}"
        );
        assert_eq!(params, vec![decimal(55, 1)], "{alias}");
    }
}

#[test]
fn having_min_max_keep_the_column_casts() {
    let ir = having_ir(
        aggregate(AggregateOp::Min, "status", "first_status"),
        "=",
        s("draft"),
        &[("status", status_enum())],
    );
    let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
    assert!(
        sql.contains(r#"HAVING MIN("status") = ($1::"post_status_enum")"#),
        "{sql}"
    );

    let mut ir = having_ir(
        aggregate(AggregateOp::Max, "tags", "top_tags"),
        "=",
        arr(vec![1.into(), 2.into()]),
        &[("tags", array_of(ColumnTypeSpec::BigInteger))],
    );
    ir.column_db_types = Some(HashMap::from([("tags".into(), "INTEGER[]".into())]));
    let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
    assert!(
        sql.contains(r#"HAVING MAX("tags") = ($1::INTEGER[])"#),
        "{sql}"
    );
}

#[test]
fn having_sum_avg_of_integers_compare_as_exact_numerics() {
    let column_types = [("views", ColumnTypeSpec::BigInteger)];
    for op in [AggregateOp::Sum, AggregateOp::Avg] {
        let ir = having_ir(
            aggregate(op.clone(), "views", "v"),
            ">",
            s("2.5"),
            &column_types,
        );
        let (_, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert_eq!(params, vec![decimal(25, 1)], "{op:?}");

        let ir = having_ir(
            aggregate(op.clone(), "views", "v"),
            ">",
            3.into(),
            &column_types,
        );
        let (_, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert_eq!(params, vec![Value::BigInt(Some(3))], "{op:?}");
    }

    let ir = having_ir(
        aggregate(AggregateOp::Count, "id", "n"),
        ">=",
        3.into(),
        &column_types,
    );
    let (_, params) = build_sql(&ir, Dialect::Postgres).unwrap();
    assert_eq!(params, vec![Value::BigInt(Some(3))]);
}
