//! TSA trust for Involute recordings.
//!
//! # Trust model
//!
//! - **Trusted anchors** — PEM bundle cached under `~/.cache/involute-verify`,
//!   refreshed every 10 days from [rfc3161.ai.moda](https://rfc3161.ai.moda)
//!   backends with `default_enabled: true` in `servers.json`.
//! - **Untrusted chain** — certificates extracted from the recording’s Time
//!   Stamp Response (`certReq: true`), passed to OpenSSL as `-untrusted`.
//!
//! Certificates that appear only in a recording are never treated as trust
//! anchors (no TOFU).

use std::fs;
use std::process::Command;

use tempfile::NamedTempFile;
use timecheck::tsa::Signature;

use crate::cache::{ensure_moda_ca_bundle, CacheConfig};
use crate::log::progress;

/// Inputs for OpenSSL `ts -verify` for one recording.
pub struct PreparedTsaTrust {
    /// Trusted anchors for `-CAfile`.
    pub ca_pem: String,
    /// Intermediates / leaf from the recording TSR for `-untrusted`.
    pub untrusted_pem: String,
    /// Whether to pass `-partial_chain` (bundle may include intermediates).
    pub partial_chain: bool,
}

/// Build trust material: cached moda CAs + TSR chain as untrusted.
pub fn prepare_trust_for_signature(
    cfg: &CacheConfig,
    signature: &Signature,
) -> anyhow::Result<PreparedTsaTrust> {
    let ca_pem = ensure_moda_ca_bundle(cfg)?;
    if ca_pem.trim().is_empty() || !ca_pem.contains("BEGIN CERTIFICATE") {
        anyhow::bail!("moda CA bundle is empty after cache refresh");
    }

    progress("  extracting cert chain from recording TSR (untrusted only)…");
    let chain_pem = extract_certs_from_tsr(signature)?;
    let n_certs = count_pem_certs(&chain_pem);
    if n_certs == 0 {
        anyhow::bail!("TSR contains no certificates; recording must be stamped with certReq: true");
    }
    progress(format!(
        "  TSR embedded {n_certs} certificate(s) → -untrusted"
    ));
    progress(format!(
        "  trusted anchors: moda bundle ({} certs, {} bytes)",
        count_pem_certs(&ca_pem),
        ca_pem.len()
    ));

    Ok(PreparedTsaTrust {
        ca_pem,
        untrusted_pem: chain_pem,
        partial_chain: true,
    })
}

/// Extract PEM certificates from a DER TimeStampResp via OpenSSL.
pub fn extract_certs_from_tsr(signature: &Signature) -> anyhow::Result<String> {
    let tsr = NamedTempFile::new()?;
    let token = NamedTempFile::new()?;
    let pem_out = NamedTempFile::new()?;

    let der = hex::decode(signature.to_string())?;
    fs::write(tsr.path(), &der)?;

    let token_status = Command::new("openssl")
        .args([
            "ts",
            "-reply",
            "-in",
            &tsr.path().display().to_string(),
            "-token_out",
            "-out",
            &token.path().display().to_string(),
        ])
        .output()?;
    if !token_status.status.success() {
        anyhow::bail!(
            "openssl ts -reply -token_out failed: {}",
            String::from_utf8_lossy(&token_status.stderr)
        );
    }

    let certs_status = Command::new("openssl")
        .args([
            "pkcs7",
            "-inform",
            "DER",
            "-in",
            &token.path().display().to_string(),
            "-print_certs",
            "-outform",
            "PEM",
            "-out",
            &pem_out.path().display().to_string(),
        ])
        .output()?;
    if !certs_status.status.success() {
        anyhow::bail!(
            "openssl pkcs7 -print_certs failed: {}",
            String::from_utf8_lossy(&certs_status.stderr)
        );
    }

    Ok(fs::read_to_string(pem_out.path())?)
}

fn count_pem_certs(pem: &str) -> usize {
    pem.matches("BEGIN CERTIFICATE").count()
}
