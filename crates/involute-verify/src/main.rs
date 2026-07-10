//! Involute competitive solve recording verifier (CLI).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use involute_verify::{set_verbose, verify_recording, VerifyOptions};

#[derive(Debug, Parser)]
#[command(
    name = "involute-verify",
    about = "Verify INVOLUTE competitive solve recordings",
    long_about = "Linux verifier for INVOLUTE solve recordings.\n\n\
Checks drand scramble binding, scramble regeneration, event replay, the TSA \
commitment digest, and the RFC 3161 timestamp (OpenSSL).\n\n\
Puzzle definitions and moda TSA trust anchors are cached under \
~/.cache/involute-verify (override with --cache-dir). Puzzles are downloaded \
from involute.chandler.io on first use; moda CAs refresh every 10 days.\n\n\
Progress is written to stderr. Pass --json for a SolveVerification report on \
stdout. Recording TSRs supply only -untrusted intermediates (certReq: true)."
)]
struct Cli {
    /// Path to an Involute solve recording JSON file
    recording: PathBuf,

    /// Print SolveVerification JSON on stdout (progress still on stderr)
    #[arg(long)]
    json: bool,

    /// Suppress progress messages on stderr
    #[arg(long, short = 'q')]
    quiet: bool,

    /// Cache directory (default: ~/.cache/involute-verify)
    #[arg(long, value_name = "DIR")]
    cache_dir: Option<PathBuf>,

    /// Base URL for versioned puzzle artifacts
    #[arg(
        long,
        value_name = "URL",
        default_value = "https://involute.chandler.io/puzzles"
    )]
    puzzle_base_url: String,

    /// Re-probe moda backends even if the cached CA bundle is fresh
    #[arg(long)]
    force_refresh_certs: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    set_verbose(!cli.quiet);

    let mut options = VerifyOptions::default();
    if let Some(dir) = cli.cache_dir {
        options.cache_dir = dir;
    }
    options.puzzle_base_url = cli.puzzle_base_url;
    options.force_refresh_certs = cli.force_refresh_certs;

    match verify_recording(&cli.recording, &options) {
        Ok(report) => {
            if cli.json {
                match serde_json::to_string_pretty(&report) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("failed to serialize report: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            } else {
                println!("PASS");
                println!(
                    "puzzle: {} ({})",
                    report.puzzle_canonical_id, report.puzzle_version
                );
                println!("solution_stm: {}", report.solution_stm);
                if let Some(d) = report.durations.speedsolve {
                    println!("speedsolve_ms: {}", d.num_milliseconds());
                }
                if let Some(t) = report.verified_timestamps.completion {
                    println!("tsa_gen_time: {}", t.to_rfc3339());
                }
                if let (Some(start), Some(end)) = (
                    report.verified_timestamps.scramble_range_start,
                    report.verified_timestamps.scramble_range_end,
                ) {
                    println!(
                        "scramble_range: {} .. {}",
                        start.to_rfc3339(),
                        end.to_rfc3339()
                    );
                }
                if let Some(d) = report.durations.timestamp_network_latency {
                    println!("timestamp_network_latency_ms: {}", d.num_milliseconds());
                }
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            if cli.json {
                let value = serde_json::json!({
                    "ok": false,
                    "error": err.to_string(),
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| err.to_string())
                );
            } else {
                eprintln!("FAIL: {err:#}");
            }
            ExitCode::FAILURE
        }
    }
}
