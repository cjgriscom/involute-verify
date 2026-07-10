//! Scramble algorithm v1 (ChaCha12), matching the INVOLUTE web app.
//!
//! Spec: repository `docs/solve-recording.md` §4–§9 (seed formula, RNG, weak-scramble).

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use rand_chacha::ChaCha12Rng;
use rand_core::{RngCore, SeedableRng};
use sha2::{Digest, Sha256};

use crate::puzzle::PuzzleDef;
use crate::rejection::{is_weak_scramble_labels, RejectionCache};
use crate::replay::{
    apply_swap_pairs_in_place, decode_baseline, encode_baseline, identity_labels, Labels,
};

/// Maximum scramble generation attempts (inclusive).
pub const MAX_SCRAMBLE_ATTEMPTS: u32 = 10;

/// Supported `scramble.version` value.
pub const SCRAMBLE_ALGORITHM_VERSION: u32 = 1;

/// Build the timed-solve seed: `{iso8601}_{base64(sha256(drand_signature))}`.
pub fn format_timed_scramble_seed(time_iso: &str, signature: &[u8]) -> String {
    let digest = Sha256::digest(signature);
    format!("{time_iso}_{}", B64.encode(digest))
}

/// Seed ChaCha12 from `SHA-256(u32_le(len) ‖ utf8(seed))`.
pub fn rng_from_seed(seed: &str) -> ChaCha12Rng {
    let mut hasher = Sha256::new();
    hasher.update((seed.len() as u32).to_le_bytes());
    hasher.update(seed.as_bytes());
    let digest = hasher.finalize();
    let mut seed_bytes = [0u8; 32];
    seed_bytes.copy_from_slice(&digest[..32]);
    ChaCha12Rng::from_seed(seed_bytes)
}

/// Next move index in `0..move_count`.
pub fn next_move_index(rng: &mut ChaCha12Rng, move_count: usize) -> usize {
    debug_assert!(move_count > 0);
    (rng.next_u32() as usize) % move_count
}

/// Result of regenerating and validating a scramble.
#[derive(Debug, Clone)]
pub struct ScrambleCheck {
    pub baseline: String,
    pub attempts: u32,
    pub labels: Labels,
}

/// Regenerate scramble v1 and check claimed baseline / attempts.
pub fn regenerate_scramble(
    puzzle: &PuzzleDef,
    seed: &str,
    claimed_attempts: u32,
    claimed_baseline: &str,
    cache: &RejectionCache,
) -> anyhow::Result<ScrambleCheck> {
    if puzzle.scramble_rejection_bfs_depth >= 0
        && cache.header.depth as i32 != puzzle.scramble_rejection_bfs_depth
    {
        anyhow::bail!(
            "rejection cache depth {} ≠ scrambleRejectionBfsDepth {}",
            cache.header.depth,
            puzzle.scramble_rejection_bfs_depth
        );
    }
    if !(1..=MAX_SCRAMBLE_ATTEMPTS).contains(&claimed_attempts) {
        anyhow::bail!("attempts {claimed_attempts} out of range 1..=10");
    }

    let move_swaps: Vec<&[(u16, u16)]> = puzzle.moves.iter().map(|m| m.swaps.as_slice()).collect();
    let move_count = move_swaps.len();
    if move_count == 0 {
        anyhow::bail!("puzzle has no moves");
    }

    let mut rng = rng_from_seed(seed);
    let min = puzzle.min_scramble_moves;
    let points = puzzle.pieces as usize;

    let mut accepted: Option<(u32, Labels)> = None;
    for attempt in 1..=MAX_SCRAMBLE_ATTEMPTS {
        let mut labels = identity_labels(points);
        for _ in 0..min {
            let idx = next_move_index(&mut rng, move_count);
            apply_swap_pairs_in_place(&mut labels, move_swaps[idx]);
        }

        let weak = is_weak_scramble_labels(
            &labels,
            points,
            puzzle.scramble_rejection_bfs_depth,
            &move_swaps,
            cache,
        )?;

        if attempt < claimed_attempts {
            if !weak {
                anyhow::bail!(
                    "attempt {attempt} was already non-weak but claimed attempts={claimed_attempts}"
                );
            }
            continue;
        }

        if weak {
            anyhow::bail!("accepted attempt {attempt} is weak");
        }
        accepted = Some((attempt, labels));
        break;
    }

    let (attempts, labels) = accepted.ok_or_else(|| {
        anyhow::anyhow!(
            "could not derive a non-weak scramble within {MAX_SCRAMBLE_ATTEMPTS} attempts"
        )
    })?;

    if attempts != claimed_attempts {
        anyhow::bail!("regenerated attempts {attempts} ≠ claimed {claimed_attempts}");
    }

    let baseline = encode_baseline(&labels, points);
    if baseline != claimed_baseline {
        anyhow::bail!("regenerated baseline {baseline} ≠ claimed {claimed_baseline}");
    }

    let decoded = decode_baseline(claimed_baseline, points)?;
    if decoded != labels {
        anyhow::bail!("claimed baseline decode mismatch");
    }

    Ok(ScrambleCheck {
        baseline,
        attempts,
        labels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_rng_vectors() {
        // docs/solve-recording.md §9
        let cases: &[(&str, [u32; 4])] = &[
            ("", [4029122272, 2625047272, 2419810370, 4046097121]),
            ("abc", [340927267, 3367352852, 1348168027, 3750729735]),
            ("00", [2946936326, 2552349042, 1182452546, 1119557327]),
            ("deadbeef", [1798100396, 2228036537, 2908366426, 3107919643]),
        ];
        for &(seed, expected) in cases {
            let mut rng = rng_from_seed(seed);
            for &e in &expected {
                assert_eq!(rng.next_u32(), e, "seed={seed:?}");
            }
        }

        let mut rng = ChaCha12Rng::from_seed([0u8; 32]);
        assert_eq!(rng.next_u32(), 1788540059);
    }
}
