//! Solve recording JSON types and event encoding.
//!
//! Wire format: `type: "involute-solve-recording"`, `formatVersion: 1`.
//! See `docs/solve-recording.md` in this repository.

use serde::Deserialize;
use timecheck::drand::DrandRound;

/// Value of the top-level `type` field.
pub const SOLVE_RECORDING_TYPE: &str = "involute-solve-recording";

/// Parsed v1 solve recording.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SolveRecordingV1 {
    pub format_version: u32,
    #[serde(rename = "type")]
    pub recording_type: String,
    pub app_build_id: String,
    pub puzzle_id: String,
    pub puzzle_version: String,
    pub puzzle_name: String,
    pub baseline: String,
    pub settings: serde_json::Value,
    pub scramble: ScrambleParamsV1,
    pub tsa: TsaFields,
    pub timing: TimingFields,
    pub outcome: String,
    pub events: Vec<String>,
}

/// Scramble parameters embedded in the recording.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrambleParamsV1 {
    pub version: u32,
    pub time: String,
    pub seed: String,
    pub drand_round: DrandRound,
    pub attempts: u32,
}

/// RFC 3161 stamp fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TsaFields {
    pub digest_sha256: String,
    pub signature_hex: Option<String>,
    pub requested_at_t: u64,
    pub latency_ms: Option<u64>,
    pub status: String,
}

/// Timing fields relative to the scramble epoch.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingFields {
    pub epoch: String,
    pub inspection_seconds: u32,
    pub solve_clock_started_at: Option<u64>,
    pub final_input_at: Option<u64>,
    pub animation_completed_at: Option<u64>,
    pub elapsed_ms: Option<u64>,
}

/// Decoded compact event (`t|type|…`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveEvent {
    Move { t: u64, move_name: String },
    Undo { t: u64 },
    ShowSolved { t: u64, on: bool },
    Highlighter { t: u64, pieces: Vec<u16> },
    AnimationSpeed { t: u64, value: u32 },
    InspectionExpired { t: u64 },
    SolveClockStarted { t: u64, reason: String },
    Solved { t: u64 },
}

impl SolveEvent {
    /// Event timestamp in milliseconds from the scramble epoch.
    pub fn t(&self) -> u64 {
        match self {
            Self::Move { t, .. }
            | Self::Undo { t }
            | Self::ShowSolved { t, .. }
            | Self::Highlighter { t, .. }
            | Self::AnimationSpeed { t, .. }
            | Self::InspectionExpired { t }
            | Self::SolveClockStarted { t, .. }
            | Self::Solved { t } => *t,
        }
    }

    /// Re-encode to the compact wire form used in the commitment.
    pub fn encode(&self) -> String {
        match self {
            Self::Move { t, move_name } => format!("{t}|move|{move_name}"),
            Self::Undo { t } => format!("{t}|undo"),
            Self::ShowSolved { t, on } => format!("{t}|showSolved|{}", if *on { "1" } else { "0" }),
            Self::Highlighter { t, pieces } => {
                if pieces.is_empty() {
                    format!("{t}|highlighter")
                } else {
                    let ids = pieces
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("{t}|highlighter|{ids}")
                }
            }
            Self::AnimationSpeed { t, value } => format!("{t}|animationSpeed|{value}"),
            Self::InspectionExpired { t } => format!("{t}|inspectionExpired"),
            Self::SolveClockStarted { t, reason } => format!("{t}|solveClockStarted|{reason}"),
            Self::Solved { t } => format!("{t}|solved"),
        }
    }
}

/// Parse one compact event string.
pub fn parse_solve_event(encoded: &str) -> anyhow::Result<SolveEvent> {
    let parts: Vec<&str> = encoded.split('|').collect();
    if parts.len() < 2 {
        anyhow::bail!("invalid event: {encoded}");
    }
    let t: u64 = parts[0]
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid event t: {encoded}"))?;
    match parts[1] {
        "move" => {
            let move_name = parts
                .get(2)
                .copied()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("invalid move event: {encoded}"))?
                .to_string();
            Ok(SolveEvent::Move { t, move_name })
        }
        "undo" => Ok(SolveEvent::Undo { t }),
        "showSolved" => Ok(SolveEvent::ShowSolved {
            t,
            on: parts.get(2) == Some(&"1"),
        }),
        "highlighter" => {
            let pieces = if let Some(raw) = parts.get(2).filter(|s| !s.is_empty()) {
                raw.split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.parse::<u16>())
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                Vec::new()
            };
            Ok(SolveEvent::Highlighter { t, pieces })
        }
        "animationSpeed" => {
            let value: u32 = parts
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("invalid animationSpeed: {encoded}"))?
                .parse()?;
            Ok(SolveEvent::AnimationSpeed { t, value })
        }
        "inspectionExpired" => Ok(SolveEvent::InspectionExpired { t }),
        "solveClockStarted" => {
            let reason = parts
                .get(2)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("invalid solveClockStarted: {encoded}"))?;
            if reason != "firstMove" && reason != "inspectionExpired" {
                anyhow::bail!("invalid solveClockStarted: {encoded}");
            }
            Ok(SolveEvent::SolveClockStarted {
                t,
                reason: reason.to_string(),
            })
        }
        "solved" => Ok(SolveEvent::Solved { t }),
        other => anyhow::bail!("unknown event type: {other}"),
    }
}

/// Load and validate a recording file (format, type, outcome).
pub fn load_recording(path: &std::path::Path) -> anyhow::Result<SolveRecordingV1> {
    let text = std::fs::read_to_string(path)?;
    let recording: SolveRecordingV1 = serde_json::from_str(&text)?;
    if recording.format_version != 1 {
        anyhow::bail!("unsupported formatVersion {}", recording.format_version);
    }
    if recording.recording_type != SOLVE_RECORDING_TYPE {
        anyhow::bail!("unexpected type {}", recording.recording_type);
    }
    if recording.outcome != "solved" {
        anyhow::bail!("outcome is not solved: {}", recording.outcome);
    }
    Ok(recording)
}
