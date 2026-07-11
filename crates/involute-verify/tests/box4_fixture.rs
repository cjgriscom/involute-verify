//! Golden-path integration tests against a checked-in box4 recording.
//!
//! Puzzle files are seeded into a temp cache (no puzzle download). Full TSA
//! verify may refresh moda CAs over the network on first run.

use std::fs;
use std::path::{Path, PathBuf};

use involute_verify::commitment::{build_commitment_digest, digest_hex};
use involute_verify::puzzle::load_puzzle;
use involute_verify::recording::{load_recording, parse_solve_event};
use involute_verify::rejection::decode_rejection_cache;
use involute_verify::replay::{decode_baseline, replay_events};
use involute_verify::scramble::{format_timed_scramble_seed, regenerate_scramble};
use involute_verify::set_verbose;
use involute_verify::verify::{verify_loaded, VerifyOptions};
use involute_verify::CacheConfig;
use tempfile::TempDir;
use timecheck::drand::Chain;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join("2026-07-09 INVOLUTE Replay (box4) 0h00m09s32.json")
}

fn seed_box4_cache(cache_root: &Path) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/box4");
    let dest = cache_root.join("box4/1.0.0");
    fs::create_dir_all(&dest).unwrap();
    fs::copy(src.join("data.yaml"), dest.join("data.yaml")).unwrap();
    fs::copy(
        src.join("rejection-cache.bin"),
        dest.join("rejection-cache.bin"),
    )
    .unwrap();
}

fn test_options(cache_root: &Path) -> VerifyOptions {
    VerifyOptions {
        cache_dir: cache_root.to_path_buf(),
        puzzle_base_url: "https://involute.chandler.io/puzzles".to_string(),
        force_refresh_certs: false,
    }
}

#[test]
fn box4_offline_checks() {
    set_verbose(false);
    let tmp = TempDir::new().unwrap();
    seed_box4_cache(tmp.path());
    let cfg = CacheConfig {
        cache_dir: tmp.path().to_path_buf(),
        puzzle_base_url: "https://involute.chandler.io/puzzles".to_string(),
        force_refresh_certs: false,
    };

    let recording = load_recording(&fixture_path()).unwrap();
    let puzzle = load_puzzle(&cfg, &recording.puzzle_id, &recording.puzzle_version).unwrap();
    assert_eq!(puzzle.version, recording.puzzle_version);

    Chain::quicknet()
        .verify(&recording.scramble.drand_round)
        .unwrap();
    let expected_seed = format_timed_scramble_seed(
        &recording.scramble.time,
        &recording.scramble.drand_round.signature,
    );
    assert_eq!(expected_seed, recording.scramble.seed);

    let cache = decode_rejection_cache(&puzzle.rejection_cache).unwrap();
    regenerate_scramble(
        &puzzle,
        &recording.scramble.seed,
        recording.scramble.attempts,
        &recording.baseline,
        &cache,
    )
    .unwrap();

    let events: Vec<_> = recording
        .events
        .iter()
        .map(|e| parse_solve_event(e).unwrap())
        .collect();
    let baseline = decode_baseline(&recording.baseline, puzzle.pieces as usize).unwrap();
    replay_events(&puzzle, &baseline, &events).unwrap();

    let digest = build_commitment_digest(&recording).unwrap();
    assert!(
        digest_hex(&digest).eq_ignore_ascii_case(&recording.tsa.digest_sha256),
        "digest mismatch"
    );
}

#[test]
fn box4_full_verify_with_tsa() {
    set_verbose(true);
    let tmp = TempDir::new().unwrap();
    seed_box4_cache(tmp.path());
    let options = test_options(tmp.path());

    let recording = load_recording(&fixture_path()).unwrap();
    let report = verify_loaded(&recording, &options).expect("full verify should pass");
    assert_eq!(report.puzzle_canonical_id, "test_puzzle");
    assert!(report.errors.is_empty());
    assert!(report.verified_timestamps.scramble_range_start.is_some());
    assert!(report.verified_timestamps.scramble_range_end.is_some());
    assert!(report.verified_timestamps.completion.is_some());
    assert!(report.durations.speedsolve.is_some());
    assert_eq!(
        report.durations.speedsolve.unwrap().num_milliseconds(),
        recording.timing.elapsed_ms.unwrap() as i64
    );
}
