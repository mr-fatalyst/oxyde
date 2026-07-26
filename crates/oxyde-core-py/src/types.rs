//! Result types for mutation operations, encoded directly to msgpack.

// msgpack container lengths are u32 by format; real counts never approach it.
#![allow(clippy::cast_possible_truncation)]

use oxyde_driver::{write_array_len, write_map_len, write_rmpv_value, write_str, write_u64};

/// Encode an INSERT result (bulk path: only PKs) to msgpack.
/// Format: `{"affected": N, "inserted_ids": [id, ...]}`
pub(crate) fn encode_insert_result(affected: usize, inserted_ids: &[rmpv::Value]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(32 + inserted_ids.len() * 8);
    write_map_len(&mut buf, 2);
    write_str(&mut buf, "affected");
    write_u64(&mut buf, affected as u64);
    write_str(&mut buf, "inserted_ids");
    write_array_len(&mut buf, inserted_ids.len() as u32);
    for id in inserted_ids {
        write_rmpv_value(&mut buf, id);
    }
    buf
}

/// Encode a mutation result (UPDATE/DELETE without RETURNING) to msgpack.
/// Format: `{"affected": N}`
pub(crate) fn encode_mutation_result(affected: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16);
    write_map_len(&mut buf, 1);
    write_str(&mut buf, "affected");
    write_u64(&mut buf, affected);
    buf
}
