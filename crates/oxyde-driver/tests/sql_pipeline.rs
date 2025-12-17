use oxyde_codec::{Filter, FilterNode, Operation, QueryIR, IR_PROTO_VERSION};
use oxyde_driver::{close_pool, execute_query, execute_statement, init_pool, PoolSettings};
use oxyde_query::{build_sql, Dialect};
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

#[tokio::test]
async fn sqlite_end_to_end_pipeline() {
    let pool_name = format!("pipeline_{}", Uuid::new_v4().simple());
    let mut settings = PoolSettings::default();
    settings.max_connections = Some(1);
    settings.min_connections = Some(1);
    init_pool(&pool_name, "sqlite::memory:", settings)
        .await
        .expect("init pool");

    execute_statement(
        &pool_name,
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        &[],
    )
    .await
    .unwrap();

    let mut insert_values = HashMap::new();
    insert_values.insert("id".to_string(), json!(1));
    insert_values.insert("name".to_string(), json!("Ada Lovelace"));
    let insert_ir = QueryIR {
        proto: IR_PROTO_VERSION,
        op: Operation::Insert,
        table: "users".into(),
        cols: None,
        col_types: None,
        filter_tree: None,
        limit: None,
        offset: None,
        order_by: None,
        values: Some(insert_values),
        bulk_values: None,
        bulk_update: None,
        model: None,
        distinct: None,
        column_mappings: None,
        joins: None,
        aggregates: None,
        returning: None,
        group_by: None,
        having: None,
        exists: None,
        count: None,
        on_conflict: None,
        lock: None,
        union_query: None,
        union_all: None,
        sql: None,
        params: None,
        pk_column: None,
    };
    let (insert_sql, insert_params) = build_sql(&insert_ir, Dialect::Sqlite).unwrap();
    execute_statement(&pool_name, &insert_sql, &insert_params)
        .await
        .unwrap();

    let select_ir = QueryIR {
        proto: IR_PROTO_VERSION,
        op: Operation::Select,
        table: "users".into(),
        cols: Some(vec!["id".into(), "name".into()]),
        col_types: None,
        filter_tree: Some(FilterNode::Condition(Filter {
            field: "id".into(),
            operator: "=".into(),
            value: json!(1),
            column: None,
        })),
        limit: None,
        offset: None,
        order_by: None,
        values: None,
        bulk_values: None,
        bulk_update: None,
        model: None,
        distinct: None,
        column_mappings: None,
        joins: None,
        aggregates: None,
        returning: None,
        group_by: None,
        having: None,
        exists: None,
        count: None,
        on_conflict: None,
        lock: None,
        union_query: None,
        union_all: None,
        sql: None,
        params: None,
        pk_column: None,
    };
    let (select_sql, select_params) = build_sql(&select_ir, Dialect::Sqlite).unwrap();
    let rows = execute_query(&pool_name, &select_sql, &select_params, None)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("name"),
        Some(&serde_json::Value::String("Ada Lovelace".into()))
    );

    close_pool(&pool_name).await.unwrap();
}
