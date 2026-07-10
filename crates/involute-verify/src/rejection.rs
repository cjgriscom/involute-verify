//! RJCT v1 rejection cache and weak-scramble checks.
//!
//! A scramble is *weak* if it is the identity, appears in the precomputed BFS
//! ball, or lies within `span` moves of a cached state.

use std::collections::HashSet;

use blake3::Hasher;

use crate::replay::{apply_swap_pairs_in_place, Labels};

/// Magic `RJCT` (little-endian).
pub const RJCT_MAGIC: u32 = 0x524A_4354;
/// Supported cache format version.
pub const RJCT_VERSION: u8 = 1;
/// Fixed header size in bytes.
pub const RJCT_HEADER_SIZE: usize = 16;
/// Truncated BLAKE3 digest length stored per state.
pub const RJCT_HASH_BYTES: usize = 16;
/// Cap used when choosing the verify-time span ball radius.
pub const REJECTION_SPAN_VERIFY_MAX: u64 = 1025;

/// Parsed RJCT header fields.
#[derive(Debug, Clone)]
pub struct RejectionCacheHeader {
    pub points: u16,
    pub depth: u8,
    pub span: u8,
    pub hash_bytes: u8,
    pub state_count: u32,
    pub body_offset: usize,
}

/// Decoded rejection cache (header + digest set).
#[derive(Debug, Clone)]
pub struct RejectionCache {
    pub header: RejectionCacheHeader,
    pub keys: HashSet<[u8; RJCT_HASH_BYTES]>,
}

/// Decode an RJCT v1 blob.
pub fn decode_rejection_cache(bytes: &[u8]) -> anyhow::Result<RejectionCache> {
    if bytes.len() < RJCT_HEADER_SIZE {
        anyhow::bail!("rejection cache blob too small");
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into()?);
    if magic != RJCT_MAGIC {
        anyhow::bail!("invalid rejection cache magic");
    }
    if bytes[4] != RJCT_VERSION {
        anyhow::bail!("unsupported rejection cache version {}", bytes[4]);
    }
    let points = u16::from_le_bytes(bytes[5..7].try_into()?);
    let depth = bytes[7];
    let span = bytes[8];
    let hash_bytes = bytes[9];
    let state_count = u32::from_le_bytes(bytes[10..14].try_into()?);
    if hash_bytes as usize != RJCT_HASH_BYTES {
        anyhow::bail!("invalid hashBytes {hash_bytes}");
    }
    let expected = RJCT_HEADER_SIZE + state_count as usize * RJCT_HASH_BYTES;
    if bytes.len() < expected {
        anyhow::bail!("rejection cache blob truncated");
    }

    let mut keys = HashSet::with_capacity(state_count as usize);
    for i in 0..state_count as usize {
        let start = RJCT_HEADER_SIZE + i * RJCT_HASH_BYTES;
        let mut dig = [0u8; RJCT_HASH_BYTES];
        dig.copy_from_slice(&bytes[start..start + RJCT_HASH_BYTES]);
        keys.insert(dig);
    }

    Ok(RejectionCache {
        header: RejectionCacheHeader {
            points,
            depth,
            span,
            hash_bytes,
            state_count,
            body_offset: RJCT_HEADER_SIZE,
        },
        keys,
    })
}

fn bytes_per_label(points: usize) -> usize {
    if points <= 255 {
        1
    } else {
        2
    }
}

/// Pack 1-indexed labels into the RJCT wire encoding.
pub fn pack_label_array(labels: &Labels, points: usize) -> Vec<u8> {
    let bpl = bytes_per_label(points);
    let mut buf = vec![0u8; points * bpl];
    if bpl == 1 {
        for slot in 1..=points {
            buf[slot - 1] = labels[slot] as u8;
        }
    } else {
        for slot in 1..=points {
            let off = (slot - 1) * 2;
            buf[off..off + 2].copy_from_slice(&labels[slot].to_le_bytes());
        }
    }
    buf
}

/// Truncated BLAKE3 of the packed label array.
pub fn hash_label_array(labels: &Labels, points: usize) -> [u8; RJCT_HASH_BYTES] {
    let packed = pack_label_array(labels, points);
    let mut hasher = Hasher::new();
    hasher.update(&packed);
    let full = hasher.finalize();
    let mut out = [0u8; RJCT_HASH_BYTES];
    out.copy_from_slice(&full.as_bytes()[..RJCT_HASH_BYTES]);
    out
}

/// Whether every key piece is home.
pub fn is_identity_labels(labels: &Labels, points: usize) -> bool {
    (1..=points).all(|i| labels[i] == i as u16)
}

/// Largest span with `span^move_count < REJECTION_SPAN_VERIFY_MAX`.
pub fn rejection_span(move_count: usize) -> u8 {
    assert!(move_count >= 1);
    let mut span = 1u64;
    while (span + 1).pow(move_count as u32) < REJECTION_SPAN_VERIFY_MAX {
        span += 1;
    }
    span as u8
}

/// Effective span used for the verify-time ball (`min(span, depth)`).
pub fn effective_rejection_span(depth: u8, span: u8) -> u8 {
    span.min(depth)
}

fn expand_span_ball_keys(
    start: &Labels,
    points: usize,
    moves: &[&[(u16, u16)]],
    span: u8,
) -> HashSet<[u8; RJCT_HASH_BYTES]> {
    let mut keys = HashSet::new();
    let mut seen_packed = HashSet::new();

    let start_packed = pack_label_array(start, points);
    seen_packed.insert(start_packed.clone());
    keys.insert(hash_label_array(start, points));
    if span == 0 {
        return keys;
    }

    let mut frontier = vec![start.clone()];
    for _ in 0..span {
        let mut next = Vec::new();
        for state in &frontier {
            for swaps in moves {
                let mut working = state.clone();
                apply_swap_pairs_in_place(&mut working, swaps);
                let packed = pack_label_array(&working, points);
                if seen_packed.insert(packed) {
                    keys.insert(hash_label_array(&working, points));
                    next.push(working);
                }
            }
        }
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    keys
}

/// Return whether `labels` is a weak scramble under the given cache / depth.
pub fn is_weak_scramble_labels(
    labels: &Labels,
    points: usize,
    scramble_rejection_bfs_depth: i32,
    moves: &[&[(u16, u16)]],
    cache: &RejectionCache,
) -> anyhow::Result<bool> {
    if scramble_rejection_bfs_depth < 0 {
        return Ok(false);
    }
    if is_identity_labels(labels, points) {
        return Ok(true);
    }
    if cache.header.points as usize != points {
        anyhow::bail!(
            "rejection cache points {} ≠ puzzle pieces {points}",
            cache.header.points
        );
    }

    let digest = hash_label_array(labels, points);
    if cache.keys.contains(&digest) {
        return Ok(true);
    }

    let span = cache.header.span;
    let ball = expand_span_ball_keys(labels, points, moves, span);
    Ok(ball.iter().any(|k| cache.keys.contains(k)))
}
