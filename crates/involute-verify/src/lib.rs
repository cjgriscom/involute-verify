//! Verifier for [INVOLUTE](https://involute.chandler.io) competitive solve recordings.
//!
//! # What it checks
//!
//! 1. **drand** — scramble seed is bound to a verified quicknet round
//! 2. **Scramble** — baseline regenerates from the seed (ChaCha12 + RJCT weak-scramble)
//! 3. **Replay** — events reach a solved state inside the TSA commitment window
//! 4. **Commitment** — SHA-256 digest matches the recording
//! 5. **TSA** — RFC 3161 token verifies via OpenSSL against cached moda CAs
//!
//! # Quick start
//!
//! ```no_run
//! use std::path::Path;
//! use involute_verify::{set_verbose, verify_recording, VerifyOptions};
//!
//! set_verbose(false);
//! let report = verify_recording(Path::new("recording.json"), &VerifyOptions::default())?;
//! assert!(report.errors.is_empty());
//! # Ok::<(), anyhow::Error>(())
//! ```
//!
//! Puzzle definitions and moda trust anchors are cached under
//! `~/.cache/involute-verify` (see [`cache`]). Network is needed on first use,
//! cache miss, or when moda CAs are older than 10 days.
//!
//! # Modules
//!
//! Most modules support the verifier pipeline and the recording wire format.
//! The stable entry points are re-exported below.

#![forbid(unsafe_code)]

pub mod cache;
pub mod certs;
pub mod commitment;
pub mod log;
pub mod puzzle;
pub mod recording;
pub mod rejection;
pub mod replay;
pub mod scramble;
pub mod verification;
pub mod verify;

pub use cache::{default_cache_dir, CacheConfig};
pub use log::set_verbose;
pub use verification::{Durations, SolveVerification, Timestamps, VerifiedTimestamps};
pub use verify::{verify_loaded, verify_recording, VerifyOptions, VerifyReport};
