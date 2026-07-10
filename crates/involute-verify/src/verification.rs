//! Verification report types.
//!
//! Field names and chrono serde shapes match Hyperspeedcube’s `SolveVerification`
//! so leaderboard / tooling JSON can be shared across apps.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Result of a successful (or partially annotated) solve verification.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SolveVerification {
    /// Canonical puzzle id (leaderboard category).
    pub puzzle_canonical_id: String,
    /// Puzzle definition version string.
    pub puzzle_version: String,
    /// Net solution move count (STM after undos).
    pub solution_stm: u64,
    /// Whether piece filters were used (always `false` for Involute v1).
    pub used_filters: bool,
    /// Whether macros were used (always `false` for Involute v1).
    pub used_macros: bool,

    /// Wall-clock times derived from the recording.
    pub timestamps: Timestamps,
    /// Times backed by third-party crypto (drand range, TSA genTime).
    pub verified_timestamps: VerifiedTimestamps,
    /// Intervals derived from [`Timestamps`] / [`VerifiedTimestamps`].
    pub durations: Durations,

    /// Soft errors collected during verification (empty on hard success).
    pub errors: Vec<String>,
}

/// Client-side timestamps from the recording log.
#[derive(Serialize, Deserialize, Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct Timestamps {
    /// When the scramble was generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scramble_generation: Option<DateTime<Utc>>,
    /// When the scrambled puzzle was presented (inspection start).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspection_start: Option<DateTime<Utc>>,
    /// Blindfold “don” time (unused by Involute).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blindfold_don: Option<DateTime<Utc>>,
    /// Solve clock start / first move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solve_start: Option<DateTime<Utc>>,
    /// Final solving input (`finalInputAt`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solve_completion: Option<DateTime<Utc>>,
}

/// Cryptographically corroborated timestamps.
#[derive(Serialize, Deserialize, Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct VerifiedTimestamps {
    /// Earliest time the scramble could have been generated (drand round start).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scramble_range_start: Option<DateTime<Utc>>,
    /// Latest time the scramble should have been generated (drand round end).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scramble_range_end: Option<DateTime<Utc>>,
    /// TSA token `genTime` (latest plausible completion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<DateTime<Utc>>,
}

/// Derived durations between timestamp fields.
#[derive(Serialize, Deserialize, Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct Durations {
    /// `scramble_generation - scramble_range_end`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scramble_network_latency: Option<Duration>,
    /// `inspection_start - scramble_generation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scramble_application: Option<Duration>,

    /// `solve_start - inspection_start`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspection: Option<Duration>,
    /// `solve_completion - solve_start`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speedsolve: Option<Duration>,

    /// Blindsolve memo duration (unused by Involute).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<Duration>,
    /// Blindsolve total duration (unused by Involute).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blindsolve: Option<Duration>,

    /// `completion - solve_completion` (TSA network latency).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_network_latency: Option<Duration>,
}

impl Durations {
    /// Compute durations from timestamp fields (Hyperspeedcube-compatible formulas).
    pub fn new(
        timestamps: Timestamps,
        verified_timestamps: VerifiedTimestamps,
        is_valid_blindsolve: bool,
    ) -> Self {
        Self {
            scramble_network_latency: (|| {
                Some(timestamps.scramble_generation? - verified_timestamps.scramble_range_end?)
            })(),
            scramble_application: (|| {
                Some(timestamps.inspection_start? - timestamps.scramble_generation?)
            })(),

            inspection: (|| Some(timestamps.solve_start? - timestamps.inspection_start?))(),
            speedsolve: (|| Some(timestamps.solve_completion? - timestamps.solve_start?))(),

            memo: (|| Some(timestamps.blindfold_don? - timestamps.inspection_start?))()
                .filter(|_| is_valid_blindsolve),
            blindsolve: (|| Some(timestamps.solve_completion? - timestamps.inspection_start?))()
                .filter(|_| is_valid_blindsolve),

            timestamp_network_latency: (|| {
                Some(verified_timestamps.completion? - timestamps.solve_completion?)
            })(),
        }
    }
}
