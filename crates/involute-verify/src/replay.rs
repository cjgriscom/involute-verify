//! Piece-label permutations and solve-event replay.

use crate::puzzle::{CanonicalMove, PuzzleDef};
use crate::recording::SolveEvent;

/// 1-indexed piece labels; index `0` is unused.
pub type Labels = Vec<u16>;

/// Identity labeling for `points` pieces.
pub fn identity_labels(points: usize) -> Labels {
    let mut labels = vec![0u16; points + 1];
    for i in 1..=points {
        labels[i] = i as u16;
    }
    labels
}

/// Apply simultaneous swap pairs (involutions) in place.
pub fn apply_swap_pairs_in_place(labels: &mut Labels, swaps: &[(u16, u16)]) {
    let mut reads = std::collections::HashMap::new();
    for &(a, b) in swaps {
        reads.insert(a, labels[a as usize]);
        reads.insert(b, labels[b as usize]);
    }
    for &(a, b) in swaps {
        labels[a as usize] = reads[&b];
        labels[b as usize] = reads[&a];
    }
}

/// Encode labels as a comma-separated baseline string.
pub fn encode_baseline(labels: &Labels, points: usize) -> String {
    (1..=points)
        .map(|slot| labels[slot].to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Decode a baseline string into labels.
pub fn decode_baseline(encoded: &str, points: usize) -> anyhow::Result<Labels> {
    let mut labels = identity_labels(points);
    let trimmed = encoded.trim();
    if trimmed.is_empty() {
        return Ok(labels);
    }
    let values: Vec<u16> = trimmed
        .split(',')
        .map(|s| s.trim().parse::<u16>())
        .collect::<Result<_, _>>()?;
    if values.len() != points {
        anyhow::bail!("expected {points} baseline values, got {}", values.len());
    }
    for (i, piece) in values.into_iter().enumerate() {
        labels[i + 1] = piece;
    }
    Ok(labels)
}

/// Whether all `key_pieces` are home.
pub fn is_solved(labels: &Labels, key_pieces: &[u16]) -> bool {
    key_pieces.iter().all(|&id| labels[id as usize] == id)
}

/// Reject solve-path events that fall after `final_input_at`.
///
/// Without this check, a recording could stamp an incomplete prefix and still
/// “solve” via post-commitment moves.
pub fn assert_solve_path_in_commitment_window(
    events: &[SolveEvent],
    final_input_at: u64,
) -> anyhow::Result<()> {
    for event in events {
        let outside = match event {
            SolveEvent::Move { t, move_name } if *t > final_input_at => Some(format!(
                "move {move_name} at t={t} > finalInputAt={final_input_at}"
            )),
            SolveEvent::Undo { t } if *t > final_input_at => {
                Some(format!("undo at t={t} > finalInputAt={final_input_at}"))
            }
            SolveEvent::Solved { t } if *t > final_input_at => {
                Some(format!("solved at t={t} > finalInputAt={final_input_at}"))
            }
            SolveEvent::SolveClockStarted { t, .. } if *t > final_input_at => Some(format!(
                "solveClockStarted at t={t} > finalInputAt={final_input_at}"
            )),
            _ => None,
        };
        if let Some(msg) = outside {
            anyhow::bail!("solve event outside commitment window: {msg}");
        }
    }
    Ok(())
}

/// Replay move/undo events from `baseline` and require a solved end state.
pub fn replay_events(
    puzzle: &PuzzleDef,
    baseline: &Labels,
    events: &[SolveEvent],
) -> anyhow::Result<Labels> {
    let mut labels = baseline.clone();
    let mut history: Vec<Labels> = Vec::new();
    let moves_by_name: std::collections::HashMap<&str, &CanonicalMove> =
        puzzle.moves.iter().map(|m| (m.name.as_str(), m)).collect();

    let mut saw_solved = false;
    for event in events {
        match event {
            SolveEvent::Move { move_name, .. } => {
                let mv = moves_by_name
                    .get(move_name.as_str())
                    .ok_or_else(|| anyhow::anyhow!("unknown move {move_name}"))?;
                history.push(labels.clone());
                apply_swap_pairs_in_place(&mut labels, &mv.swaps);
            }
            SolveEvent::Undo { .. } => {
                labels = history
                    .pop()
                    .ok_or_else(|| anyhow::anyhow!("undo with empty history"))?;
            }
            SolveEvent::Solved { .. } => {
                saw_solved = true;
            }
            _ => {}
        }
    }

    if !saw_solved {
        anyhow::bail!("recording events missing solved marker");
    }
    if !is_solved(&labels, &puzzle.key_pieces) {
        anyhow::bail!("final state is not solved on key pieces");
    }
    Ok(labels)
}
