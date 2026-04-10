use oxyde_codec::{Filter, FilterNode, Operation, QueryIR, IR_PROTO_VERSION};
use oxyde_driver::{
    close_pool, execute_query_columnar, execute_statement, init_pool, PoolSettings,
};
use oxyde_query::{build_sql, Dialect};
use std::collections::HashMap;
use uuid::Uuid;

fn decode_columnar(buf: &[u8]) -> (Vec<String>, Vec<Vec<rmpv::Value>>) {
    let val: rmpv::Value = rmp_serde::from_slice(buf).unwrap();
    let arr = val.as_array().unwrap();
    let columns: Vec<String> = arr[0]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let rows: Vec<Vec<rmpv::Value>> = arr[1]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_array().unwrap().clone())
        .collect();
    (columns, rows)
}

#[tokio::test]
async fn sqlite_end_to_end_pipeline() {
    let pool_name = format!("pipeline_{}", Uuid::new_v4().simple());
    let settings = PoolSettings {
        max_connections: Some(1),
        min_connections: Some(1),
        ..Default::default()
    };
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
    insert_values.insert("id".to_string(), rmpv::Value::Integer(1.into()));
    insert_values.insert(
        "name".to_string(),
        rmpv::Value::String("Ada Lovelace".into()),
    );
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
            value: rmpv::Value::Integer(1.into()),
            column: None,
            escape: None,
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
    let (bytes, num_rows) = execute_query_columnar(&pool_name, &select_sql, &select_params, None)
        .await
        .unwrap();

    assert_eq!(num_rows, 1);
    let (columns, rows) = decode_columnar(&bytes);
    assert_eq!(columns, vec!["id", "name"]);
    assert_eq!(rows[0][1].as_str().unwrap(), "Ada Lovelace");

    close_pool(&pool_name).await.unwrap();
}
