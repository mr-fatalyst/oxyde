//! CellEncoder trait and generic columnar encoding.
//!
//! Encodes database rows directly to msgpack bytes without intermediate
//! serde_json::Value. Each backend implements CellEncoder with its
//! own type-specific decoding; the generic functions handle the
//! columnar structure.

use std::collections::HashMap;

use oxyde_codec::ColumnTypeSpec;
use sqlx::{Column, Database, Row};

/// Column metadata for encoding.
#[derive(Debug, Clone)]
pub struct ColumnMeta {
    pub name: String,
    pub db_type: String,
    /// Typed column spec from `QueryIR.column_types`. `None` for columns
    /// outside the model (raw SQL, aggregate aliases) — those fall back to
    /// the runtime DB type.
    pub spec: Option<ColumnTypeSpec>,
}

/// Backend-specific cell encoder.
///
/// Implementations decode a single cell from an sqlx Row and write
/// it directly to a msgpack buffer via `rmp::encode::write_*`.
pub trait CellEncoder {
    type Row: sqlx::Row;

    /// Extract column metadata from the first row.
    fn extract_columns(
        row: &Self::Row,
        col_types: Option<&HashMap<String, ColumnTypeSpec>>,
    ) -> Vec<ColumnMeta>
    where
        <<Self::Row as Row>::Database as Database>::Column: Column,
    {
        row.columns()
            .iter()
            .map(|c| {
                let name = Column::name(c).to_string();
                let spec = col_types.and_then(|ct| ct.get(&name).cloned());
                ColumnMeta {
                    db_type: Column::type_info(c).to_string().to_uppercase(),
                    spec,
                    name,
                }
            })
            .collect()
    }

    /// Try to encode a cell using the typed column spec.
    /// Returns `true` if the spec was recognized and encoded;
    /// `false` (for `Unknown`) falls through to the db_type path.
    fn try_encode_by_spec(
        buf: &mut Vec<u8>,
        row: &Self::Row,
        idx: usize,
        spec: &ColumnTypeSpec,
    ) -> bool;

    /// Encode a cell using the database column type (fallback).
    fn encode_by_db_type(buf: &mut Vec<u8>, row: &Self::Row, idx: usize, db_type: &str);

    /// Encode a single cell: spec first, then runtime DB type.
    fn encode_cell(buf: &mut Vec<u8>, row: &Self::Row, idx: usize, col: &ColumnMeta) {
        if let Some(spec) = &col.spec {
            if Self::try_encode_by_spec(buf, row, idx, spec) {
                return;
            }
        }
        Self::encode_by_db_type(buf, row, idx, &col.db_type);
    }
}

// ── msgpack write helpers ──────────────────────────────────────────────
// Vec<u8> Write impl never fails, so unwrap is safe.

#[inline]
pub fn write_nil(buf: &mut Vec<u8>) {
    rmp::encode::write_nil(buf).unwrap();
}

#[inline]
pub fn write_bool(buf: &mut Vec<u8>, v: bool) {
    rmp::encode::write_bool(buf, v).unwrap();
}

#[inline]
pub fn write_i64(buf: &mut Vec<u8>, v: i64) {
    rmp::encode::write_sint(buf, v).unwrap();
}

#[inline]
pub fn write_i32_as_i64(buf: &mut Vec<u8>, v: i32) {
    rmp::encode::write_sint(buf, i64::from(v)).unwrap();
}

#[inline]
pub fn write_u64(buf: &mut Vec<u8>, v: u64) {
    rmp::encode::write_uint(buf, v).unwrap();
}

#[inline]
pub fn write_f64(buf: &mut Vec<u8>, v: f64) {
    rmp::encode::write_f64(buf, v).unwrap();
}

#[inline]
pub fn write_str(buf: &mut Vec<u8>, v: &str) {
    rmp::encode::write_str(buf, v).unwrap();
}

#[inline]
pub fn write_bin(buf: &mut Vec<u8>, v: &[u8]) {
    rmp::encode::write_bin(buf, v).unwrap();
}

#[inline]
pub fn write_array_len(buf: &mut Vec<u8>, len: u32) {
    rmp::encode::write_array_len(buf, len).unwrap();
}

#[inline]
pub fn write_map_len(buf: &mut Vec<u8>, len: u32) {
    rmp::encode::write_map_len(buf, len).unwrap();
}

/// Write a serde_json::Value as msgpack (for JSON columns).
pub fn write_json_value(buf: &mut Vec<u8>, val: &serde_json::Value) {
    match val {
        serde_json::Value::Null => write_nil(buf),
        serde_json::Value::Bool(b) => write_bool(buf, *b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                write_i64(buf, i);
            } else if let Some(u) = n.as_u64() {
                write_u64(buf, u);
            } else if let Some(f) = n.as_f64() {
                write_f64(buf, f);
            } else {
                write_nil(buf);
            }
        }
        serde_json::Value::String(s) => write_str(buf, s),
        serde_json::Value::Array(arr) => {
            write_array_len(buf, arr.len() as u32);
            for v in arr {
                write_json_value(buf, v);
            }
        }
        serde_json::Value::Object(map) => {
            write_map_len(buf, map.len() as u32);
            for (k, v) in map {
                write_str(buf, k);
                write_json_value(buf, v);
            }
        }
    }
}

/// Write an rmpv::Value as msgpack (for PK values from INSERT RETURNING).
pub fn write_rmpv_value(buf: &mut Vec<u8>, val: &rmpv::Value) {
    match val {
        rmpv::Value::Nil => write_nil(buf),
        rmpv::Value::Boolean(b) => write_bool(buf, *b),
        rmpv::Value::Integer(n) => {
            if let Some(i) = n.as_i64() {
                write_i64(buf, i);
            } else if let Some(u) = n.as_u64() {
                write_u64(buf, u);
            } else {
                write_nil(buf);
            }
        }
        rmpv::Value::F32(f) => write_f64(buf, f64::from(*f)),
        rmpv::Value::F64(f) => write_f64(buf, *f),
        rmpv::Value::String(s) => write_str(buf, s.as_str().unwrap_or_default()),
        rmpv::Value::Binary(b) => write_bin(buf, b),
        rmpv::Value::Array(arr) => {
            write_array_len(buf, arr.len() as u32);
            for v in arr {
                write_rmpv_value(buf, v);
            }
        }
        rmpv::Value::Map(pairs) => {
            write_map_len(buf, pairs.len() as u32);
            for (k, v) in pairs {
                write_rmpv_value(buf, k);
                write_rmpv_value(buf, v);
            }
        }
        rmpv::Value::Ext(_, _) => write_nil(buf),
    }
}

// ── Generic encode functions ───────────────────────────────────────────
use futures::StreamExt;

/// Info about a JOIN relation for dedup encoding.
pub struct RelationInfo {
    /// Column prefix in result set, e.g. "author"
    pub prefix: String,
    /// PK column name in result set, e.g. "author__id"
    pub pk_col: String,
}

/// Msgpack nil byte (0xc0) — used to detect NULL PKs from LEFT JOINs.
const MSGPACK_NIL: u8 = 0xc0;

/// Encode a single cell into a small buffer (for PK extraction).
fn encode_pk_cell<E: CellEncoder>(row: &E::Row, col_idx: usize, col_meta: &ColumnMeta) -> Vec<u8> {
    let mut pk_buf = Vec::with_capacity(16);
    E::encode_cell(&mut pk_buf, row, col_idx, col_meta);
    pk_buf
}

/// Encode rows from a stream using columnar format (and optionally dedup for JOINs).
///
/// Returns `(msgpack_bytes, row_count)`.
pub async fn encode_stream<E, S>(
    mut stream: S,
    col_types: Option<&HashMap<String, ColumnTypeSpec>>,
    relations: Option<&[RelationInfo]>,
) -> Result<(Vec<u8>, usize), sqlx::Error>
where
    E: CellEncoder,
    S: futures::Stream<Item = Result<E::Row, sqlx::Error>> + Unpin,
    <<E::Row as Row>::Database as Database>::Column: Column,
{
    let first_row = match stream.next().await {
        Some(Ok(row)) => row,
        Some(Err(e)) => return Err(e),
        None => {
            let has_rels = relations.is_some_and(|r| !r.is_empty());
            let mut buf = Vec::with_capacity(8);
            if has_rels {
                write_array_len(&mut buf, 3);
                write_array_len(&mut buf, 0);
                write_array_len(&mut buf, 0);
                write_map_len(&mut buf, 0);
            } else {
                write_array_len(&mut buf, 2);
                write_array_len(&mut buf, 0);
                write_array_len(&mut buf, 0);
            }
            return Ok((buf, 0));
        }
    };

    let all_columns = E::extract_columns(&first_row, col_types);
    let rels = relations.unwrap_or(&[]);
    let has_rels = !rels.is_empty();

    let mut main_indices: Vec<usize> = Vec::new();
    let mut main_columns: Vec<&ColumnMeta> = Vec::new();

    struct RelGroup<'a> {
        prefix: &'a str,
        pk_col_idx: usize,
        col_indices: Vec<usize>,
        col_metas: Vec<&'a ColumnMeta>,
        stripped_names: Vec<String>,
        seen: std::collections::HashSet<Vec<u8>>,
        data_buf: Vec<u8>,
        refs_buf: Vec<u8>,
    }

    let mut rel_groups: Vec<RelGroup<'_>> = Vec::new();

    if has_rels {
        let prefixes_sep: Vec<String> = rels.iter().map(|r| format!("{}__", r.prefix)).collect();

        for ri in rels {
            rel_groups.push(RelGroup {
                prefix: &ri.prefix,
                pk_col_idx: usize::MAX,
                col_indices: Vec::new(),
                col_metas: Vec::new(),
                stripped_names: Vec::new(),
                seen: std::collections::HashSet::new(),
                data_buf: Vec::new(),
                refs_buf: Vec::new(),
            });
        }

        for (idx, col) in all_columns.iter().enumerate() {
            let mut matched = false;
            for (gi, psep) in prefixes_sep.iter().enumerate() {
                if col.name.starts_with(psep) {
                    if col.name == rels[gi].pk_col {
                        rel_groups[gi].pk_col_idx = idx;
                    }
                    rel_groups[gi].col_indices.push(idx);
                    rel_groups[gi].col_metas.push(col);
                    rel_groups[gi]
                        .stripped_names
                        .push(col.name[psep.len()..].to_string());
                    matched = true;
                    break;
                }
            }
            if !matched {
                main_indices.push(idx);
                main_columns.push(col);
            }
        }
    } else {
        for (idx, col) in all_columns.iter().enumerate() {
            main_indices.push(idx);
            main_columns.push(col);
        }
    }

    let mut main_rows_buf = Vec::new();
    let mut num_rows = 0;

    let mut process_row = |row: &E::Row, num_rows: &mut usize| {
        *num_rows += 1;

        write_array_len(&mut main_rows_buf, main_indices.len() as u32);
        for (pos, &idx) in main_indices.iter().enumerate() {
            E::encode_cell(&mut main_rows_buf, row, idx, main_columns[pos]);
        }

        if has_rels {
            for group in rel_groups.iter_mut() {
                let pk_bytes =
                    encode_pk_cell::<E>(row, group.pk_col_idx, &all_columns[group.pk_col_idx]);

                if pk_bytes.len() == 1 && pk_bytes[0] == MSGPACK_NIL {
                    write_nil(&mut group.refs_buf);
                } else {
                    if group.seen.insert(pk_bytes.clone()) {
                        group.data_buf.extend_from_slice(&pk_bytes);
                        write_array_len(&mut group.data_buf, group.col_indices.len() as u32);
                        for (pos, &idx) in group.col_indices.iter().enumerate() {
                            E::encode_cell(&mut group.data_buf, row, idx, group.col_metas[pos]);
                        }
                    }
                    group.refs_buf.extend_from_slice(&pk_bytes);
                }
            }
        }
    };

    process_row(&first_row, &mut num_rows);

    while let Some(row_res) = stream.next().await {
        let row = row_res?;
        process_row(&row, &mut num_rows);
    }

    let mut buf = Vec::with_capacity(main_rows_buf.len() + 1024);
    if has_rels {
        write_array_len(&mut buf, 3);
    } else {
        write_array_len(&mut buf, 2);
    }

    write_array_len(&mut buf, main_columns.len() as u32);
    for col in &main_columns {
        write_str(&mut buf, &col.name);
    }

    write_array_len(&mut buf, num_rows as u32);
    buf.extend_from_slice(&main_rows_buf);

    if has_rels {
        write_map_len(&mut buf, rel_groups.len() as u32);
        for group in rel_groups {
            write_str(&mut buf, group.prefix);
            write_map_len(&mut buf, 3);

            write_str(&mut buf, "columns");
            write_array_len(&mut buf, group.stripped_names.len() as u32);
            for name in &group.stripped_names {
                write_str(&mut buf, name);
            }

            write_str(&mut buf, "data");
            write_map_len(&mut buf, group.seen.len() as u32);
            buf.extend_from_slice(&group.data_buf);

            write_str(&mut buf, "refs");
            write_array_len(&mut buf, num_rows as u32);
            buf.extend_from_slice(&group.refs_buf);
        }
    }

    Ok((buf, num_rows))
}

/// Encode rows from a stream as a mutation-with-returning result:
/// `{"affected": N, "columns": [...], "rows": [[...], ...]}`
pub async fn encode_stream_mutation_returning<E, S>(
    mut stream: S,
    col_types: Option<&HashMap<String, ColumnTypeSpec>>,
) -> Result<Vec<u8>, sqlx::Error>
where
    E: CellEncoder,
    S: futures::Stream<Item = Result<E::Row, sqlx::Error>> + Unpin,
    <<E::Row as Row>::Database as Database>::Column: Column,
{
    let mut num_rows = 0;

    let first_row = match stream.next().await {
        Some(Ok(row)) => row,
        Some(Err(e)) => return Err(e),
        None => {
            let mut buf = Vec::with_capacity(32);
            write_map_len(&mut buf, 3);
            write_str(&mut buf, "affected");
            write_u64(&mut buf, 0);
            write_str(&mut buf, "columns");
            write_array_len(&mut buf, 0);
            write_str(&mut buf, "rows");
            write_array_len(&mut buf, 0);
            return Ok(buf);
        }
    };

    let columns = E::extract_columns(&first_row, col_types);
    num_rows += 1;

    let mut rows_buf = Vec::new();
    write_array_len(&mut rows_buf, columns.len() as u32);
    for (i, col) in columns.iter().enumerate() {
        E::encode_cell(&mut rows_buf, &first_row, i, col);
    }

    while let Some(row_res) = stream.next().await {
        let row = row_res?;
        num_rows += 1;
        write_array_len(&mut rows_buf, columns.len() as u32);
        for (i, col) in columns.iter().enumerate() {
            E::encode_cell(&mut rows_buf, &row, i, col);
        }
    }

    let mut buf = Vec::with_capacity(rows_buf.len() + 128);
    write_map_len(&mut buf, 3);
    write_str(&mut buf, "affected");
    write_u64(&mut buf, num_rows as u64);
    write_str(&mut buf, "columns");

    write_array_len(&mut buf, columns.len() as u32);
    for col in &columns {
        write_str(&mut buf, &col.name);
    }

    write_str(&mut buf, "rows");
    write_array_len(&mut buf, num_rows as u32);
    buf.extend_from_slice(&rows_buf);

    Ok(buf)
}
