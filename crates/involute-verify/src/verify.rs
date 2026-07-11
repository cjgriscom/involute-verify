//! End-to-end verification of a loaded solve recording.
//!
//! Pipeline (also mirrored in stderr progress when verbose):
//!
//! 1. Load puzzle definition for `(puzzleId, puzzleVersion)` from cache / site
//! 2. Verify drand round + scramble seed formula
//! 3. Regenerate scramble and check baseline / attempts
//! 4. Replay events within the commitment window (`t ≤ finalInputAt`)
//! 5. Recompute and match the TSA commitment digest
//! 6. Prepare TSA trust (cached moda CAs + TSR chain)
//! 7. OpenSSL `ts -verify`

use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::{DateTime, Duration, Utc};
use timecheck::drand::Chain;
use timecheck::tsa::{Signature, Tsa};

use crate::cache::{default_cache_dir, CacheConfig};
use crate::certs::prepare_trust_for_signature;
use crate::commitment::{build_commitment_digest, commitment_events_through, digest_hex};
use crate::log::{progress, step};
use crate::puzzle::load_puzzle;
use crate::recording::{load_recording, parse_solve_event, SolveEvent, SolveRecordingV1};
use crate::rejection::decode_rejection_cache;
use crate::replay::{assert_solve_path_in_commitment_window, decode_baseline, replay_events};
use crate::scramble::{
    format_timed_scramble_seed, regenerate_scramble, SCRAMBLE_ALGORITHM_VERSION,
};
use crate::verification::{Durations, SolveVerification, Timestamps, VerifiedTimestamps};

const STEPS: u32 = 7;
const DEFAULT_PUZZLE_BASE_URL: &str = "https://involute.chandler.io/puzzles";

/// Options for [`verify_recording`] / [`verify_loaded`].
#[derive(Debug, Clone)]
pub struct VerifyOptions {
    /// Cache root (`~/.cache/involute-verify` by default).
    pub cache_dir: PathBuf,
    /// Base URL for versioned puzzle artifacts.
    pub puzzle_base_url: String,
    /// Ignore the 10-day moda CA TTL and re-probe backends.
    pub force_refresh_certs: bool,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            cache_dir: default_cache_dir()
                .unwrap_or_else(|_| PathBuf::from(".cache/involute-verify")),
            puzzle_base_url: DEFAULT_PUZZLE_BASE_URL.to_string(),
            force_refresh_certs: false,
        }
    }
}

impl VerifyOptions {
    fn cache_config(&self) -> CacheConfig {
        CacheConfig {
            cache_dir: self.cache_dir.clone(),
            puzzle_base_url: self.puzzle_base_url.clone(),
            force_refresh_certs: self.force_refresh_certs,
        }
    }
}

/// Alias for [`SolveVerification`] (historical Involute naming).
pub type VerifyReport = SolveVerification;

/// Load a recording from disk and verify it.
pub fn verify_recording(path: &Path, options: &VerifyOptions) -> anyhow::Result<SolveVerification> {
    progress(format!("loading recording: {}", path.display()));
    let recording = load_recording(path)?;
    progress(format!(
        "loaded {} / {} (format v{}, {} events)",
        recording.puzzle_id,
        recording.puzzle_name,
        recording.format_version,
        recording.events.len()
    ));
    verify_loaded(&recording, options)
}

/// Verify an already-parsed recording.
pub fn verify_loaded(
    recording: &SolveRecordingV1,
    options: &VerifyOptions,
) -> anyhow::Result<SolveVerification> {
    let cache = options.cache_config();

    step(
        1,
        STEPS,
        format!(
            "loading puzzle `{}` @ {}",
            recording.puzzle_id, recording.puzzle_version
        ),
    );
    let puzzle = load_puzzle(&cache, &recording.puzzle_id, &recording.puzzle_version)?;
    progress(format!(
        "  puzzle ok: version={}, pieces={}, minScrambleMoves={}, rejectionDepth={}",
        puzzle.version,
        puzzle.pieces,
        puzzle.min_scramble_moves,
        puzzle.scramble_rejection_bfs_depth
    ));

    step(2, STEPS, "verifying drand quicknet round + scramble seed");
    if recording.scramble.version != SCRAMBLE_ALGORITHM_VERSION {
        anyhow::bail!(
            "unsupported scramble.version {}",
            recording.scramble.version
        );
    }
    let chain = Chain::quicknet();
    chain
        .verify(&recording.scramble.drand_round)
        .map_err(|e| anyhow::anyhow!("drand verify failed: {e}"))?;
    progress(format!(
        "  drand round {} signature ok",
        recording.scramble.drand_round.number
    ));
    let expected_seed = format_timed_scramble_seed(
        &recording.scramble.time,
        &recording.scramble.drand_round.signature,
    );
    if expected_seed != recording.scramble.seed {
        anyhow::bail!(
            "seed mismatch: expected {expected_seed}, got {}",
            recording.scramble.seed
        );
    }
    progress("  seed formula matches");

    let scramble_range = chain
        .round_time_range(recording.scramble.drand_round.number)
        .map_err(|e| anyhow::anyhow!("drand round_time_range failed: {e}"))?;
    progress(format!(
        "  scramble range: {} .. {}",
        scramble_range.start.to_rfc3339(),
        scramble_range.end.to_rfc3339()
    ));

    step(
        3,
        STEPS,
        format!(
            "regenerating ChaCha12 scramble (attempts={}, minMoves={})",
            recording.scramble.attempts, puzzle.min_scramble_moves
        ),
    );
    let rjct = decode_rejection_cache(&puzzle.rejection_cache)?;
    progress(format!(
        "  rejection cache: depth={}, span={}, states={}",
        rjct.header.depth, rjct.header.span, rjct.header.state_count
    ));
    let _scramble = regenerate_scramble(
        &puzzle,
        &recording.scramble.seed,
        recording.scramble.attempts,
        &recording.baseline,
        &rjct,
    )?;
    progress(format!("  baseline matches: {}", recording.baseline));

    let final_input_at = recording
        .timing
        .final_input_at
        .ok_or_else(|| anyhow::anyhow!("missing timing.finalInputAt"))?;
    step(
        4,
        STEPS,
        format!("replaying solve path within commitment window (t ≤ {final_input_at})"),
    );
    let events: Vec<_> = recording
        .events
        .iter()
        .map(|e| parse_solve_event(e))
        .collect::<Result<_, _>>()?;
    assert_solve_path_in_commitment_window(&events, final_input_at)?;
    progress("  all move/undo/solved events are within commitment window");

    let committed_events = commitment_events_through(&recording.events, final_input_at)?;
    let baseline_labels = decode_baseline(&recording.baseline, puzzle.pieces as usize)?;
    replay_events(&puzzle, &baseline_labels, &committed_events)?;
    progress("  committed event prefix reaches solved on key pieces");

    let solution_stm = count_solution_stm(&committed_events);
    progress(format!("  solution_stm={solution_stm}"));

    if let (Some(start), Some(elapsed)) = (
        recording.timing.solve_clock_started_at,
        recording.timing.elapsed_ms,
    ) {
        let computed = final_input_at.saturating_sub(start);
        if computed != elapsed {
            anyhow::bail!("elapsedMs {elapsed} ≠ finalInputAt - solveClockStartedAt ({computed})");
        }
        progress(format!("  timing ok: elapsedMs={elapsed}"));
    }

    step(5, STEPS, "recomputing TSA commitment digest");
    if recording.tsa.status != "ok" {
        anyhow::bail!("tsa.status is {}, expected ok", recording.tsa.status);
    }
    let digest = build_commitment_digest(recording)?;
    let digest_hex_str = digest_hex(&digest);
    if !eq_hex_ignore_case(&digest_hex_str, &recording.tsa.digest_sha256) {
        anyhow::bail!(
            "commitment digest mismatch: recomputed {digest_hex_str}, recording {}",
            recording.tsa.digest_sha256
        );
    }
    progress(format!("  digest ok: {digest_hex_str}"));

    let sig_hex = recording
        .tsa
        .signature_hex
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing tsa.signatureHex"))?;
    let signature =
        Signature::from_str(sig_hex).map_err(|e| anyhow::anyhow!("invalid signatureHex: {e}"))?;
    progress(format!("  TSR length: {} bytes", sig_hex.len() / 2));

    step(
        6,
        STEPS,
        "preparing TSA trust (cached moda CAs + recording chain)",
    );
    let trust = prepare_trust_for_signature(&cache, &signature)?;

    step(
        7,
        STEPS,
        "openssl ts -verify (moda CAfile + TSR -untrusted)",
    );
    progress("  running openssl ts -verify…");
    let tsa = Tsa::with_pem("https://rfc3161.ai.moda", trust.ca_pem);
    tsa.verify_with_options(
        &digest,
        &signature,
        Some(&trust.untrusted_pem),
        trust.partial_chain,
    )
    .map_err(|e| anyhow::anyhow!("TSA openssl verify failed: {e}"))?;

    let gen_time = signature
        .timestamp()
        .map_err(|e| anyhow::anyhow!("TSA timestamp parse failed: {e}"))?;
    progress(format!("  TSA genTime: {}", gen_time.to_rfc3339()));
    progress("all checks passed");

    let scramble_generation = parse_scramble_time(&recording.scramble.time)?;
    // Recorder epoch is scramble confirm ≈ when the scramble is presented.
    let inspection_start = Some(scramble_generation);
    let solve_start = recording
        .timing
        .solve_clock_started_at
        .map(|t| absolute_time(scramble_generation, t));
    let solve_completion = Some(absolute_time(scramble_generation, final_input_at));

    let timestamps = Timestamps {
        scramble_generation: Some(scramble_generation),
        inspection_start,
        blindfold_don: None,
        solve_start,
        solve_completion,
    };
    let verified_timestamps = VerifiedTimestamps {
        scramble_range_start: Some(scramble_range.start),
        scramble_range_end: Some(scramble_range.end),
        completion: Some(gen_time),
    };
    let durations = Durations::new(timestamps, verified_timestamps, false);

    Ok(SolveVerification {
        puzzle_canonical_id: recording.puzzle_id.clone(),
        puzzle_version: recording.puzzle_version.clone(),
        solution_stm,
        used_filters: false,
        used_macros: false,
        timestamps,
        verified_timestamps,
        durations,
        errors: vec![],
    })
}

fn parse_scramble_time(s: &str) -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(s)
        .map_err(|e| anyhow::anyhow!("invalid scramble.time {s:?}: {e}"))?
        .with_timezone(&Utc))
}

fn absolute_time(scramble: DateTime<Utc>, t_ms: u64) -> DateTime<Utc> {
    scramble + Duration::milliseconds(t_ms as i64)
}

/// Net solution STM after undos and consecutive identical-move cancellation.
///
/// See `docs/stm.md`.
fn count_solution_stm(events: &[SolveEvent]) -> u64 {
    let mut moves: Vec<&str> = Vec::new();
    for event in events {
        match event {
            SolveEvent::Move { move_name, .. } => moves.push(move_name.as_str()),
            SolveEvent::Undo { .. } => {
                moves.pop();
            }
            _ => {}
        }
    }

    // Collapse consecutive identical moves in pairs (A A → ∅, A A A → A).
    let mut stack: Vec<&str> = Vec::new();
    for m in moves {
        if stack.last() == Some(&m) {
            stack.pop();
        } else {
            stack.push(m);
        }
    }
    stack.len() as u64
}

fn eq_hex_ignore_case(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::SolveEvent;

    fn moves(names: &[&str]) -> Vec<SolveEvent> {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| SolveEvent::Move {
                t: i as u64,
                move_name: (*name).to_string(),
            })
            .collect()
    }

    #[test]
    fn stm_collapses_consecutive_identical_moves() {
        // B A A C → B C
        assert_eq!(count_solution_stm(&moves(&["B", "A", "A", "C"])), 2);
        // B A A A C → B A C
        assert_eq!(count_solution_stm(&moves(&["B", "A", "A", "A", "C"])), 3);
        // B A A B C → C
        assert_eq!(count_solution_stm(&moves(&["B", "A", "A", "B", "C"])), 1);
    }

    #[test]
    fn stm_applies_undos_before_collapse() {
        let events = vec![
            SolveEvent::Move {
                t: 0,
                move_name: "A".into(),
            },
            SolveEvent::Move {
                t: 1,
                move_name: "A".into(),
            },
            SolveEvent::Undo { t: 2 },
        ];
        // After undo: [A], not an empty cancelled pair.
        assert_eq!(count_solution_stm(&events), 1);
    }
}
