//! Runtime cache under `~/.cache/involute-verify`.
//!
//! - **Moda TSA CAs** — refreshed every 10 days from `servers.json` (`default_enabled`)
//! - **Puzzle defs** — downloaded on demand from involute.chandler.io by `(id, version)`

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::log::progress;

const SERVERS_JSON_URL: &str = "https://rfc3161.ai.moda/servers.json";
const DEFAULT_PUZZLE_BASE_URL: &str = "https://involute.chandler.io/puzzles";
const CERT_TTL: Duration = Duration::from_secs(10 * 24 * 60 * 60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_ATTEMPTS: u32 = 3;
const DOWNLOAD_BACKOFF: &[Duration] = &[
    Duration::from_millis(200),
    Duration::from_secs(1),
    Duration::from_secs(3),
];

/// Default cache root: `$XDG_CACHE_HOME/involute-verify` or `~/.cache/involute-verify`.
pub fn default_cache_dir() -> anyhow::Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        let p = PathBuf::from(xdg);
        if !p.as_os_str().is_empty() {
            return Ok(p.join("involute-verify"));
        }
    }
    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".cache/involute-verify"))
}

/// Configuration for cache lookups and network fetches.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub cache_dir: PathBuf,
    pub puzzle_base_url: String,
    pub force_refresh_certs: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_dir: default_cache_dir()
                .unwrap_or_else(|_| PathBuf::from(".cache/involute-verify")),
            puzzle_base_url: DEFAULT_PUZZLE_BASE_URL.to_string(),
            force_refresh_certs: false,
        }
    }
}

impl CacheConfig {
    fn moda_pem_path(&self) -> PathBuf {
        self.cache_dir.join("moda-cas.pem")
    }

    fn moda_meta_path(&self) -> PathBuf {
        self.cache_dir.join("moda-meta.json")
    }

    fn puzzle_dir(&self, id: &str, version: &str) -> PathBuf {
        self.cache_dir.join(id).join(version)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ModaMeta {
    fetched_at_unix: u64,
    servers_url: String,
    backends: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TsaServerInfo {
    name: String,
    url: String,
    #[serde(default)]
    default_enabled: bool,
}

#[derive(Debug, Deserialize)]
struct PuzzleIndex {
    puzzles: Vec<PuzzleIndexEntry>,
}

#[derive(Debug, Deserialize)]
struct PuzzleIndexEntry {
    id: String,
    version: String,
    files: PuzzleIndexFiles,
}

#[derive(Debug, Deserialize)]
struct PuzzleIndexFiles {
    #[serde(rename = "data.yaml")]
    data_yaml: FileDigest,
    #[serde(rename = "rejection-cache.bin")]
    rejection_cache: FileDigest,
}

#[derive(Debug, Deserialize)]
struct FileDigest {
    sha256: String,
}

/// Ensure moda CA bundle is present and not older than 10 days; return PEM text.
pub fn ensure_moda_ca_bundle(cfg: &CacheConfig) -> anyhow::Result<String> {
    fs::create_dir_all(&cfg.cache_dir)?;
    let pem_path = cfg.moda_pem_path();
    let meta_path = cfg.moda_meta_path();

    let stale = cfg.force_refresh_certs || is_cert_cache_stale(&pem_path, &meta_path);
    if !stale {
        let pem = fs::read_to_string(&pem_path)?;
        if pem.contains("BEGIN CERTIFICATE") {
            progress(format!(
                "  using cached moda CA bundle ({} certs)",
                pem.matches("BEGIN CERTIFICATE").count()
            ));
            return Ok(pem);
        }
    }

    progress(if cfg.force_refresh_certs {
        "  refreshing moda CA bundle (--force-refresh-certs)…"
    } else if pem_path.is_file() {
        "  moda CA bundle older than 10 days; refreshing…"
    } else {
        "  downloading moda CA bundle (first run)…"
    });

    match refresh_moda_ca_bundle(cfg) {
        Ok(pem) => Ok(pem),
        Err(err) => {
            if pem_path.is_file() {
                let pem = fs::read_to_string(&pem_path)?;
                if pem.contains("BEGIN CERTIFICATE") {
                    progress(format!(
                        "  warning: moda refresh failed ({err:#}); using stale cached bundle"
                    ));
                    return Ok(pem);
                }
            }
            Err(err).context("failed to download moda CA bundle and no usable cache")
        }
    }
}

fn is_cert_cache_stale(pem_path: &Path, meta_path: &Path) -> bool {
    if !pem_path.is_file() || !meta_path.is_file() {
        return true;
    }
    let Ok(raw) = fs::read_to_string(meta_path) else {
        return true;
    };
    let Ok(meta) = serde_json::from_str::<ModaMeta>(&raw) else {
        return true;
    };
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return true;
    };
    now.as_secs().saturating_sub(meta.fetched_at_unix) >= CERT_TTL.as_secs()
}

fn refresh_moda_ca_bundle(cfg: &CacheConfig) -> anyhow::Result<String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(PROBE_TIMEOUT))
        .http_status_as_error(true)
        .build()
        .new_agent();

    let all_servers: Vec<TsaServerInfo> = agent
        .get(SERVERS_JSON_URL)
        .call()
        .map_err(|e| anyhow::anyhow!("fetch {SERVERS_JSON_URL}: {e}"))?
        .into_body()
        .read_json()
        .map_err(|e| anyhow::anyhow!("parse {SERVERS_JSON_URL}: {e}"))?;
    let servers: Vec<TsaServerInfo> = all_servers
        .into_iter()
        .filter(|s| s.default_enabled)
        .collect();
    if servers.is_empty() {
        anyhow::bail!("{SERVERS_JSON_URL}: no backends with default_enabled=true");
    }

    let mut combined = String::new();
    let mut backends = Vec::new();
    let total = servers.len();

    for (i, server) in servers.iter().enumerate() {
        match probe_server_chain(&agent, &server.url) {
            Ok(pem) => {
                let n = split_pem_blocks(&pem).len();
                progress(format!(
                    "    [{}/{total}] {}: ok — {n} cert(s)",
                    i + 1,
                    server.name
                ));
                combined.push_str(&format!("# {}\n", server.name));
                combined.push_str(pem.trim());
                combined.push('\n');
                backends.push(server.name.clone());
            }
            Err(e) => {
                progress(format!(
                    "    [{}/{total}] {}: failed — {e}",
                    i + 1,
                    server.name
                ));
            }
        }
    }

    if backends.is_empty() {
        anyhow::bail!("no moda default_enabled backends returned usable certificate chains");
    }

    let deduped = dedupe_pem_blocks(&combined);
    let header = format!("# Cached by involute-verify from {SERVERS_JSON_URL}\n\n");
    let pem = format!("{header}{deduped}");
    atomic_write(&cfg.moda_pem_path(), pem.as_bytes())?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let meta = ModaMeta {
        fetched_at_unix: now,
        servers_url: SERVERS_JSON_URL.to_string(),
        backends,
    };
    atomic_write(
        &cfg.moda_meta_path(),
        serde_json::to_string_pretty(&meta)?.as_bytes(),
    )?;

    progress(format!(
        "  cached moda CA bundle ({} unique certs)",
        deduped.matches("BEGIN CERTIFICATE").count()
    ));
    Ok(pem)
}

/// Ensure puzzle files for `(id, version)` are on disk; return their directory.
pub fn ensure_puzzle(cfg: &CacheConfig, id: &str, version: &str) -> anyhow::Result<PathBuf> {
    validate_path_segment(id, "puzzleId")?;
    validate_path_segment(version, "puzzleVersion")?;

    let dir = cfg.puzzle_dir(id, version);
    let yaml_path = dir.join("data.yaml");
    let cache_path = dir.join("rejection-cache.bin");
    if yaml_path.is_file() && cache_path.is_file() {
        progress(format!("  using cached puzzle {id}@{version}"));
        return Ok(dir);
    }

    progress(format!("  downloading puzzle {id}@{version}…"));
    fs::create_dir_all(&dir)?;

    let base = cfg.puzzle_base_url.trim_end_matches('/');
    let yaml_url = format!("{base}/{id}/{version}/data.yaml");
    let cache_url = format!("{base}/{id}/{version}/rejection-cache.bin");

    let expected = fetch_expected_digests(cfg, id, version).ok();

    let yaml_bytes = download_with_retries(&yaml_url)?;
    let cache_bytes = download_with_retries(&cache_url)?;

    if let Some((yaml_sha, cache_sha)) = expected {
        verify_sha256(&yaml_bytes, &yaml_sha, "data.yaml")?;
        verify_sha256(&cache_bytes, &cache_sha, "rejection-cache.bin")?;
    }

    atomic_write(&yaml_path, &yaml_bytes)?;
    atomic_write(&cache_path, &cache_bytes)?;
    progress(format!(
        "  cached puzzle {id}@{version} (yaml {} B, rejection-cache {} B)",
        yaml_bytes.len(),
        cache_bytes.len()
    ));
    Ok(dir)
}

fn fetch_expected_digests(
    cfg: &CacheConfig,
    id: &str,
    version: &str,
) -> anyhow::Result<(String, String)> {
    let base = cfg.puzzle_base_url.trim_end_matches('/');
    let index_url = format!("{base}/index.json");
    let body = download_with_retries(&index_url)?;
    let index: PuzzleIndex =
        serde_json::from_slice(&body).map_err(|e| anyhow::anyhow!("parse {index_url}: {e}"))?;
    let entry = index
        .puzzles
        .into_iter()
        .find(|p| p.id == id && p.version == version)
        .ok_or_else(|| anyhow::anyhow!("{index_url}: no entry for {id}@{version}"))?;
    Ok((
        entry.files.data_yaml.sha256,
        entry.files.rejection_cache.sha256,
    ))
}

fn verify_sha256(bytes: &[u8], expected_hex: &str, label: &str) -> anyhow::Result<()> {
    let got = hex::encode(Sha256::digest(bytes));
    if !got.eq_ignore_ascii_case(expected_hex) {
        anyhow::bail!("{label} sha256 mismatch: got {got}, expected {expected_hex}");
    }
    Ok(())
}

fn download_with_retries(url: &str) -> anyhow::Result<Vec<u8>> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .http_status_as_error(true)
        .build()
        .new_agent();

    let mut last_err = None;
    for attempt in 0..DOWNLOAD_ATTEMPTS {
        match agent.get(url).call() {
            Ok(resp) => {
                return resp
                    .into_body()
                    .read_to_vec()
                    .map_err(|e| anyhow::anyhow!("read {url}: {e}"));
            }
            Err(e) => {
                last_err = Some(anyhow::anyhow!("GET {url}: {e}"));
                if let Some(delay) = DOWNLOAD_BACKOFF.get(attempt as usize) {
                    thread::sleep(*delay);
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("GET {url} failed")))
}

fn validate_path_segment(s: &str, label: &str) -> anyhow::Result<()> {
    if s.is_empty() || s.contains('/') || s.contains('\\') || s.contains("..") || s.contains('\0') {
        anyhow::bail!("invalid {label}: {s:?}");
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn probe_server_chain(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let data_path = dir.path().join("data.bin");
    let query_path = dir.path().join("req.tsq");
    let tsr_path = dir.path().join("resp.tsr");
    let token_path = dir.path().join("token.p7");
    let pem_path = dir.path().join("certs.pem");
    fs::write(&data_path, [0xABu8; 32]).map_err(|e| e.to_string())?;

    let query_status = Command::new("openssl")
        .args([
            "ts",
            "-query",
            "-data",
            &data_path.display().to_string(),
            "-cert",
            "-sha256",
            "-out",
            &query_path.display().to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .map_err(|e| e.to_string())?;
    if !query_status.success() {
        return Err("openssl ts -query failed".into());
    }

    let query_bytes = fs::read(&query_path).map_err(|e| e.to_string())?;
    let body = agent
        .post(url)
        .header("Content-Type", "application/timestamp-query")
        .send(&query_bytes)
        .map_err(|e| format!("POST {url}: {e}"))?
        .into_body()
        .read_to_vec()
        .map_err(|e| format!("read TSR: {e}"))?;
    fs::write(&tsr_path, &body).map_err(|e| e.to_string())?;

    let token_out = Command::new("openssl")
        .args([
            "ts",
            "-reply",
            "-in",
            &tsr_path.display().to_string(),
            "-token_out",
            "-out",
            &token_path.display().to_string(),
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !token_out.status.success() {
        return Err(format!(
            "openssl ts -reply: {}",
            String::from_utf8_lossy(&token_out.stderr)
        ));
    }

    let certs_out = Command::new("openssl")
        .args([
            "pkcs7",
            "-inform",
            "DER",
            "-in",
            &token_path.display().to_string(),
            "-print_certs",
            "-outform",
            "PEM",
            "-out",
            &pem_path.display().to_string(),
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !certs_out.status.success() {
        return Err(format!(
            "openssl pkcs7 -print_certs: {}",
            String::from_utf8_lossy(&certs_out.stderr)
        ));
    }

    fs::read_to_string(&pem_path).map_err(|e| e.to_string())
}

fn split_pem_blocks(pem: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_cert = false;
    for line in pem.lines() {
        if line.contains("BEGIN CERTIFICATE") {
            in_cert = true;
            current.clear();
            current.push_str(line);
            current.push('\n');
            continue;
        }
        if in_cert {
            current.push_str(line);
            current.push('\n');
            if line.contains("END CERTIFICATE") {
                in_cert = false;
                out.push(current.clone());
            }
        }
    }
    out
}

fn dedupe_pem_blocks(pem: &str) -> String {
    let mut out = String::new();
    let mut seen = HashSet::new();
    for block in split_pem_blocks(pem) {
        if seen.insert(block.clone()) {
            out.push_str(&block);
        }
    }
    out
}
