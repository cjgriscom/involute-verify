//! Puzzle definitions loaded from the local cache (downloaded on demand).
//!
//! Files live at `{cache}/{id}/{version}/data.yaml` and `rejection-cache.bin`,
//! typically under `~/.cache/involute-verify`, fetched from
//! `https://involute.chandler.io/puzzles/{id}/{version}/…`.

use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::cache::{ensure_puzzle, CacheConfig};

/// Parsed puzzle definition used by scramble + replay.
#[derive(Debug, Clone)]
pub struct PuzzleDef {
    pub id: String,
    pub version: String,
    pub name: String,
    pub pieces: u16,
    pub min_scramble_moves: u32,
    pub scramble_rejection_bfs_depth: i32,
    pub generator: String,
    pub moves: Vec<CanonicalMove>,
    pub key_pieces: Vec<u16>,
    pub rejection_cache: Vec<u8>,
}

/// Named generator move (A, B, C, …) with simultaneous swap pairs.
#[derive(Debug, Clone)]
pub struct CanonicalMove {
    pub name: String,
    pub swaps: Vec<(u16, u16)>,
}

#[derive(Debug, Deserialize)]
struct YamlPuzzle {
    id: String,
    version: String,
    name: String,
    pieces: u16,
    #[serde(rename = "minScrambleMoves")]
    min_scramble_moves: u32,
    #[serde(rename = "scrambleRejectionBfsDepth")]
    scramble_rejection_bfs_depth: i32,
    generator: String,
}

/// Ensure `(id, version)` is cached, then parse into a [`PuzzleDef`].
pub fn load_puzzle(cfg: &CacheConfig, id: &str, version: &str) -> anyhow::Result<PuzzleDef> {
    let dir = ensure_puzzle(cfg, id, version)?;
    load_puzzle_from_dir(&dir, id, version)
}

/// Parse a puzzle from an already-populated cache directory.
pub fn load_puzzle_from_dir(dir: &Path, id: &str, version: &str) -> anyhow::Result<PuzzleDef> {
    let yaml_path = dir.join("data.yaml");
    let cache_path = dir.join("rejection-cache.bin");
    let yaml_bytes =
        fs::read(&yaml_path).map_err(|e| anyhow::anyhow!("read {}: {e}", yaml_path.display()))?;
    let rejection_cache =
        fs::read(&cache_path).map_err(|e| anyhow::anyhow!("read {}: {e}", cache_path.display()))?;

    let yaml: YamlPuzzle = serde_yaml::from_slice(&yaml_bytes)
        .map_err(|e| anyhow::anyhow!("parse {}: {e}", yaml_path.display()))?;
    if yaml.id != id {
        anyhow::bail!("puzzle id mismatch: yaml has {}, expected {id}", yaml.id);
    }
    if yaml.version != version {
        anyhow::bail!(
            "puzzle version mismatch: yaml has {}, expected {version}",
            yaml.version
        );
    }

    let moves = parse_generator(&yaml.generator)?;
    let mut key_pieces: Vec<u16> = moves
        .iter()
        .flat_map(|m| m.swaps.iter().flat_map(|&(a, b)| [a, b]))
        .collect();
    key_pieces.sort_unstable();
    key_pieces.dedup();

    Ok(PuzzleDef {
        id: yaml.id,
        version: yaml.version,
        name: yaml.name,
        pieces: yaml.pieces,
        min_scramble_moves: yaml.min_scramble_moves,
        scramble_rejection_bfs_depth: yaml.scramble_rejection_bfs_depth,
        generator: yaml.generator,
        moves,
        key_pieces,
        rejection_cache,
    })
}

/// Parse `[(a,b)(c,d),…]` into named moves A, B, C, …
pub fn parse_generator(input: &str) -> anyhow::Result<Vec<CanonicalMove>> {
    let trimmed = input.trim();
    let inner = strip_outer_brackets(trimmed, '[', ']')?;
    let groups = split_move_groups(inner);
    let mut moves = Vec::with_capacity(groups.len());
    for (i, group) in groups.into_iter().enumerate() {
        let name = char::from_u32(b'A' as u32 + i as u32)
            .filter(|c| c.is_ascii_uppercase())
            .map(|c| c.to_string())
            .ok_or_else(|| anyhow::anyhow!("too many generator moves"))?;
        let swaps = parse_swap_pairs(group.trim())?;
        moves.push(CanonicalMove { name, swaps });
    }
    Ok(moves)
}

fn strip_outer_brackets(s: &str, open: char, close: char) -> anyhow::Result<&str> {
    let s = s.trim();
    if s.starts_with(open) && s.ends_with(close) {
        Ok(&s[open.len_utf8()..s.len() - close.len_utf8()])
    } else {
        Ok(s)
    }
}

fn split_move_groups(inner: &str) -> Vec<&str> {
    let mut groups = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, ch) in inner.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                let piece = inner[start..i].trim();
                if !piece.is_empty() {
                    groups.push(piece);
                }
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = inner[start..].trim();
    if !tail.is_empty() {
        groups.push(tail);
    }
    groups
}

fn parse_swap_pairs(move_str: &str) -> anyhow::Result<Vec<(u16, u16)>> {
    let mut pairs = Vec::new();
    let bytes = move_str.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            let close = move_str[i..]
                .find(')')
                .ok_or_else(|| anyhow::anyhow!("unclosed swap pair in {move_str}"))?;
            let inner = &move_str[i + 1..i + close];
            let mut parts = inner.split(',');
            let a = parts
                .next()
                .ok_or_else(|| anyhow::anyhow!("bad swap pair"))?
                .trim()
                .parse::<u16>()?;
            let b = parts
                .next()
                .ok_or_else(|| anyhow::anyhow!("bad swap pair"))?
                .trim()
                .parse::<u16>()?;
            pairs.push((a, b));
            i += close + 1;
        } else {
            i += 1;
        }
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_box4_generator() {
        let moves = parse_generator("[(1,2),(2,3),(3,4),(4,1)]").unwrap();
        assert_eq!(moves.len(), 4);
        assert_eq!(moves[0].name, "A");
        assert_eq!(moves[0].swaps, vec![(1, 2)]);
        assert_eq!(moves[3].name, "D");
        assert_eq!(moves[3].swaps, vec![(4, 1)]);
    }
}
