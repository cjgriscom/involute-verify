//! TSA commitment digest (SHA-256 over canonical JSON).
//!
//! Matches the browser’s `buildCommitment` / `JSON.stringify` field order so
//! digests agree across TypeScript and Rust.

use serde::Serialize;
use sha2::{Digest, Sha256};
use timecheck::drand::DrandRound;

use crate::recording::{parse_solve_event, SolveEvent, SolveRecordingV1, SOLVE_RECORDING_TYPE};
use crate::scramble::SCRAMBLE_ALGORITHM_VERSION;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommitmentScramble<'a> {
    version: u32,
    time: &'a str,
    seed: &'a str,
    drand_round: &'a DrandRound,
    attempts: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommitmentTiming {
    inspection_seconds: u32,
    solve_clock_started_at: Option<u64>,
    final_input_at: u64,
}

#[derive(Debug, Serialize)]
struct Commitment<'a> {
    #[serde(rename = "formatVersion")]
    format_version: u32,
    #[serde(rename = "type")]
    recording_type: &'static str,
    #[serde(rename = "puzzleId")]
    puzzle_id: &'a str,
    #[serde(rename = "puzzleVersion")]
    puzzle_version: &'a str,
    baseline: &'a str,
    scramble: CommitmentScramble<'a>,
    timing: CommitmentTiming,
    events: Vec<String>,
}

/// Events with `t ≤ final_input_at`, in recording order.
pub fn commitment_events_through(
    events: &[String],
    final_input_at: u64,
) -> anyhow::Result<Vec<SolveEvent>> {
    let mut out = Vec::new();
    for enc in events {
        let ev = parse_solve_event(enc)?;
        if ev.t() <= final_input_at {
            out.push(ev);
        }
    }
    Ok(out)
}

/// Recompute the SHA-256 commitment digest for a recording.
pub fn build_commitment_digest(recording: &SolveRecordingV1) -> anyhow::Result<[u8; 32]> {
    let final_input_at = recording
        .timing
        .final_input_at
        .ok_or_else(|| anyhow::anyhow!("missing timing.finalInputAt"))?;

    let events = commitment_events_through(&recording.events, final_input_at)?;
    let event_strings: Vec<String> = events.iter().map(SolveEvent::encode).collect();

    if recording.scramble.version != SCRAMBLE_ALGORITHM_VERSION {
        anyhow::bail!(
            "unsupported scramble.version {}",
            recording.scramble.version
        );
    }

    let commitment = Commitment {
        format_version: 1,
        recording_type: SOLVE_RECORDING_TYPE,
        puzzle_id: &recording.puzzle_id,
        puzzle_version: &recording.puzzle_version,
        baseline: &recording.baseline,
        scramble: CommitmentScramble {
            version: recording.scramble.version,
            time: &recording.scramble.time,
            seed: &recording.scramble.seed,
            drand_round: &recording.scramble.drand_round,
            attempts: recording.scramble.attempts,
        },
        timing: CommitmentTiming {
            inspection_seconds: recording.timing.inspection_seconds,
            solve_clock_started_at: recording.timing.solve_clock_started_at,
            final_input_at,
        },
        events: event_strings,
    };

    // serde_json preserves struct field order; matches JS object insertion order.
    let canonical = serde_json::to_string(&commitment)?;
    let digest = Sha256::digest(canonical.as_bytes());
    Ok(digest.into())
}

/// Lowercase hex encoding of a digest.
pub fn digest_hex(digest: &[u8; 32]) -> String {
    hex::encode(digest)
}
